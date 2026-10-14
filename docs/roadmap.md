# UNI product roadmap — Memo Labs Dev

Updated: 2026-10-12. Owner: Memo Labs Dev (memolabs-dev). Maturity: developer preview v0.9.4.

## Product contract

**UNI verifies claims about the artifact you ship; it does not trust an agent's statement that the task is done.**

UNI does not plan tasks, orchestrate agents, host accounts, replace CI, enforce all runtime side effects, or prove properties its trusted verifiers cannot observe. It complements CUESHEET (execution coordination) and Drop (product/workflows), and can remain standalone.

## Operational policy

- Repository: https://github.com/memolabs-dev/uni-protocol
- Automation identity: the memo-labs-automation GitHub App (`memo-labs-automation[bot]`); owner's account also allowed. Its installation must include this repository before it can open PRs here. The legacy user account drop-agent-bot was retired on 2026-10-14 (blocked by the organization's secure 2FA requirement).
- Pull requests and required CI before merge; native auto-merge only for trusted in-repository authors after green required checks.
- An explicit hold-merge label or draft state stops automated merge. A PR from a fork is never auto-merged.
- Never check out or execute PR code in a privileged pull_request_target workflow.
- Changes touching trusted verifiers, identity roots or authorization policy warrant focused human attention before relying on them: tests alone cannot decide trust.
- Use only GitHub-hosted disposable runners for untrusted PR builds.
- The constitution and canonical spec remain single sources of truth. No framework rewrite or new cloud service in the core.

## Milestones and acceptance

### M0 — Foundation (immediate)

- [x] Transfer repository to memolabs-dev.
- [x] Grant drop-agent-bot write permissions.
- [ ] Update public and release-action references after transfer.
- [ ] Install a single stable required CI status aggregating the full matrix, examples, action smoke and lint.
- [ ] Protect main: PR required, CI required, admins cannot bypass, no force push/delete.
- [ ] Enable native auto-merge after required checks, without any bypass.
- [ ] Record a verifiable baseline on Mac plus GitHub-hosted runners.

Done when: an intentionally failing PR cannot merge, a green trusted PR can auto-merge, and the published action still downloads the correct release.

### M1 — Credible independent evaluation (next)

- [ ] Publish a preregistered, reproducible evaluation protocol and independent oracle design.
- [ ] Extend tasks beyond scripted fixtures to real-agent, cross-stack, held-out tasks.
- [ ] Report false accepts, false rejects, coverage and authoring/CI overhead with confidence intervals.
- [ ] Add and keep regression fixtures for mutation and contract tampering.
- [ ] Reproduce results in GitHub CI; preserve enough metadata to audit claims later.

Done when: external contributors can reproduce both successful detections and failures without private infrastructure or undocumented tokens.

### M2 — Harden evidence semantics

- [ ] Audit path canonicalization, symlink boundaries and untracked/ignored file drift.
- [ ] Audit executable/registry tampering, proof replay, environmental assumptions and signer trust.
- [ ] Make partial coverage unmistakable in reports; explicit unverified state.
- [ ] Expand independent verifier adapters and safe execution options without introducing command injection.
- [ ] Expand unit, property-based and cross-platform tests.

Done when: every declared evidence dimension has positive and negative tests, and no silently skipped verifier is treated as proof.

### M3 — Developer experience and adoption

- [ ] Node/TS, Python and Rust profile starters with runnable examples.
- [ ] One-command onboarding and stable contract templates.
- [ ] Accurate inline GitHub PR annotations and compact explanations.
- [ ] Documented integration with CUESHEET in an isolated example.
- [ ] Versioned migrations for contract, IR, evidence and action formats.

Done when: a developer outside the project can install, add a contract, fail a deliberately wrong patch and understand why within a short tutorial.

### M4 — Trustworthy portability and 1.0 decision

- [ ] Optional in-toto-compatible subject-attestation export and offline verifier.
- [ ] Signatures only once a real independent producer/validator and key lifecycle exist.
- [ ] Optional OpenTelemetry correlation bridge; no telemetry/cloud dependency in core.
- [ ] Supply-chain hardening for release artifacts (digests, provenance, versioned action).
- [ ] Full migration notes, threat model, compatibility and honest benchmark.
- [ ] Freeze v1 compatibility only after usability and security acceptance.

Done when: UNI's evidence is independently interpretable and reproducibly checks real PRs across platforms.

## Non-goals

Do not build a hosted control plane, database server, generic agent SDK, runtime policy engine or prediction market. Do not use an LLM judge as the only verifier for a critical guarantee. Do not claim artifact-wide correctness from a narrow checked file set.

## Research

See [outcome assurance landscape](research/outcome-assurance-landscape-2026-10.md). Past data and caveats: [current-status](current-status.md).
