# Writing verifiers

v0.1 ships one executor (shell) plus the trusted registry. Two extension paths:

## 1. Register a command (no code, recommended default)

Any tool with a meaningful exit code or output already works:

```toml
[verifiers."npm.test-auth"]
run = "npm test -- --run auth"
expect = "Tests  12 passed"
files = ["src/auth/**"]
timeout = 300

[verifiers."no-secrets"]
run = "! grep -RIn 'AKIA[0-9A-Z]\\{16\\}' ."
expect_not = "AKIA"
```

### Content-bound watcher scope

The `files` patterns use slash-separated paths relative to the workspace.
`*` matches within **one** path component (so `tests/*.py` works), and
`**` matches zero or more components (so `tests/**` is recursive).
By default UNI watches every matching regular file, including gitignored
backups and caches. It fails closed for unsafe symlinks and when the watched
subject is empty; `uni lint` warns about zero-match patterns without running
the verifier.

For **repository source files**, the trusted registry can opt into:

```toml
[verifiers."python.tests"]
run = "python -m unittest discover"
files = ["tests/**/*.py", "src/**/*.py"]
tracked_only = true
```

This hashes only Git-indexed paths, including modified/staged worktree files.
Gitignored and untracked files do not affect evidence. New source files must be
staged with `git add` to enter the watched scope. This is **not** proof about
untracked files or runtime-generated outputs, and should not be selected for
verifiers whose behavior depends on them. Without a Git repository rooted at the
workspace, missing tracked files or unsafe tracked symlinks, the watched subject
is invalid (fail closed). Changing `tracked_only` invalidates old cached proof.
The option must be a TOML boolean; invalid values make that verifier unavailable.

Design the expectation on the tool's real stdout (capture a run first; the
reporter format is part of the contract, see the Node `ℹ fail 0` gotcha in
`verification.md`).

## 2. Built-in adapters (v0.3)

`type` selects the adapter; `shell` is the default.

| Type | Behavior | Extra registry fields |
|---|---|---|
| `shell` | runs the command via `sh -c` (POSIX) or `cmd /C` (Windows), native timeout | `run`, `expect`, `expect_not` |
| `file-hash` | runs nothing; each expected path must exist and match its sha256 | `files`, `expect_sha256` |

```toml
[verifiers."artifact.digest"]
type = "file-hash"
files = ["dist/app.js"]
expect_sha256 = "057095fd5ef5..."        # single-file shorthand
# or per-path:
# expect_sha256 = { "dist/app.js" = "...", "dist/app.css" = "..." }
```

Any other `type` is a hard error (a registry typo never silently falls back to
running a command), and inline `shell` overrides are refused for non-shell
types. See `examples/artifact/` for a working demo.

## 3. Write a code adapter

The seam is implemented in `crates/uni-verify` (`pub trait Verifier`):

```rust
pub trait Verifier {
    fn name(&self) -> &'static str;
    fn run(&self, spec: &VerifierSpec, workspace: &Path) -> Result<VerifierOutcome>;
}

pub struct VerifierOutcome {
    pub state: EvidenceState,   // Valid | Invalid
    pub exit_code: i32,
    pub command: String,
    pub output: String,
}
```

An adapter must:

1. Return raw observations only: `run_spec` wraps them into `Evidence` with
   every binding (`commit_sha`, `workspace_dirty`, `artifact_hash`, actor,
   context hashes). Never build `Evidence` by hand.
2. Decide `Valid`/`Invalid` from deterministic observations; no network, no
   clock, no RNG in the pass/fail decision.
3. Never read the contract: only the authorized spec and the workspace.
4. Declare `type` in the registry and refuse inline overrides (the loader and
   `verifier_for` already enforce this for built-ins).

Candidates for the next adapters (ranked by demand): git diff range,
Semgrep/CodeQL findings, Playwright report parser, Lighthouse budget, coverage
ratio with claim-specific test selection.

## Test the adapter at its seam

Given a fixture workspace + a fixed contract, assert the produced
`Evidence.state` and the final `Decision`. External behavior only, never
internals (same rule the core follows: test `assure_contract` and `evaluate`,
not the parser or the store).
`crates/uni-verify` unit tests are the model: `file_hash_adapter_matches_and_mismatches`.

## File-hash path safety

The built-in `file-hash` adapter only reads regular workspace-relative files.
It refuses absolute paths, parent-directory traversal and symlink components,
even if the content hash would match. Every path in `expect_sha256` is included
in the observed subject and therefore invalidates cached proof on drift, even
if `files` was omitted in the trusted registry. On an unsafe, unreadable or
missing file the verification cannot be accepted; missing-file diagnostics
retain the `MISSING` label.

This is a conservative trust boundary. If a project uses symlinked artifacts,
materialize the artifact in the workspace and verify that regular file.
