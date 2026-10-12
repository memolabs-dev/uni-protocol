# Evidence

Evidence is a machine-checkable record that a verifier ran and what it produced.
It lives in `.uni/evidence/<claim-id>.json` (canonical JSON, see `Evidence` in
`crates/uni-evidence`).

## Shape

| Field | Meaning |
|---|---|
| id | claim + hash of the verifier command |
| claim_id | the claim this proves |
| producer | which verifier produced it (`shell-verifier` v0.1) |
| command | the trusted command that ran |
| exit_code | process exit status |
| output_hash / output_excerpt | sha256 of combined stdout+stderr, plus first 2000 chars |
| commit_sha / workspace_dirty | git binding at run time |
| artifact_hash | sha256 over watched files (see `files` in verification.md) |
| state | `Valid` / `Invalid` / `Stale` |
| created_at / duration_ms | wall clock and runtime (volatile, excluded from `uni report`) |

## States

- **Valid**: command exited 0, `expect` matched (if any), `expect_not` absent (if any).
- **Invalid**: non-zero exit or expectation mismatch.
- **Stale**: was valid once, no longer bound to current state.

## Invalidation (the reason this exists)

Evidence is only trusted while its bindings hold. Validity is a product of
seven dimensions (Verification Context, v0.2):

| Dimension | Recorded as | Drift means |
|---|---|---|
| Contract | `contract_hash` (sha256 of the `.uni` source) | re-run |
| Subject | `artifact_hash` (sha256 over watched `files`) | re-run |
| Verifier | `fingerprint` (ref + run + expects + files) | re-run (different verification) |
| Verifier configuration | `registry_hash` (sha256 of `config.toml`) | re-run |
| Environment | `commit_sha` + dirty flag, `platform` (os-arch) | re-run |
| Policy | `policy_hash` (recorded for audit) | decision recompute, no re-run |
| Time | `created_at`, `expires_at` (from the verifier's `max_age_hours`) | expired evidence is stale, so the verifier re-runs |

1. **Git binding** (`is_stale`): if HEAD moved or dirty-state changed since the
   run, the stored evidence is Stale and `uni verify` re-runs the verifier.
   Dirtiness means "tracked content changed". It excludes `.uni/` runtime
   files (a verify must not invalidate its own fresh evidence, and concurrent
   verifies must stay consistent) and untracked files (build output such as
   `target/` is not code, and a verifier that compiles would otherwise stale
   its own proof). Trust-relevant `.uni` changes are covered by registry and
   policy hashes; new source files are covered by the verifier re-running, and
   by `files` globs where a claim declares its subject. Residual risk, stated
   plainly: an untracked file that no verifier observes does not by itself
   invalidate evidence. Declare the subject with `files` when that matters.
2. **Content binding** (`artifact_hash`): if a verifier declares `files` globs,
   the evidence stores a deterministic sha256 over an ordered list of path and
   content hashes. Both the digest and per-file map come from one filesystem
   enumeration. A wildcard like `src/*.rs` matches a filename segment, and
   `src/**` matches files recursively. Empty or unreadable watched subjects,
   and symlinks under the static prefix of a watched glob, fail closed; UNI
   does not follow symlinked subjects outside the workspace. As before,
   build caches, Git metadata, and UNI runtime output are not watched.
   Changes to a watched subject invalidate previously accepted evidence.
   This does not make claims about untracked content outside the declared scope.
3. **Context binding** (`contract_hash`, `registry_hash`, `platform`): editing
   the contract, touching any registry entry, or moving across platforms
   invalidates. Legacy v0.1.0 files (empty hashes) still load.
4. **Policy binding** (`policy_hash`): recorded for audit; a policy change
   recomputes the decision from stored evidence without re-running verifiers.
5. **Stale-but-unreprovable**: a claim whose previous proof drifted AND whose
   re-run cannot renew it escalates under `escalate_on_stale` (human look),
   instead of merely reporting missing evidence.
6. **Time**: a verifier may declare `max_age_hours`. The expiry is stamped on
   the evidence at run time and travels with it, so nothing in the registry can
   silently extend a proof that already decayed. Use it for claims whose truth
   decays (a security scan, a dependency audit, an availability probe) and not
   for deterministic test results, which do not expire. This is the one
   time-dependent dimension of validity by design: the decision for a given
   `(contract, evidence, policy)` is still deterministic, while *whether* a
   proof is still valid is asked at read time. An expired proof is stale, not
   missing: the verifier runs again and a fresh window is stamped.

A decision is never silently reused across a binding break: old evidence cannot
mask a new commit (e2e test: `stale_evidence_does_not_mask_new_commit`).

## Audit trail

`uni verify` appends IntentVerified, EvidenceRun, EvidenceReused,
EvidenceStale, RegistryChanged, IdentityVerified, IdentityUnverified and
DecisionIssued to `.uni/events.jsonl`, with `uni.*` attributes. `uni bind`
appends BindingAuthorized. See `uni events`.

The journal is append-only but not unbounded: once it passes 1 MiB it is
archived as `events.<timestamp>.jsonl` and a fresh one starts, keeping the three
newest archives. `uni events` reads the current window, `uni events --all` reads
the archives too, and `uni doctor` reports the size and the rotation cap. The
live journal is never pruned as if it were an archive.

`uni events --otlp` emits the journal as an OTLP/JSON document (one span per
event, resource `service.name=uni`, the `uni.*` attributes as span attributes),
so an existing collector can ingest UNI without a UNI-specific integration.
Trace and span ids are derived from the event content, so the export is
deterministic for a given journal rather than random: two exports of the same
journal are byte-identical.

## Bundles (v0.3)

```bash
uni bundle export contract.uni --out bundle.jsonl
uni bundle verify bundle.jsonl    # offline, read-only
```

A bundle is JSONL: a header (version, intent, registry_hash, contract_hash,
tool) then one record per artifact (contract, registry, policy, evidence,
binding, decision, events). Every record carries `sha256` over its canonical
body, so transport tampering is detectable offline. Cross-checks reject
bundles whose pieces do not belong together: evidence gathered under another
registry or contract, a decision citing claims with no bundled evidence, a
REQUIRE without its binding, evidence for claims absent from the contract.

Importing a bundle never injects proofs into a live cache: verification only
reports. Transport is not authority.
