# UNI × CUESHEET — bounded owner-acceptance pilot

Status: **reproducible controlled dogfood**, issue [#5](https://github.com/memolabs-dev/uni-protocol/issues/5)
remains open until this gate is exercised on an actual CUESHEET PR before
release. No hosted LLM, agent API key, or production account is used.

## Scenario

The scope is one CUESHEET code-worker safety invariant: worker effects must
never grant the worker authority to forge completion receipts. The owner-held
`contract.uni`, `acceptance.toml` and `config.toml` reside in **UNI**,
not in the implementing agent's CUESHEET checkout. They authorize the genuine
CUESHEET unit suite at `test/code-worker-loop.test.ts` (7 assertions).

The fixed demonstration revision is
`d07c3d5ffc3b67b8046da454a4d1c78f465b2961` in
[Memo Labs Dev CUESHEET](https://github.com/memolabs-dev/cuesheet).
The script refuses dirty trees or unexpected revisions, copies the source to
a disposable temporary workspace, pins the owner manifest digest before
copying it, then invokes the **actual** UNI `coverage` and `verify` CLIs.
It deliberately removes `finish` from CUESHEET's forbidden tools in the
*disposable copy only*, re-verifies, and requires an inline stale-evidence
annotation on the changed source path. The real CUESHEET checkout is untouched.

### Run

Prerequisites: local Rust toolchain, Python 3, Node.js 24+ and clean CUESHEET
checkout at the exact revision above.

```bash
cargo build --release -p uni-cli
python3 experiments/cuesheet-dogfood/demo.py \
  --cuesheet /path/to/clean/cuesheet \
  --uni ./target/release/uni \
  --expected-revision d07c3d5ffc3b67b8046da454a4d1c78f465b2961
```

Or dispatch `.github/workflows/cuesheet-dogfood.yml` in UNI Actions. The
workflow checks out both repositories, uses a GitHub-hosted disposable runner,
publishes measured timing in the step summary, and emits a warning annotation
for deliberate drift. No new cloud product is required.

### Measurement (one controlled macOS run, 2026-10-13)

- Owner bundle setup: **433 ms**; valid UNI verification: **158 ms**.
- Deliberately modified verifier subject: UNI `EvidenceRequired` in **150 ms**,
  with an inline annotation naming
  `src/adapters/code-worker-loop.ts`.
- Controlled clean-sample false alerts: **0 of 1**; **real-agent runs: 0**.

These numbers are a single bounded observation, not CI runtime benchmarks or
population-level false alert rates. CI will report its own timings when
dispatched. Completion of #5 still requires a genuine CUESHEET PR candidate,
separately approved owner criteria, repeat runs, and a release blocking check.

### Trust limits

The SHA-256 digest detects a changed manifest relative to the checked-in
owner bundle; it does **not** authenticate an external human. The owner must
protect the UNI repository and acceptance variables in CI. `COMPLETE` means
coverage of **declared** criteria only, not all unspoken product expectations.
The script invokes CUESHEET source tests; run only reviewed source on an
isolated worker, not arbitrary third-party submissions on a workstation.
