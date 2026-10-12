use crate::{dot_uni, events};
use anyhow::{anyhow, Result};
use std::path::Path;
use wait_timeout::ChildExt;

/// The execute half of the loop: run the command the human chose, then verify
/// the contract. The command comes from the command line, never from the
/// contract, so this adds no trust surface; the executor's exit code is
/// reported but never decides, because only evidence decides.
pub(crate) fn cmd_run(
    file: &Path,
    command: &str,
    actor: Option<&str>,
    timeout_ms: u64,
    brief: bool,
    as_json: bool,
) -> Result<()> {
    use std::process::Stdio;
    let ws = std::env::current_dir()?;
    let started = std::time::Instant::now();

    // `--brief` writes the work order and points the executor at it, because
    // the study showed that a brief nobody is told to read is decoration. It is
    // opt-in: `uni run` keeps doing exactly what it did before by default, and
    // the executor still receives the command line untouched.
    let mut brief_env: Option<(String, std::path::PathBuf)> = None;
    if brief {
        let body = crate::cmd::handoff::render_brief(file, false)?;
        let path = dot_uni().join("brief.md");
        crate::cmd::handoff::write_atomic(&path, &format!("{body}\n"))?;
        let abs = ws.join(&path);
        if !as_json {
            eprintln!(
                "[uni] work order written to {}; UNI_BRIEF points the executor at it",
                abs.display()
            );
        }
        brief_env = Some(("UNI_BRIEF".into(), abs));
    }

    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(&ws)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some((key, value)) = &brief_env {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn()?;
    let timeout = std::time::Duration::from_millis(timeout_ms.max(1));
    let (executor_code, executor_output) = match child.wait_timeout(timeout)? {
        Some(_) => {
            let out = child.wait_with_output()?;
            let code = out.status.code().unwrap_or(-1);
            let text = [out.stdout, out.stderr].concat();
            (code, String::from_utf8_lossy(&text).to_string())
        }
        None => {
            let _ = child.kill();
            let out = child.wait_with_output()?;
            let mut text = String::from_utf8_lossy(&[out.stdout, out.stderr].concat()).to_string();
            text.push_str(&format!("\n[uni] executor killed after {timeout_ms}ms\n"));
            (-1, text)
        }
    };
    if !as_json {
        print!("{executor_output}");
        if !executor_output.ends_with('\n') {
            println!();
        }
        println!(
            "[uni] executor exit {executor_code} in {}ms; verifying the outcome",
            started.elapsed().as_millis()
        );
    }

    let journal_path = dot_uni();
    let _ = events::append(&[events::Event {
        name: "ExecutionRun",
        attrs: vec![
            ("uni.execution.command".into(), command.to_string()),
            ("uni.execution.exit_code".into(), executor_code.to_string()),
            (
                "uni.execution.duration_ms".into(),
                started.elapsed().as_millis().to_string(),
            ),
        ],
    }]);
    let _ = journal_path;

    // The executor's exit code is data, not truth: the decision is the verdict.
    cmd_verify(file, as_json, actor, false)
}

/// Authorized resolution of a claim's verification (v0.4).
#[derive(Default)]
struct Resolution {
    binding_hash: Option<String>,
    selector: Option<String>,
}

/// Gate: a verification runs only under an authorized binding when it either
/// declares a resolution REQUIRE (v0.2) or uses a `{{selector}}` template
/// (v0.4). A template plus an authorization is what removes the study's
/// evidence-name coupling: the worker names the test, the human authorizes it.
fn require_binding(
    du: &Path,
    claim_id: &str,
    verifier_ref: &str,
    requirement: Option<&str>,
    is_template: bool,
) -> Result<Resolution> {
    let req = requirement.unwrap_or("");
    if !is_template && requirement.is_none() {
        return Ok(Resolution::default());
    }
    // The remediation must name the act that actually works: a template needs
    // a selector, a plain REQUIRE needs its requirement text.
    let remediation = if is_template {
        format!("uni bind --claim {claim_id} --verifier {verifier_ref} --selector <test-name>")
    } else {
        format!("uni bind --claim {claim_id} --verifier {verifier_ref} --requirement '{req}'")
    };
    match uni_evidence::binding::load_binding(du, claim_id) {
        Some(b) if b.verifier_ref == verifier_ref && b.requirement == req => {
            if is_template && b.selector.as_deref().unwrap_or("").trim().is_empty() {
                return Err(anyhow!(
                    "claim '{claim_id}' uses a selector-template verifier but its binding has no selector; authorize with: uni bind --claim {claim_id} --verifier {verifier_ref} --selector <test-name>"
                ));
            }
            Ok(Resolution {
                binding_hash: Some(b.binding_hash),
                selector: b.selector,
            })
        }
        Some(b) => Err(anyhow!(
            "claim '{claim_id}' was rebound (bound: requirement '{}' verifier '{}' selector {:?}; contract expects requirement '{req}' on verifier '{verifier_ref}'); re-authorize with: {remediation}",
            b.requirement,
            b.verifier_ref,
            b.selector,
        )),
        None => Err(anyhow!(
            "claim '{claim_id}' needs an authorized binding (requirement '{req}', template {is_template}) but none exists; authorize with: {remediation}"
        )),
    }
}

pub(crate) fn cmd_verify(
    file: &Path,
    as_json: bool,
    actor_flag: Option<&str>,
    attest: bool,
) -> Result<()> {
    if attest {
        return Err(anyhow!(
            "signed provenance (A4) is reserved: no signer is configured in v0.2 (see docs/decisions.md)"
        ));
    }
    let (ir, _) = crate::cmd::contract::load_contract(file)?;
    let ws = std::env::current_dir()?;
    let du = dot_uni();
    // B3 actor model: the executor is whoever runs this command (local,
    // self-declared, capped at A2). The verifier actor is proven in one of two
    // ways: a JWT in UNI_IDENTITY_TOKEN, verified against an issuer pinned in
    // .uni/config.toml [identities] (A3), or `--actor`, a self-declared
    // identity that reaches no further than A3-D.
    let executor = uni_evidence::Actor::local();
    let identity_token = std::env::var("UNI_IDENTITY_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty());
    let (actor, identity_verified) = match identity_token.as_deref() {
        Some(token) => {
            if actor_flag.is_some() {
                return Err(anyhow!(
                    "--actor and UNI_IDENTITY_TOKEN are mutually exclusive: the verified token names the actor"
                ));
            }
            let identities = uni_verify::identity::load_identities(&du, &ws)?;
            (
                uni_verify::identity::verify_token(token, &identities)?,
                true,
            )
        }
        None => (
            match actor_flag {
                Some(id) => uni_evidence::Actor::declared(id),
                None => executor.clone(),
            },
            false,
        ),
    };
    let external_scheme = actor_flag.map(|id| id.split_once("://").map(|(s, _)| s).unwrap_or(""));
    // A scheme prefix without a token is a declaration, not a proof: recorded
    // explicitly, and it stays self-declared.
    let identity_unverified_warning = matches!(
        external_scheme,
        Some("spiffe") | Some("entra") | Some("oidc")
    );
    let (cur_sha, cur_dirty) = uni_evidence::git_info(&ws);
    let registry = uni_verify::load_registry(&du);
    // Verification Context, computed once per run: any drift on these
    // dimensions invalidates stored evidence (B1); policy drift instead
    // forces a decision recompute, which every verify does anyway.
    let contract_text = std::fs::read_to_string(file).unwrap_or_default();
    let contract_hash = uni_evidence::sha256_hex(contract_text.as_bytes());
    let registry_text = std::fs::read_to_string(du.join("config.toml")).unwrap_or_default();
    let registry_hash = uni_evidence::sha256_hex(registry_text.as_bytes());
    let platform = uni_evidence::platform();
    // B2: trust-boundary diff against the last acknowledged registry snapshot.
    // A changed registry never silently reuses old evidence (B1 already stales
    // it); here we name what changed and flag it for CI/human review.
    let prev_registry_hash = std::fs::read_to_string(du.join(".registry.hash"))
        .unwrap_or_default()
        .trim()
        .to_string();
    let prev_registry_text =
        std::fs::read_to_string(du.join(".registry.snapshot.toml")).unwrap_or_default();
    let trust_boundary_changed =
        !prev_registry_hash.is_empty() && prev_registry_hash != registry_hash;
    let tb_diff = if trust_boundary_changed {
        uni_verify::registry_diff(&prev_registry_text, &registry_text)
    } else {
        uni_verify::RegistryDiff::default()
    };
    if trust_boundary_changed && !as_json {
        println!("REGISTRY_CHANGED");
        println!(
            "Previous: sha256:{}",
            &prev_registry_hash[..12.min(prev_registry_hash.len())]
        );
        println!("Current:  sha256:{}", &registry_hash[..12]);
        println!(
            "Existing evidence: STALE\nAuthorization: REQUIRED ({} added, {} removed, {} changed)",
            tb_diff.added.len(),
            tb_diff.removed.len(),
            tb_diff.changed.len()
        );
        for name in tb_diff
            .added
            .iter()
            .chain(tb_diff.removed.iter())
            .chain(tb_diff.changed.iter())
            .take(10)
        {
            println!("  - {name}");
        }
    }
    // 1) Try persisted evidence first (cheap, content-addressed).
    let mut stored = vec![];
    let mut need_run = vec![];
    let mut stale_ids: Vec<String> = vec![];
    let mut stale_reasons: std::collections::BTreeMap<String, Vec<uni_evidence::StaleReason>> =
        std::collections::BTreeMap::new();
    let mut journal: Vec<events::Event> = vec![events::Event {
        name: "IntentVerified",
        attrs: vec![
            ("uni.intent.id".into(), ir.intent.id.clone()),
            ("uni.contract.version".into(), ir.uni_version.clone()),
        ],
    }];
    if identity_verified {
        journal.push(events::Event {
            name: "IdentityVerified",
            attrs: vec![
                ("uni.actor.id".into(), actor.id.clone()),
                ("uni.actor.source".into(), actor.source.clone()),
                ("uni.actor.assurance".into(), actor.assurance.clone()),
            ],
        });
    }
    if identity_unverified_warning {
        journal.push(events::Event {
            name: "IdentityUnverified",
            attrs: vec![
                ("uni.actor.id".into(), actor.id.clone()),
                ("uni.actor.source".into(), actor.source.clone()),
                ("uni.actor.assurance".into(), actor.assurance.clone()),
            ],
        });
    }
    if trust_boundary_changed {
        journal.push(events::Event {
            name: "RegistryChanged",
            attrs: vec![
                (
                    "uni.registry.previous".into(),
                    prev_registry_hash[..8.min(prev_registry_hash.len())].into(),
                ),
                ("uni.registry.current".into(), registry_hash[..8].into()),
                ("uni.registry.added".into(), tb_diff.added.join(",")),
                ("uni.registry.removed".into(), tb_diff.removed.join(",")),
                ("uni.registry.changed".into(), tb_diff.changed.join(",")),
            ],
        });
    }
    for v in &ir.verification {
        // v0.2 authorization gate: a verification carrying a resolution
        // requirement executes ONLY under a matching authorized binding.
        // No binding, or a binding for different text/verifier -> hard error,
        // never a silent run. AI may propose; only `uni bind` authorizes.
        let base_spec = registry.get(&v.verifier_ref).cloned();
        let is_template = base_spec
            .as_ref()
            .map(uni_verify::is_selector_template)
            .unwrap_or(false);
        let resolution = require_binding(
            &du,
            &v.claim_id,
            &v.verifier_ref,
            v.requirement.as_deref(),
            is_template,
        )?;
        // Resolve the selector into the command before anything is hashed or
        // fingerprinted: two selectors on the same key are different proofs.
        let resolved_spec = match &base_spec {
            Some(spec) => Some(uni_verify::with_selector(
                spec,
                resolution.selector.as_deref(),
            )?),
            None => None,
        };
        let binding_hash = resolution.binding_hash.clone();
        // content-bound evidence: hash computed from the verifier's watched files
        let subject = resolved_spec
            .as_ref()
            .map(|spec| uni_verify::subject_snapshot(spec, &ws))
            .unwrap_or_default();
        let current_ah = subject.digest;
        let current_af = subject.files;
        let fingerprint = match &resolved_spec {
            Some(spec) => uni_verify::spec_fingerprint(&v.verifier_ref, spec, &actor.id),
            None => format!("inline:{}:{}", v.verifier_ref, actor.id),
        };
        let ctx = uni_evidence::EvidenceContext {
            fingerprint,
            commit_sha: cur_sha.clone(),
            workspace_dirty: cur_dirty,
            artifact_hash: current_ah,
            artifact_files: current_af,
            registry_hash: registry_hash.clone(),
            contract_hash: contract_hash.clone(),
            platform: platform.clone(),
            binding_hash: binding_hash.clone(),
        };
        match uni_evidence::load_valid_for_claim(&du, &v.claim_id, &ctx) {
            uni_evidence::CacheOutcome::Hit(ev) => {
                // The proof is reused, but the decision context is now:
                // independence is evaluated against the CURRENT executor.
                let mut ev = *ev;
                ev.executor = executor.clone();
                journal.push(events::Event {
                    name: "EvidenceReused",
                    attrs: vec![
                        ("uni.claim.id".into(), v.claim_id.clone()),
                        ("uni.intent.id".into(), ir.intent.id.clone()),
                    ],
                });
                stored.push(ev)
            }
            uni_evidence::CacheOutcome::Stale(reasons) => {
                stale_reasons.insert(v.claim_id.clone(), reasons.clone());
                journal.push(events::Event {
                    name: "EvidenceStale",
                    attrs: vec![
                        ("uni.claim.id".into(), v.claim_id.clone()),
                        ("uni.intent.id".into(), ir.intent.id.clone()),
                        (
                            "uni.stale.reasons".into(),
                            reasons
                                .iter()
                                .map(|r| r.label())
                                .collect::<Vec<_>>()
                                .join(","),
                        ),
                        (
                            "uni.stale.detail".into(),
                            reasons
                                .iter()
                                .map(|r| r.describe())
                                .collect::<Vec<_>>()
                                .join(" | "),
                        ),
                    ],
                });
                stale_ids.push(v.claim_id.clone());
                need_run.push((
                    v.claim_id.clone(),
                    v.verifier_ref.clone(),
                    v.inline_shell.clone(),
                    binding_hash.clone(),
                    resolved_spec.clone(),
                ));
            }
            uni_evidence::CacheOutcome::Miss => need_run.push((
                v.claim_id.clone(),
                v.verifier_ref.clone(),
                v.inline_shell.clone(),
                binding_hash.clone(),
                resolved_spec.clone(),
            )),
        }
    }
    // Policy source selection: OPA bundle when both rego + opa binary exist, else TOML stack.
    // Resolved BEFORE running verifiers so fresh evidence records the policy
    // it was gathered under (audit dimension; drift forces recompute, not re-run).
    let opa_bundle = du.join("policies/opa.rego");
    let policies_dir = du.join("policies");
    let opa_available = opa_bundle.exists()
        && std::process::Command::new("opa")
            .arg("version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
    let provider: Box<dyn uni_decision::PolicyProvider> = if opa_available {
        Box::new(uni_decision::OpaPolicy { bundle: opa_bundle })
    } else {
        Box::new(uni_decision::TomlPolicy { dir: &policies_dir })
    };
    let policy = provider.resolve();
    let policy_hash = uni_evidence::sha256_hex(
        serde_json::to_string(&policy)
            .unwrap_or_default()
            .as_bytes(),
    );
    // 2) Re-run only for missing/stale/invalid claims (unlocked: reruns are
    // idempotent and deterministic, so concurrent runs only duplicate work).
    let mut fresh: Vec<(uni_evidence::Evidence, String)> = vec![];
    for (claim_id, ref_r, inline, binding, resolved) in need_run {
        // A resolved template comes back pre-substituted; plain verifiers are
        // still resolved through the trusted registry.
        let spec = match resolved {
            Some(spec) => spec,
            None => uni_verify::resolve_command(&ref_r, inline.as_deref(), &registry)?,
        };
        journal.push(events::Event {
            name: "EvidenceRun",
            attrs: vec![
                ("uni.claim.id".into(), claim_id.clone()),
                ("uni.verifier.id".into(), ref_r.clone()),
                ("uni.intent.id".into(), ir.intent.id.clone()),
            ],
        });
        let fingerprint = uni_verify::spec_fingerprint(&ref_r, &spec, &actor.id);
        let mut ev = uni_verify::run_spec(
            &claim_id,
            &ref_r,
            &spec,
            &ws,
            spec.timeout,
            &actor,
            &executor,
        )?;
        ev.registry_hash = registry_hash.clone();
        ev.contract_hash = contract_hash.clone();
        ev.platform = platform.clone();
        ev.policy_hash = policy_hash.clone();
        ev.binding_hash = binding.unwrap_or_default();
        if uni_evidence::is_stale(&ev, &cur_sha, cur_dirty) {
            ev.state = uni_evidence::EvidenceState::Stale;
        }
        fresh.push((ev, fingerprint));
    }
    for (ev, _) in &fresh {
        stored.push(ev.clone());
    }
    // Policy already resolved above (recorded on fresh evidence for audit).
    let mut decision =
        uni_decision::apply_policy(uni_decision::evaluate_intent(&ir, &stored), &policy);
    // Stale-but-unreprovable escalation: a claim whose previous proof drifted
    // out of context and could NOT be re-proven needs a human, not a retry.
    // (When the re-run succeeds the claim is Valid and this never fires.)
    if policy.escalate_on_stale && decision.decision != uni_decision::Decision::Accepted {
        let valid: std::collections::HashSet<&str> = decision
            .claims
            .iter()
            .filter(|c| c.state == uni_evidence::EvidenceState::Valid)
            .map(|c| c.claim_id.as_str())
            .collect();
        if stale_ids.iter().any(|id| !valid.contains(id.as_str())) {
            decision.decision = uni_decision::Decision::Escalated;
            decision.reason = format!(
                "policy escalate_on_stale: stale proof could not be renewed: {}",
                decision.reason
            );
        }
    }
    let assurance = uni_decision::assurance_for(&stored);
    let independent =
        uni_decision::independence(&stored) == uni_decision::Independence::Independent;
    let identity = uni_decision::identity_assurance(&stored);
    // Durable drift record: a claim that went stale keeps its reasons until it is
    // proved again, so `uni explain` can narrate the drift on later runs too
    // (the re-run may already have overwritten the proof file).
    let stale_path = du.join("decisions").join("stale.json");
    let mut drift: std::collections::BTreeMap<String, Vec<uni_evidence::StaleReason>> =
        uni_evidence::load_json(&stale_path).unwrap_or_default();
    for id in stale_reasons.keys() {
        drift.insert(id.clone(), stale_reasons[id].clone());
    }
    let verified_now: Vec<String> = decision
        .claims
        .iter()
        .filter(|c| c.state == uni_evidence::EvidenceState::Valid)
        .map(|c| c.claim_id.clone())
        .collect();
    for id in verified_now {
        drift.remove(&id);
    }
    uni_evidence::save_json(&stale_path, &drift)?;

    let last = serde_json::json!({
        "intent": {"id": ir.intent.id, "domain": ir.intent.domain, "goal": ir.intent.goal},
        // The revision the decision was made against, and why any previous
        // proof stopped applying. This is what `uni explain` narrates.
        "commit": cur_sha,
        "stale": drift.iter().map(|(claim, reasons)| (
            claim.clone(),
            serde_json::Value::Array(
                reasons
                    .iter()
                    .map(|r| serde_json::json!({
                        "dimension": r.label(),
                        "detail": r.describe(),
                    }))
                    .collect(),
            ),
        )).collect::<serde_json::Map<String, serde_json::Value>>(),
        "decision": decision.decision,
        "reason": decision.reason,
        "claims": decision.claims,
        "assurance": assurance,
        "independent_actor": independent,
        "identity_assurance": identity,
    });
    journal.push(events::Event {
        name: "DecisionIssued",
        attrs: vec![
            ("uni.intent.id".into(), ir.intent.id.clone()),
            (
                "uni.decision.state".into(),
                format!("{:?}", decision.decision),
            ),
            ("uni.assurance.level".into(), assurance.to_string()),
            ("uni.actor.independent".into(), independent.to_string()),
            ("uni.actor.identity_assurance".into(), identity.to_string()),
        ],
    });
    // 3) Persist phase, serialized: evidence files + journal + last.json are
    // written atomically under an exclusive lock so concurrent verifies can
    // never interleave or truncate each other's state.
    {
        let _lock = uni_evidence::acquire_lock(&du)?;
        for (ev, fingerprint) in &fresh {
            uni_evidence::save_json(
                &uni_evidence::evidence_path(&du, &ev.claim_id, fingerprint),
                ev,
            )?;
        }
        events::append(&journal)?;
        uni_evidence::save_json(&du.join("decisions").join("last.json"), &last)?;
        // Acknowledge the current registry as the new trust-boundary baseline
        // (only after a completed verify: a failed run leaves the flag armed).
        std::fs::write(du.join(".registry.hash"), &registry_hash)?;
        std::fs::write(du.join(".registry.snapshot.toml"), &registry_text)?;
    }
    if as_json {
        println!(
            "{}",
            serde_json::json!({
                "intent": ir.intent.id,
                "decision": decision.decision,
                "reason": decision.reason,
                "claims": decision.claims,
                "evidence": stored,
                "trust_boundary_changed": trust_boundary_changed,
            })
        );
    } else {
        println!("\nUNI Verification — {}", ir.intent.id);
        for c in &decision.claims {
            let mark = match c.state {
                uni_evidence::EvidenceState::Valid => "PASS",
                uni_evidence::EvidenceState::Invalid => "FAIL",
                uni_evidence::EvidenceState::Stale => "STALE",
            };
            let why = stale_reasons
                .get(&c.claim_id)
                .and_then(|r| r.first())
                .map(|r| r.describe())
                .unwrap_or_default();
            if why.is_empty() {
                println!("{:<24} {mark}  {}", c.claim_id, c.detail);
            } else {
                println!("{:<24} {mark}  {}", c.claim_id, why);
            }
        }
        println!("\nDecision: {:?}", decision.decision);
        println!("Reason: {}", decision.reason);
        if identity_verified {
            println!("Identity: {} (verified)", actor.id);
        }
    }
    if !as_json
        && decision.decision != uni_decision::Decision::Accepted
        && decision.decision != uni_decision::Decision::Rejected
    {
        // PRD §16 error UX: propose the exact next command per missing claim.
        let mut hints = vec![];
        for c in &decision.claims {
            if c.state != uni_evidence::EvidenceState::Valid {
                hints.push(format!("uni verify {}", c.claim_id));
            }
        }
        println!("\nRun:\n  {}", hints.join("\n  "));
    }
    match decision.decision {
        uni_decision::Decision::Accepted => Ok(()),
        uni_decision::Decision::Rejected => Err(anyhow!("UNI REJECTED")),
        uni_decision::Decision::Escalated => Err(anyhow!("UNI ESCALATED")),
        _ => Err(anyhow!("UNI EVIDENCE_REQUIRED")),
    }
}
