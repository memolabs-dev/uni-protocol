# UNI threat model — audit baseline (2026-10-13)

Status: **partial audit**, tracked by [issue #4](https://github.com/memolabs-dev/uni-protocol/issues/4).
This records evidence and residual risk, **not a security certification**.

## Security objective and assumptions

UNI decides whether declared claims have sufficiently recent, scoped and trusted evidence.
A contract or generated worktree may be adversarial. The verifier registry
`.uni/config.toml`, the reviewed acceptance manifest and its independently
provisioned `UNI_ACCEPTANCE_SHA256` are owner-controlled **only when the host/CI
actually enforces their permissions and required checks**.
The identity issuer keys are pinned locally. External services are not trusted
as signers by default. A caller-supplied matching hash is not human approval.

## Boundaries / tests / residual risks

| Boundary | Existing enforcement and regression | Residual review |
| --- | --- | --- |
| DSL / parser | Contract is parsed before registry resolution; inline commands require trusted registry equality. | Extend parser fuzzing and malformed-contract negative fixtures. |
| Verifier registry | Named verifier specs come from `.uni/config.toml`; CLI records registry digest and invalidates on drift. | Prove CI permissions and approval workflow for changes to trusted registry; reject unreviewed registries. |
| Watched files and globs | `subject_snapshot` produces sorted per-file SHA-256 hashes, refuses unobservable declared subjects and detects verifier-time drift. | Audit changing glob membership and filesystem observation races; no atomic OS snapshot is claimed. |
| Symlinks / traversal | Watched symlinks are rejected; file-hash expected paths forbid absolute/parent traversal and linked components. | Test nested paths and concurrent link replacement on all supported platforms. |
| Untracked / ignored content | Watched globs inspect the filesystem, not only tracked Git files. | Confirm correct policy for ignored generated files, wildcard prefixes and exclusions. |
| Command origin and authorization | Shell execution requires trusted registry or the documented empty-registry bootstrap; selector bindings require authorization. | Tighten bootstrap and template substitution threat model; no generic shell sanitization is claimed. |
| Evidence cache | Cache matches claim, verifier fingerprint, actor and verification context; registry drift is recorded. **2026-10-13:** fingerprint now binds kind, run, positive/negative expectations, watched files, expected file hashes, timeout, expiry, verifier name and actor with unambiguous JSON encoding. Fingerprint length extended from 48 to 128 bits. | Validate replay/reuse against changed producer, platform, clock and output artifacts with adversarial fixtures. Existing short-key cache entries will safely miss; reverify. |
| Identity and provenance | OIDC token verification uses pinned local trust roots; unattested output must not be described as signed A4. | Audit key lifecycle, token validation, and producer independence. |
| Expiry / platform | `max_age_hours` is part of proof lifetime; repository CI covers Linux/macOS/Windows. | Test wall-clock anomalies and platform-specific path semantics. |
| Owner acceptance | Optional criteria manifest maps owner criteria to required claims and trusted verifiers; external digest pin rejects changed manifest revisions. | Cannot detect owner requirements omitted from the approved manifest; owner authorization remains external. |

## Focused regression and impact

`tests::fingerprint_includes_all_verifier_policy_dimensions` first failed:
changing the verifier kind reused the same legacy cache-key prefix. After
including all policy dimensions and increasing digest length, it passes.
This closes **one** cache-isolation defect, not the full audit in issue #4.
The old on-disk evidence files are not migrated: new fingerprints cause safe
cache misses followed by fresh verification.

## Follow-up criteria for #4

- Red/green regression for each confirmed boundary defect.
- Linux/macOS/Windows CI and full Cargo lint + tests for every patch.
- Host/CI permissions review for manifest and registry ownership.
- Time-of-check/time-of-use limits documented; never claim perfect atomicity.
- Do not enable cryptographic A4 claims until signer, verifier and key lifecycle exist.
