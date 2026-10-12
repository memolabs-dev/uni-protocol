# UNI docs (Developer Preview v0.9)

| Doc | Contents |
|---|---|
| [why-uni.md](why-uni.md) | the category: Outcome Assurance, where UNI sits, what it is not |
| [quickstart.md](quickstart.md) | 60-second loop: init, lint, verify, report |
| [language.md](language.md) | the closed `.uni` vocabulary v0.1 |
| [claims.md](claims.md) | CLAIM / INVARIANT / FORBID / REQUIRE semantics |
| [acceptance.md](acceptance.md) | owner-reviewed acceptance coverage gate (optional) |
| [evidence.md](evidence.md) | evidence shape, states, git + content invalidation |
| [verification.md](verification.md) | trusted registry, expect matchers, the trust boundary |
| [decisions.md](decisions.md) | truth table, policy layer, exit-code semantics, the assurance scale and how A3 is earned |
| [github-integration.md](github-integration.md) | PR check, step summary, stable report |
| [writing-verifiers.md](writing-verifiers.md) | registering commands vs writing adapters |
| [brief.md](brief.md) | the work order handed to an implementing agent |
| [run.md](run.md) | the execute half: run your command, then verify |
| [specification.md](specification.md) | grammar, canonical IR, behavior contract |
| [current-status.md](current-status.md) | current release, CI evidence, measured results, known limits |
| [REPORT-2026-09-14.md](REPORT-2026-09-14.md) | historical September 2026 snapshot; not current status |
| [DEEP-ANALYSIS-2026-09-14.md](DEEP-ANALYSIS-2026-09-14.md) | earlier audit at v0.18, superseded; kept as the record of how two real bugs were found |

The behavior guides describe the binary built from the current source
(`cargo build --release`, `./target/release/uni`). Dated reports are historical
records of the revision named in each file. When in doubt, run the current
binary: what executes beats what a doc describes. Constitution:
`../constitution.md`. Spec source: `../specs/001-uni-software-v01/spec.md`.

## Stale evidence

- [The check, in the pull request](flagship-check.md)
- [Why a proof stopped applying](decisions.md#why-a-proof-stopped-applying)
- [Stale Evidence Benchmark, 2026-09-14](../experiments/stale-bench/RESULTS-2026-09-14.md)

## Verified identity (A3)

- [The assurance scale, and how a token earns A3](decisions.md)
- [End-to-end proof, run against the built binary](../crates/uni-cli/tests/cli_identity.rs)
- [Registry keys for `[identities]`](verification.md)
- [**Live, against a real issuer**: GitHub Actions OIDC, run unattended](../examples/identity-github-actions/README.md)
  (`.github/workflows/identity-live.yml`; the scheduled workflow is the current proof)
- [Worked example: Google, needs a human-minted token](../examples/identity-google/README.md)
