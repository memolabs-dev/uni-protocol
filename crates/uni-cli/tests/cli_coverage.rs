use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
mod support;

fn fixture() -> PathBuf {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "uni-coverage-{}-{time}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(dir.join(".uni")).unwrap();
    std::fs::write(dir.join("artifact.txt"), "published").unwrap();
    let hash = uni_evidence::sha256_hex(b"published");
    std::fs::write(dir.join(".uni/config.toml"), format!(
        "[verifiers.\"owner.valid\"]\ntype = \"file-hash\"\nfiles = [\"artifact.txt\"]\nexpect_sha256 = {{ \"artifact.txt\" = \"{hash}\" }}\n"
    )).unwrap();
    std::fs::write(dir.join("contract.uni"), "VERSION 0.1\nDOMAIN software\nINTENT ledger\nGOAL\n  Deliver an accounted ledger.\nCLAIM regular REQUIRED\n  ENSURE regular credits and debits work\nVERIFY regular\n  USING owner.valid\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n").unwrap();
    dir
}

fn criteria_line(id: &str, claim: &str, ensure: &str, verifier: &str) -> String {
    format!("\n[[criteria]]\nid = \"{id}\"\nclaim = \"{claim}\"\nensure = \"{ensure}\"\nverifier = \"{verifier}\"\n")
}

fn manifest(dir: &Path, more: &str) {
    std::fs::write(
        dir.join("owner.toml"),
        format!(
            "version = 1\nintent = \"ledger\"\n{}{}",
            criteria_line(
                "regular",
                "regular",
                "regular credits and debits work",
                "owner.valid"
            ),
            more
        ),
    )
    .unwrap();
}

fn cli(dir: &Path, args: &[&str]) -> (bool, serde_json::Value) {
    let out = Command::new(support::bin())
        .arg("--json")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let body = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or_else(|_| {
        panic!(
            "CLI did not return JSON: stdout={body}, stderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    });
    (out.status.success(), parsed)
}

#[test]
fn declared_coverage_complete_and_verify_with_opt_in_gate() {
    let dir = fixture();
    manifest(&dir, "");
    let (passed, coverage) = cli(
        &dir,
        &["coverage", "contract.uni", "--manifest", "owner.toml"],
    );
    assert!(passed);
    assert_eq!(coverage["status"], "COMPLETE");
    assert_eq!(coverage["declared"], 1);
    assert_eq!(coverage["mapped"], 1);
    assert_eq!(coverage["authority"], "CALLER_SUPPLIED_NOT_AUTHENTICATED");
    assert_eq!(coverage["scope"], "DECLARED_CRITERIA_ONLY");
    assert_eq!(coverage["manifest_sha256"].as_str().unwrap().len(), 64);
    let (accepted, verify) = cli(
        &dir,
        &["verify", "contract.uni", "--acceptance", "owner.toml"],
    );
    assert!(accepted, "{}", verify);
    assert_eq!(verify["decision"], "Accepted");
    assert_eq!(verify["coverage"]["status"], "COMPLETE");
}

#[test]
fn omitted_owner_invariant_blocks_verification_before_evidence_runs() {
    let dir = fixture();
    let other = criteria_line(
        "overdraft",
        "overdraft",
        "overdraft must be refused",
        "owner.valid",
    );
    manifest(&dir, &other);
    let (passed, coverage) = cli(
        &dir,
        &["coverage", "contract.uni", "--manifest", "owner.toml"],
    );
    assert!(!passed);
    assert_eq!(coverage["status"], "INCOMPLETE");
    assert_eq!(coverage["declared"], 2);
    assert_eq!(coverage["mapped"], 1);
    assert_eq!(coverage["findings"][0]["kind"], "missing_claim");
    let (accepted, verify) = cli(
        &dir,
        &["verify", "contract.uni", "--acceptance", "owner.toml"],
    );
    assert!(!accepted);
    assert_eq!(verify["decision"], "EvidenceRequired");
    assert_eq!(verify["coverage"]["status"], "INCOMPLETE");
    assert!(
        !dir.join(".uni/evidence").exists(),
        "verifiers may not execute before coverage preflight"
    );
    let (legacy, result) = cli(&dir, &["verify", "contract.uni"]);
    assert!(legacy, "{result}");
    assert_eq!(result["decision"], "Accepted");
    assert!(
        result.get("coverage").is_none(),
        "legacy JSON shape must remain unchanged"
    );
}

#[test]
fn duplicate_criterion_id_is_rejected() {
    let dir = fixture();
    manifest(
        &dir,
        &criteria_line(
            "regular",
            "other",
            "regular credits and debits work",
            "owner.valid",
        ),
    );
    let (ok, report) = cli(
        &dir,
        &["coverage", "contract.uni", "--manifest", "owner.toml"],
    );
    assert!(!ok);
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "duplicate_or_empty_id"));
}

#[test]
fn duplicate_claim_mapping_is_rejected() {
    let dir = fixture();
    manifest(
        &dir,
        &criteria_line(
            "another",
            "regular",
            "regular credits and debits work",
            "owner.valid",
        ),
    );
    let (ok, report) = cli(
        &dir,
        &["coverage", "contract.uni", "--manifest", "owner.toml"],
    );
    assert!(!ok);
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "duplicate_or_empty_claim"));
}

#[test]
fn tampered_owner_wording_is_rejected() {
    let dir = fixture();
    manifest(&dir, "");
    let body = std::fs::read_to_string(dir.join("contract.uni"))
        .unwrap()
        .replace("regular credits and debits work", "anything works");
    std::fs::write(dir.join("contract.uni"), body).unwrap();
    let (ok, report) = cli(
        &dir,
        &["coverage", "contract.uni", "--manifest", "owner.toml"],
    );
    assert!(!ok);
    assert_eq!(report["findings"][0]["kind"], "ensure_mismatch");
}

#[test]
fn verifier_mapping_must_be_trusted_and_present() {
    let dir = fixture();
    let content = format!(
        "version = 1\nintent = \"ledger\"\n{}",
        criteria_line(
            "regular",
            "regular",
            "regular credits and debits work",
            "ghost.verifier"
        )
    );
    std::fs::write(dir.join("owner.toml"), content).unwrap();
    let (ok, report) = cli(
        &dir,
        &["coverage", "contract.uni", "--manifest", "owner.toml"],
    );
    assert!(!ok);
    assert_eq!(report["findings"][0]["kind"], "missing_verifier");
    let source = std::fs::read_to_string(dir.join("contract.uni"))
        .unwrap()
        .replace("owner.valid", "ghost.verifier");
    std::fs::write(dir.join("contract.uni"), source).unwrap();
    let (ok, report) = cli(
        &dir,
        &["coverage", "contract.uni", "--manifest", "owner.toml"],
    );
    assert!(!ok);
    assert_eq!(report["findings"][0]["kind"], "untrusted_verifier");
}

#[test]
fn unknown_manifest_keys_and_wrong_intent_are_rejected() {
    let dir = fixture();
    manifest(&dir, "");
    let old = std::fs::read_to_string(dir.join("owner.toml")).unwrap();
    std::fs::write(dir.join("owner.toml"), format!("{old}trusted = true\n")).unwrap();
    let output = Command::new(support::bin())
        .args([
            "--json",
            "coverage",
            "contract.uni",
            "--manifest",
            "owner.toml",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!output.status.success());
    std::fs::write(
        dir.join("owner.toml"),
        old.replace("intent = \"ledger\"", "intent = \"other\""),
    )
    .unwrap();
    let (ok, report) = cli(
        &dir,
        &["coverage", "contract.uni", "--manifest", "owner.toml"],
    );
    assert!(!ok);
    assert_eq!(report["findings"][0]["kind"], "intent_mismatch");
}

#[test]
fn empty_manifest_and_unsupported_version_fail_closed() {
    let dir = fixture();
    std::fs::write(dir.join("owner.toml"), "version = 1\nintent = \"ledger\"\n").unwrap();
    let (ok, report) = cli(
        &dir,
        &["coverage", "contract.uni", "--manifest", "owner.toml"],
    );
    assert!(!ok);
    assert_eq!(report["findings"][0]["kind"], "empty_manifest");
    manifest(&dir, "");
    let old = std::fs::read_to_string(dir.join("owner.toml")).unwrap();
    std::fs::write(
        dir.join("owner.toml"),
        old.replace("version = 1", "version = 2"),
    )
    .unwrap();
    let (ok, report) = cli(
        &dir,
        &["coverage", "contract.uni", "--manifest", "owner.toml"],
    );
    assert!(!ok);
    assert_eq!(report["findings"][0]["kind"], "unsupported_version");
}

#[cfg(not(windows))]
#[test]
fn incomplete_acceptance_blocks_an_executor_before_side_effects() {
    let dir = fixture();
    manifest(
        &dir,
        &criteria_line(
            "overdraft",
            "overdraft",
            "overdraft must be refused",
            "owner.valid",
        ),
    );
    let (accepted, report) = cli(
        &dir,
        &[
            "run",
            "contract.uni",
            "--acceptance",
            "owner.toml",
            "--",
            "touch cannot-have-run.txt",
        ],
    );
    assert!(!accepted);
    assert_eq!(report["coverage"]["status"], "INCOMPLETE");
    assert!(!dir.join("cannot-have-run.txt").exists());
}

fn cli_pinned(dir: &Path, args: &[&str], pin: &str) -> (bool, serde_json::Value) {
    let output = Command::new(support::bin())
        .arg("--json")
        .args(args)
        .env("UNI_ACCEPTANCE_SHA256", pin)
        .current_dir(dir)
        .output()
        .unwrap();
    let record: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "missing coverage JSON: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), record)
}

#[test]
fn externally_pinned_manifest_matches_and_fails_after_mutation() {
    let dir = fixture();
    manifest(&dir, "");
    let pin = uni_evidence::sha256_hex(&std::fs::read(dir.join("owner.toml")).unwrap());
    let args = ["verify", "contract.uni", "--acceptance", "owner.toml"];
    let (valid, accepted) = cli_pinned(&dir, &args, &pin);
    assert!(valid, "{accepted}");
    assert_eq!(accepted["coverage"]["revision_pin"], "MATCHED");
    assert_eq!(accepted["coverage"]["manifest_sha256"], pin);
    // The mapping remains semantically identical, but its owner-pinned
    // revision changed. Do not trust it until the owner renews the pin.
    let mut content = std::fs::read_to_string(dir.join("owner.toml")).unwrap();
    content.push_str("\n# changed by a worker\n");
    std::fs::write(dir.join("owner.toml"), content).unwrap();
    let (valid, blocked) = cli_pinned(&dir, &args, &pin);
    assert!(!valid);
    assert_eq!(blocked["coverage"]["revision_pin"], "MISMATCH");
    assert_eq!(
        blocked["coverage"]["findings"][0]["kind"],
        "manifest_digest_mismatch"
    );
}

#[test]
fn shortened_manifest_cannot_silently_weaken_an_owner_revision() {
    let dir = fixture();
    let extra = criteria_line(
        "overdraft",
        "overdraft",
        "overdraft must be refused",
        "owner.valid",
    );
    manifest(&dir, &extra);
    let pin = uni_evidence::sha256_hex(&std::fs::read(dir.join("owner.toml")).unwrap());

    // Removing an owner criterion yields a valid, fully mapped contract:
    // the independent digest must still expose the weakened revision.
    manifest(&dir, "");
    let (ok, report) = cli_pinned(
        &dir,
        &["coverage", "contract.uni", "--manifest", "owner.toml"],
        &pin,
    );
    assert!(!ok, "{report}");
    assert_eq!(report["declared"], 1);
    assert_eq!(report["mapped"], 1);
    assert_eq!(report["revision_pin"], "MISMATCH");
    assert_eq!(report["findings"][0]["kind"], "manifest_digest_mismatch");
}

#[cfg(not(windows))]
#[test]
fn mismatched_pin_blocks_executor_before_side_effects() {
    let dir = fixture();
    manifest(&dir, "");
    let pin = uni_evidence::sha256_hex(&std::fs::read(dir.join("owner.toml")).unwrap());
    let mut contents = std::fs::read_to_string(dir.join("owner.toml")).unwrap();
    contents.push_str("\n# worker-modified\n");
    std::fs::write(dir.join("owner.toml"), contents).unwrap();

    let (ok, report) = cli_pinned(
        &dir,
        &[
            "run",
            "contract.uni",
            "--acceptance",
            "owner.toml",
            "--",
            "touch cannot-have-run.txt",
        ],
        &pin,
    );
    assert!(!ok, "{report}");
    assert_eq!(report["coverage"]["revision_pin"], "MISMATCH");
    assert_eq!(report["decision"], "EvidenceRequired");
    assert!(
        !dir.join("cannot-have-run.txt").exists(),
        "executor must not run after a pin mismatch"
    );
}

#[test]
fn human_reports_disclose_declared_only_scope_and_manifest_digest() {
    let dir = fixture();
    manifest(&dir, "");
    let pin = uni_evidence::sha256_hex(&std::fs::read(dir.join("owner.toml")).unwrap());

    for args in [
        vec!["coverage", "contract.uni", "--manifest", "owner.toml"],
        vec!["verify", "contract.uni", "--acceptance", "owner.toml"],
    ] {
        let out = Command::new(support::bin())
            .args(&args)
            .env("UNI_ACCEPTANCE_SHA256", &pin)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("UNI acceptance coverage: COMPLETE"), "{text}");
        assert!(text.contains("revision pin MATCHED"), "{text}");
        assert!(text.contains("Scope: DECLARED_CRITERIA_ONLY"), "{text}");
        assert!(
            text.contains("authority: CALLER_SUPPLIED_NOT_AUTHENTICATED"),
            "{text}"
        );
        assert!(text.contains(&format!("Manifest sha256: {pin}")), "{text}");
    }
}

#[test]
fn invalid_revision_pin_never_disables_hash_enforcement() {
    let dir = fixture();
    manifest(&dir, "");
    let args = ["coverage", "contract.uni", "--manifest", "owner.toml"];
    for value in ["", "not-a-sha", "g".repeat(64).as_str()] {
        let (valid, report) = cli_pinned(&dir, &args, value);
        assert!(!valid);
        assert_eq!(report["revision_pin"], "INVALID");
        assert_eq!(report["findings"][0]["kind"], "invalid_revision_pin");
    }
}
