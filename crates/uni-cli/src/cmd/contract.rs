use crate::{dot_uni, InitProfile};
use anyhow::{anyhow, Context, Result};
use std::path::Path;

pub(crate) fn cmd_lint(file: &Path, as_json: bool) -> Result<()> {
    let (ir, _) = crate::cmd::contract::load_contract(file)?;
    let registry = uni_verify::load_registry(&dot_uni());
    // severity 2 = error, 1 = warning. The claim id travels as data: it used to
    // be re-parsed out of the message text, which any wording change broke.
    struct Finding {
        severity: u8,
        kind: &'static str,
        claim_id: String,
        message: String,
    }
    let mut findings: Vec<Finding> = vec![];
    // A local macro, not a closure: the closure would borrow `findings` for its
    // whole lifetime and block the reads below.
    macro_rules! push {
        ($sev:expr, $kind:expr, $cid:expr, $msg:expr) => {
            findings.push(Finding {
                severity: $sev,
                kind: $kind,
                claim_id: $cid.to_string(),
                message: $msg,
            })
        };
    }
    for c in &ir.claims {
        if !ir.verification.iter().any(|v| v.claim_id == c.id) {
            push!(
                2,
                "missing-verify",
                &c.id,
                format!("claim '{}' has no VERIFY", c.id)
            );
        }
    }
    if !registry.is_empty() {
        for v in &ir.verification {
            if v.verifier_ref != "shell" && !registry.contains_key(&v.verifier_ref) {
                push!(
                    1,
                    "unknown-verifier",
                    &v.claim_id,
                    format!(
                    "VERIFY {} uses '{}' not found in .uni/config.toml (may fail at verify time)",
                    v.claim_id, v.verifier_ref
                )
                );
            }
        }
    }
    for (i, a) in ir.verification.iter().enumerate() {
        for b in ir.verification.iter().skip(i + 1) {
            if b.claim_id == a.claim_id
                && b.verifier_ref == a.verifier_ref
                && b.inline_shell == a.inline_shell
            {
                push!(
                    1,
                    "duplicate-verify",
                    &a.claim_id,
                    format!(
                        "VERIFY '{}' → '{}' declared twice",
                        a.claim_id, a.verifier_ref
                    )
                );
            }
        }
    }
    // v0.4: a selector-template verifier needs an authorized selector binding,
    // and the specific message beats the generic unbound-requirement warning.
    for v in &ir.verification {
        let Some(spec) = registry.get(&v.verifier_ref) else {
            continue;
        };
        if !uni_verify::is_selector_template(spec) {
            continue;
        }
        let ok = match uni_evidence::binding::load_binding(&dot_uni(), &v.claim_id) {
            Some(b) => {
                b.verifier_ref == v.verifier_ref
                    && b.requirement == v.requirement.clone().unwrap_or_default()
                    && b.selector
                        .as_deref()
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false)
            }
            None => false,
        };
        if !ok {
            push!(1, "selector-template", &v.claim_id, format!(
                "claim '{}' uses selector template '{}' without an authorized selector binding (run: uni bind --claim {} --verifier {} --selector <test-name>)",
                v.claim_id, v.verifier_ref, v.claim_id, v.verifier_ref
            ));
        }
    }
    // A zero-match watcher is a common configuration error: lint reports it
    // before the verifier runs. This remains a warning, because a verifier
    // may legitimately generate a watched artifact during its execution.
    for v in &ir.verification {
        let Some(spec) = registry.get(&v.verifier_ref) else {
            continue;
        };
        let patterns = if spec.files.is_empty() && spec.kind == "file-hash" {
            spec.expect_sha256.keys().cloned().collect::<Vec<_>>()
        } else {
            spec.files.clone()
        };
        for pattern in patterns {
            let mut single = spec.clone();
            single.kind = "shell".to_string();
            single.expect_sha256.clear();
            single.files = vec![pattern.clone()];
            if uni_verify::subject_snapshot(&single, Path::new("."))
                .digest
                .is_none()
            {
                push!(
                    1,
                    "empty-watched-subject",
                    &v.claim_id,
                    format!(
                        "claim '{}' verifier '{}' watches '{}' but no safe matching files were observed (Git tracked-only mode requires git add); check scope before verify",
                        v.claim_id, v.verifier_ref, pattern
                    )
                );
            }
        }
    }
    let selector_flagged: Vec<String> = findings
        .iter()
        .filter(|f| f.kind == "selector-template")
        .map(|f| f.claim_id.clone())
        .collect();
    // ADR-002: a test name baked into the registry command is a contract
    // smell (the study's whole false-rejection class). Warning only: the
    // decision engine is untouched, but new contracts get nudged toward a
    // {{selector}} template plus `uni brief` as the handoff.
    for v in &ir.verification {
        let Some(spec) = registry.get(&v.verifier_ref) else {
            continue;
        };
        if !uni_verify::pinned_test_selector(spec) {
            continue;
        }
        push!(1, "pinned-test-selector", &v.claim_id, format!(
            "claim '{}' pins a test name in verifier '{}'; prefer a {{{{selector}}}} template with 'uni bind --selector <test-name>' and hand the worker 'uni brief' (ADR-002)",
            v.claim_id, v.verifier_ref
        ));
    }
    for v in ir.verification.iter().filter(|v| v.requirement.is_some()) {
        if selector_flagged.contains(&v.claim_id) {
            continue; // the specific selector message already says what to do
        }
        let req = v.requirement.as_deref().unwrap_or("");
        match uni_evidence::binding::load_binding(&dot_uni(), &v.claim_id) {
            Some(b) if b.verifier_ref == v.verifier_ref && b.requirement == req => {}
            _ => push!(1, "unbound-requirement", &v.claim_id, format!(
                "claim '{}' has a REQUIRE but no matching authorized binding (run: uni bind --claim {} --verifier {} --requirement '...')",
                v.claim_id, v.claim_id, v.verifier_ref
            )),
        }
    }
    let errors = findings.iter().filter(|f| f.severity == 2).count();
    let warns = findings.len() - errors;
    if as_json {
        println!(
            "{}",
            serde_json::json!({
                "intent": ir.intent.id,
                "errors": errors,
                "warnings": warns,
                "findings": findings.iter().map(|f| serde_json::json!({
                    "severity": if f.severity == 2 { "error" } else { "warning" },
                    "kind": f.kind,
                    "claim_id": f.claim_id,
                    "message": f.message,
                })).collect::<Vec<_>>(),
            })
        );
    } else {
        println!("uni lint — {}", ir.intent.id);
        for f in &findings {
            println!(
                "  {} {:<18} {}",
                if f.severity == 2 { "ERROR" } else { "WARN " },
                f.kind,
                f.message
            );
        }
        if findings.is_empty() {
            println!("  clean: coverage complete, registry refs ok");
        }
        println!("\nerrors={errors} warnings={warns}");
    }
    if errors > 0 {
        return Err(anyhow!("uni lint: {errors} error(s)"));
    }
    Ok(())
}

pub(crate) fn cmd_init(profile: InitProfile) -> Result<()> {
    // `policies` is created empty because the policy layer reads
    // `.uni/policies/*.toml`: a fresh workspace should be a place you can put a
    // policy, not one where you first have to mkdir.
    for d in [
        "contracts",
        "evidence",
        "decisions",
        "artifacts",
        "policies",
    ] {
        std::fs::create_dir_all(dot_uni().join(d))?;
    }
    std::fs::create_dir_all("uni/intents")?;
    let cfg = dot_uni().join("config.toml");
    if !cfg.exists() {
        let registry = match profile {
            InitProfile::Rust => {
                r#"[verifiers]
"project.check" = "cargo check"
"project.tests" = "cargo test"
"project.build" = "cargo build"
"#
            }
            InitProfile::Node => {
                r#"[verifiers]
"project.check" = "npm run typecheck"
"project.tests" = "npm test"
"project.build" = "npm run build"
"#
            }
            InitProfile::Python => {
                r#"[verifiers]
"project.check" = "python -m compileall -q ."
"project.tests" = "python -m unittest discover"
"#
            }
        };
        std::fs::write(&cfg, registry)?;
    }
    println!("initialized .uni/ + uni/intents/");
    Ok(())
}

pub(crate) fn load_contract(file: &Path) -> Result<(uni_ir::Ir, uni_parser::Contract)> {
    let src = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let ast = uni_parser::parse(&src)?;
    let ir = uni_ir::compile(&ast)?;
    Ok((ir, ast))
}

pub(crate) fn cmd_compile(file: &Path, as_json: bool) -> Result<()> {
    let (ir, _) = crate::cmd::contract::load_contract(file)?;
    let out = uni_ir::to_json(&ir)?;
    if as_json {
        println!("{out}");
    } else {
        println!("intent: {}", ir.intent.id);
        println!("claims: {}", ir.claims.len());
        println!("verifications: {}", ir.verification.len());
        println!("--json for canonical IR");
    }
    Ok(())
}

pub(crate) fn cmd_inspect(file: &Path, as_json: bool) -> Result<()> {
    cmd_compile(file, as_json)
}
