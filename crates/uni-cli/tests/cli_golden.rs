use std::path::{Path, PathBuf};
use std::process::Command;
mod support;
use support::bin;

fn git(cwd: &PathBuf, args: &[&str]) {
    assert!(Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap()
        .success());
}

fn run(cmd: &[&str], cwd: &PathBuf) -> i32 {
    Command::new(support::bin())
        .args(cmd)
        .current_dir(cwd)
        .status()
        .unwrap()
        .code()
        .unwrap()
}

fn repo_root() -> PathBuf {
    support::workspace_root()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let src = e.path();
        let dst = to.join(e.file_name());
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

/// Per-test workspace: examples plus the real registry, in a fresh git repo.
/// CI runs these tests in parallel and several of them mutate `.uni/` state
/// (evidence, decision, journal); sharing the repository root made them race.
/// Every test that touches `.uni/` gets its own copy instead.
fn repo_fixture(tag: &str) -> PathBuf {
    repo_fixture_with(tag, false)
}

/// `real_registry` keeps the repository's registry (needed by the stack test,
/// which runs python and node). Otherwise the fixture gets a minimal portable
/// registry: the examples it exercises would otherwise shell out to `cargo
/// check`, which cannot run outside a Rust workspace.
fn repo_fixture_with(tag: &str, real_registry: bool) -> PathBuf {
    let root = repo_root();
    let dir = std::env::temp_dir().join(format!(
        "uni-golden-{}-{}",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();
    if real_registry {
        std::fs::copy(root.join(".uni/config.toml"), dir.join(".uni/config.toml")).unwrap();
    } else {
        std::fs::write(dir.join(".uni/config.toml"),
            "[verifiers]\n\"project.check\" = \"true\"\n\"test.true\" = \"true\"\n\"test.false\" = \"false\"\n\"test.fail\" = \"false\"\n").unwrap();
    }
    std::fs::write(
        dir.join(".gitignore"),
        "/target\n.uni/evidence/\n.uni/decisions/\n.uni/events.jsonl\n",
    )
    .unwrap();
    copy_dir(&root.join("examples"), &dir.join("examples"));
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "-A"]);
    git(
        &dir,
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    );
    dir
}

/// Golden: compile output is stable (no timestamps).
#[test]
fn golden_compile() {
    let root = repo_fixture("compile");
    let contract = root.join("examples/hello/hello.uni");
    let o = Command::new(bin())
        .args(["compile", contract.to_str().unwrap()])
        .current_dir(&root)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert_eq!(
        stdout,
        "intent: hello\nclaims: 1\nverifications: 1\n--json for canonical IR\n"
    );
}

/// Golden: compile --json shape is stable (keys + counts, volatile values masked).
#[test]
fn golden_compile_json_shape() {
    let root = repo_fixture("compile-json");
    let contract = root.join("examples/hello/hello.uni");
    let o = Command::new(bin())
        .args(["--json", "compile", contract.to_str().unwrap()])
        .current_dir(&root)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["uni_version"], "0.1");
    assert_eq!(v["intent"]["id"], "hello");
    assert_eq!(v["claims"].as_array().unwrap().len(), 1);
    assert_eq!(v["verification"][0]["claim_id"], "binary-builds");
}

/// Diff-of-setup golden: verify exit codes follow the decision (0 accepted, non-zero otherwise).
#[test]
fn golden_verify_exit_codes() {
    let root = repo_fixture("exit-codes");
    assert_eq!(run(&["verify", "examples/hello/hello.uni"], &root), 0);
    assert_eq!(run(&["verify", "examples/multi/multi.uni"], &root), 0);
    assert_eq!(run(&["verify", "examples/booking/booking.uni"], &root), 0);
}

/// Golden: speckit importer extracts markdown FR + scenarios into a candidate DSL file.
#[test]
fn golden_import_speckit() {
    let dir = std::env::temp_dir().join(format!(
        "sk-import-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/contracts")).unwrap();
    std::fs::write(dir.join("constitution.md"), "# Constitution\n").unwrap();
    std::fs::write(dir.join("plan.md"), "# Plan\n").unwrap();
    std::fs::write(
        dir.join("spec.md"),
        "- **FR-001**: alpha works
- **FR-002**: beta rejects

#### Scenario: gamma path
- [ ] checkbox claim one
",
    )
    .unwrap();
    let o = Command::new(bin())
        .args(["import-speckit", dir.to_str().unwrap()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let contracts_dir = dir.join(".uni/contracts");
    let mut dsl_path = None;
    for e in std::fs::read_dir(&contracts_dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().map(|e| e == "uni").unwrap_or(false) {
            dsl_path = Some(p.clone());
        }
    }
    let dsl_path = dsl_path.expect("candidate DSL written");
    let dsl = std::fs::read_to_string(&dsl_path).unwrap();
    assert!(dsl.contains("CLAIM fr-001"), "{dsl}");
    assert!(dsl.contains("CLAIM fr-002"), "{dsl}");
    assert!(dsl.contains("CLAIM scenario-03"), "{dsl}");
    assert!(dsl.contains("CLAIM check-04"), "{dsl}");
    // the candidate must parse back with the same parser
    let dsl_path: std::path::PathBuf = std::fs::read_dir(&contracts_dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|ee| ee.path()))
        .find(|p| p.extension().map(|e| e == "uni").unwrap_or(false))
        .expect("candidate DSL written");
    let out = Command::new(bin())
        .args(["compile", dsl_path.to_str().unwrap()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("claims: 4"));
}

/// Golden: `uni report` is byte-stable across repeated verify runs (PR/CI view).
#[test]
fn golden_report_stability() {
    let root = repo_fixture("report");
    assert_eq!(
        run(&["verify", "examples/booking/booking.uni"], &root),
        0,
        "fixture must verify first"
    );
    let o1 = Command::new(bin())
        .args(["report", "--json"])
        .current_dir(&root)
        .output()
        .unwrap();
    let o2 = Command::new(bin())
        .args(["report", "--json"])
        .current_dir(&root)
        .output()
        .unwrap();
    // two calls on the same last.json are identical
    assert_eq!(o1.stdout, o2.stdout, "report must be deterministic");
    let v: serde_json::Value = serde_json::from_slice(&o1.stdout).unwrap();
    assert!(v["summary"]["claims_total"].is_u64());
    assert!(v["claims"].is_array());
    assert!(
        v["claims"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c.as_object().unwrap().len() == 2
                && c["claim_id"].is_string()
                && c["state"].is_string()),
        "only stable fields allowed: {v}"
    );
    // reason and decision survive
    assert!(v["reason"].is_string());
}

/// v0.8: append-only event journal, second verify appends cache-hit events.
#[test]
fn golden_events_journal() {
    let root = repo_fixture("events");
    assert_eq!(run(&["verify", "examples/hello/hello.uni"], &root), 0);
    // A second run is what produces cache hits; a single run only ever records
    // EvidenceRun. (Before isolation this test silently relied on evidence
    // left behind by other tests in the repository root.)
    assert_eq!(run(&["verify", "examples/hello/hello.uni"], &root), 0);
    let j = Command::new(bin())
        .args(["events", "--json"])
        .current_dir(&root)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&j.stdout);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(
        lines.len() >= 3,
        "at least IntentVerified + Evidence + DecisionIssued, got {lines:?}"
    );
    let last: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert_eq!(last["event"], "DecisionIssued");
    assert!(last["attributes"]["uni.intent.id"].is_string());
    assert!(last["attributes"]["uni.assurance.level"].is_string());
    // for a repeat run, cache hits appear as EvidenceReused events
    let texts: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    assert!(texts.iter().any(|l| l.contains("\"EvidenceReused\"")));
}

/// v0.11: lint catches claims without VERIFY (preflight, no execution).
#[test]
fn golden_lint_missing_verify() {
    let dir = std::env::temp_dir().join(format!(
        "uni-lint-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1
DOMAIN software
INTENT lint-fail
GOAL
  n
CLAIM ghost REQUIRED
  ENSURE n
",
    )
    .unwrap();
    let o = Command::new(bin())
        .args(["lint", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_ne!(o.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.contains("missing-verify") && stdout.contains("ghost"),
        "{stdout}"
    );
    // clean contract lints green
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1
DOMAIN software
INTENT lint-ok
GOAL
  n
CLAIM x REQUIRED
  ENSURE n
VERIFY x
  USING anything
",
    )
    .unwrap();
    let o2 = Command::new(bin())
        .args(["lint", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(o2.status.code(), Some(0), "{o2:?}");
}

#[test]
fn lint_warns_when_verifier_glob_matches_no_sources() {
    let dir = std::env::temp_dir().join(format!(
        "uni-lint-empty-watcher-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".uni")).unwrap();
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers.watch]\nrun = \"true\"\nfiles = [\"tests/*.py\"]\n",
    )
    .unwrap();
    std::fs::write(dir.join("contract.uni"),
        "VERSION 0.1\nDOMAIN software\nINTENT lint-glob\nGOAL\n  Check source.\nCLAIM a REQUIRED\n  ENSURE test passes\nVERIFY a\n  USING watch\n",
    ).unwrap();
    let first = Command::new(bin())
        .args(["lint", "contract.uni", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(first.status.success(), "{first:?}");
    let finding: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert!(
        finding["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["kind"] == "empty-watched-subject"),
        "{finding}"
    );
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(dir.join("tests/a.py"), "pass").unwrap();
    let second = Command::new(bin())
        .args(["lint", "contract.uni", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let clean: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert!(
        !clean["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["kind"] == "empty-watched-subject"),
        "{clean}"
    );
}

/// ADR-002: lint names a pinned test name as a contract smell (warning only,
/// the decision is untouched), and stays silent on suites and templates.
#[test]
fn golden_lint_pinned_test_selector() {
    let dir = std::env::temp_dir().join(format!(
        "uni-lint-pin-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();
    std::fs::write(dir.join(".uni/config.toml"), "[verifiers]\n\"pinned\" = \"cargo test add_works -- --exact\"\n\"suite\" = \"cargo test\"\n\"tpl\" = \"cargo test {{selector}} -- --exact\"\n").unwrap();
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1
DOMAIN software
INTENT lint-pin
GOAL
  n
CLAIM a REQUIRED
  ENSURE n
VERIFY a
  USING pinned
CLAIM b REQUIRED
  ENSURE n
VERIFY b
  USING suite
CLAIM c REQUIRED
  ENSURE n
VERIFY c
  USING tpl
",
    )
    .unwrap();
    let o = Command::new(bin())
        .args(["lint", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.contains("pinned-test-selector") && stdout.contains("claim 'a'"),
        "{stdout}"
    );
    assert_eq!(
        stdout.matches("pinned-test-selector").count(),
        1,
        "{stdout}"
    );
}

/// v0.12: OPA adapter — when a rego bundle + a shim `opa` binary exist,
/// (the shim is a shell script, so this test is unix-only)
/// the provider resolves from the bundle; absent opa falls back to TOML.
#[cfg(unix)]
#[test]
fn golden_policy_provider_opa_fallback() {
    // isolated workspace: no opa binary intercept needed — create one that works.
    use std::time::{SystemTime, UNIX_EPOCH};
    let dir = std::env::temp_dir().join(format!(
        "uni-ppa-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let shim_dir = dir.join("shim");
    std::fs::create_dir_all(&shim_dir).unwrap();
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();
    std::fs::create_dir_all(dir.join("policies")).unwrap();
    std::fs::write(dir.join("policies/opa.rego"),
"package uni\nrules = {\"escalate_on_stale\": true, \"reject_on_invalid\": true, \"min_verified_ratio\": 0.0}\n").unwrap();
    let path_env = format!("{}/bin:/bin:/usr/bin", shim_dir.display());
    let shim = shim_dir.join("opa");
    std::fs::write(
        &shim,
        r#"#!/bin/sh
if [ "$1" = "version" ]; then exit 0; fi
echo '[{"escalate_on_stale": true, "reject_on_invalid": true, "min_verified_ratio": 0.5}]'
"#,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    // PATH is passed to the CHILD only: mutating process env races with other tests.
    let child_path = format!("{path_env}:{}", std::env::var("PATH").unwrap_or_default());

    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers]\n\"p\" = \"true\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1
DOMAIN software
INTENT ppa
GOAL
  g
CLAIM x REQUIRED
  ENSURE g
VERIFY x
  USING p
",
    )
    .unwrap();
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "."]);
    git(
        &dir,
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "a",
        ],
    );
    let o1 = Command::new(bin())
        .args(["verify", "c.uni"])
        .current_dir(&dir)
        .env("PATH", &child_path)
        .output()
        .unwrap();
    let r1 = String::from_utf8_lossy(&o1.stdout).to_string();
    assert!(o1.status.success() && r1.contains("Accepted"), "{r1}");
    // and with opa absent the fallback to the TOML stack still accepts
    let o2 = Command::new(bin())
        .args(["verify", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        o2.status.success(),
        "opa-absent fallback must not block: {}",
        String::from_utf8_lossy(&o2.stderr)
    );
}

/// v0.13: doctor — healthy exit 0 on a prepared workspace, failure exit without .uni.
#[test]
fn golden_doctor_healthy_and_fail() {
    let dup = repo_root();
    let ok = bin_state(&dup);
    assert!(ok, "uni repo doctor must be healthy");
    let dir = std::env::temp_dir().join(format!(
        "uni-doc-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(bin())
        .args(["doctor"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0), "doctor must fail without .uni");
}
fn bin_state(root: &std::path::PathBuf) -> bool {
    Command::new(bin())
        .args(["doctor"])
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// v0.14: stack independence — python + nodejs examples verify green.
/// Skips silently when the runtime is missing (CI ubuntu has both).
#[test]
fn golden_stack_independence() {
    let root = repo_fixture_with("stacks", true);
    for (contract, runtime) in [
        ("examples/python/contract.uni", "python3"),
        ("examples/nodejs/contract.uni", "node"),
    ] {
        let has = Command::new("which")
            .arg(runtime)
            .stdout(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !has {
            continue;
        }
        let o = Command::new(bin())
            .args(["verify", contract])
            .current_dir(&root)
            .output()
            .unwrap();
        assert_eq!(
            o.status.code(),
            Some(0),
            "{contract} must ACCEPT with {runtime} available\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
    }
}

/// v0.16: software pack ships, lists, and materializes lint-clean contracts.
#[test]
fn golden_software_pack() {
    let root = repo_root();
    let o = Command::new(bin())
        .args(["pack", "list", "--json"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(o.status.success());
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let packs = v["packs"].as_array().unwrap();
    assert!(packs.iter().any(|p| p["name"] == "software"), "{v}");
    let sw = packs.iter().find(|p| p["name"] == "software").unwrap();
    assert_eq!(sw["templates"].as_array().unwrap().len(), 3);

    // materialize + lint in a sandbox (repo .uni registry has the referenced keys)
    let dir = std::env::temp_dir().join(format!(
        "uni-pack-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();
    std::fs::create_dir_all(dir.join("packs/software/templates")).unwrap();
    for e in std::fs::read_dir(root.join("packs/software"))
        .unwrap()
        .flatten()
    {
        if e.path().extension().map(|x| x == "toml").unwrap_or(false) {
            std::fs::copy(e.path(), dir.join("packs/software/pack.toml")).unwrap();
        }
    }
    for e in std::fs::read_dir(root.join("packs/software/templates"))
        .unwrap()
        .flatten()
    {
        std::fs::copy(
            e.path(),
            dir.join("packs/software/templates").join(e.file_name()),
        )
        .unwrap();
    }
    assert_eq!(
        run(&["pack", "template", "software", "tests-pass"], &dir),
        0
    );
    let lint = Command::new(bin())
        .args(["lint", "uni/intents/tests-pass.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        lint.status.success(),
        "{}",
        String::from_utf8_lossy(&lint.stderr)
    );
    assert!(String::from_utf8_lossy(&lint.stdout).contains("clean"));
}

/// v0.3.3: markdown shapes that used to lose requirements: numbered lists,
/// bullets with bold, and tasks.md checkboxes (already checked or not).
#[test]
fn golden_import_speckit_markdown_shapes() {
    let dir = std::env::temp_dir().join(format!(
        "sk-shapes-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/contracts")).unwrap();
    std::fs::write(
        dir.join("spec.md"),
        "# Spec\n1. **FR-010**: numbered list requirement\n- **FR-011**: bullet bold requirement\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("tasks.md"),
        "## Tasks\n- [ ] unchecked task\n- [x] already checked task\n",
    )
    .unwrap();
    std::fs::write(dir.join("constitution.md"), "# C\n").unwrap();
    std::fs::write(dir.join("plan.md"), "# P\n").unwrap();
    let o = Command::new(bin())
        .args(["import-speckit", dir.to_str().unwrap()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let mut dsl = String::new();
    for e in std::fs::read_dir(dir.join(".uni/contracts")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().map(|x| x == "uni").unwrap_or(false) {
            dsl = std::fs::read_to_string(&p).unwrap();
        }
    }
    assert!(dsl.contains("CLAIM fr-010"), "{dsl}");
    assert!(dsl.contains("CLAIM fr-011"), "{dsl}");
    assert!(dsl.contains("CLAIM check-03"), "{dsl}");
    assert!(dsl.contains("CLAIM check-04"), "{dsl}");
}

/// v0.5: the work order names the exact test a claim's verifier selects, so an
/// agent cannot be penalised for a naming guess. Guidance only, deterministic.
#[test]
fn golden_brief_names_required_test() {
    let dir = std::env::temp_dir().join(format!(
        "uni-brief-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();
    std::fs::write(dir.join(".uni/config.toml"),
        "[verifiers]\n\"mini.upper\" = {\"run\" = \"cargo test clamp_upper_works -- --exact\", \"expect\" = \"test result: ok. 1 passed\"}\n").unwrap();
    std::fs::write(dir.join("c.uni"), "VERSION 0.1\nDOMAIN software\nINTENT brief-demo\nGOAL\n g\nCLAIM upper REQUIRED\n  ENSURE clamps above\nVERIFY upper\n  USING mini.upper\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n").unwrap();

    let o = Command::new(bin())
        .args(["brief", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let md = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(md.contains("clamp_upper_works"), "{md}");
    assert!(md.contains("test result: ok. 1 passed"), "{md}");
    assert!(!md.contains("Warning"), "{md}");

    // Byte-stable: same inputs, same bytes (it can be committed and diffed).
    let o2 = Command::new(bin())
        .args(["brief", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(o.stdout, o2.stdout, "brief must be deterministic");

    // --out writes atomically and says so on stderr.
    let o3 = Command::new(bin())
        .args(["brief", "c.uni", "--out", "brief.md"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(o3.status.code(), Some(0));
    let written = std::fs::read_to_string(dir.join("brief.md")).unwrap();
    assert_eq!(written.trim_end(), md.trim_end());
}

/// v0.5: an unresolvable verifier is reported in the brief instead of silenced.
#[test]
fn golden_brief_reports_unresolvable_verifier() {
    let dir = std::env::temp_dir().join(format!(
        "uni-brief-bad-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers]\n\"known\" = \"true\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("c.uni"), "VERSION 0.1\nDOMAIN software\nINTENT brief-bad\nGOAL\n g\nCLAIM x REQUIRED\n  ENSURE g\nCLAIM y REQUIRED\n  ENSURE h\nVERIFY x\n  USING known\nVERIFY y\n  USING ghost\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n").unwrap();
    let o = Command::new(bin())
        .args(["brief", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0));
    let md = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(md.contains("NOT RESOLVABLE"), "{md}");
    assert!(md.contains("ghost"), "{md}");
}

/// v0.8: importing a Spec Kit feature emits the work order next to the
/// candidate contract, so the evidence requirement reaches the worker without
/// the registry->test-name hop that caused the study's false rejections.
#[test]
fn golden_speckit_import_emits_brief() {
    let dir = std::env::temp_dir().join(format!(
        "sk-brief-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/contracts")).unwrap();
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers]\n\"project.tests\" = \"cargo test\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("spec.md"), "- **FR-001**: alpha works\n").unwrap();
    std::fs::write(dir.join("constitution.md"), "# C\n").unwrap();
    std::fs::write(dir.join("plan.md"), "# P\n").unwrap();
    std::fs::write(dir.join("tasks.md"), "# T\n").unwrap();
    let o = Command::new(bin())
        .args(["--json", "import-speckit", dir.to_str().unwrap()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(
        v["candidate_brief"]
            .as_str()
            .unwrap()
            .ends_with(".brief.md"),
        "{v}"
    );
    assert_eq!(v["candidate_claims"].as_u64(), Some(1));
    let brief_abs = dir.join(
        v["candidate_brief"]
            .as_str()
            .unwrap()
            .trim_start_matches("./"),
    );
    assert!(
        brief_abs.exists(),
        "brief not written at {}",
        brief_abs.display()
    );
    let text = std::fs::read_to_string(&brief_abs).unwrap();
    assert!(text.contains("Work order"), "{text}");
    assert!(text.contains("fr-001"), "{text}");
}

/// v0.5: a selector-template verifier is described in the work order as
/// "you name the test, a human authorizes it", never as a raw token.
#[test]
fn golden_brief_explains_selector_templates() {
    let dir = std::env::temp_dir().join(format!(
        "uni-brief-tpl-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();
    std::fs::write(dir.join(".uni/config.toml"),
        "[verifiers]\n\"suite\" = {\"run\" = \"cargo test {{selector}} -- --exact\", \"expect\" = \"test result: ok. 1 passed\"}\n").unwrap();
    std::fs::write(dir.join("c.uni"), "VERSION 0.1\nDOMAIN software\nINTENT tpl\nGOAL\n g\nCLAIM x REQUIRED\n  ENSURE g\nVERIFY x\n  USING suite\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n").unwrap();
    let o = Command::new(bin())
        .args(["brief", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let md = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(
        md.contains("uni bind --claim x --verifier suite --selector"),
        "{md}"
    );
    assert!(
        md.contains("cargo test <your-test-name> -- --exact"),
        "{md}"
    );
    assert!(
        !md.contains("{{selector}}"),
        "the raw token must not leak into the work order: {md}"
    );

    let j = Command::new(bin())
        .args(["--json", "brief", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&j.stdout).unwrap();
    assert_eq!(v["claims"][0]["selector_template"], true, "{v}");
    // The machine-readable work order must not leak the token either.
    let cmd = v["claims"][0]["evidence"]["command"].as_str().unwrap_or("");
    assert!(!cmd.contains("{{selector}}"), "{cmd}");
    assert!(cmd.contains("<your-test-name>"), "{cmd}");
}
