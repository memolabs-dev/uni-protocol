# UNI: outcome assurance landscape — research snapshot

Date: 2026-10-12. Status: evidence-backed architecture research, **not** a claim that UNI beats existing tools.

## Thesis and category

UNI should remain the local, deterministic **outcome assurance** layer: intent, observable claims, independent verifier evidence, policy and decision. A run trace or an agent's self-reported success is not proof that the shipped artifact satisfies the owner's requirements. Conversely, a passing UNI decision only proves the specific claims and subjects that its trusted verifiers actually observed.

The strongest initial buyer/user is a software engineer or team shipping code through agent-driven pull requests. The first product is a developer CLI and a PR check, not a hosted agent orchestration framework.

## Adjacent products, not identical substitutes

| Project or standard | Main job | What UNI should learn | What UNI should not copy |
|---|---|---|---|
| OpenAI Agent Evals and trace grading | Diagnose runs with traces, datasets and graders | Repeatable scored studies and failure taxonomy | Model judgment in the deterministic trust path |
| Inspect AI (UK AISI) | Reproducible agent evaluation harness with external agents | Reuse evaluator harnesses and held-out datasets | Rebuild a complete model-eval framework |
| Pydantic Evals | Code-first deterministic/model-based graders, trajectories | First-class task cases and report structure | Treat a scored trajectory as an artifact proof |
| LangSmith | Online/offline datasets, evaluators and experiment comparison | Human-labeled ground truth and longitudinal regressions | Become a centralized observability SaaS |
| Arize Phoenix | OTEL traces, datasets, evaluation and debugging | Optional OTLP export and trace correlation | Confuse log events with authorization to accept |
| AgentSpec (research) | Enforce runtime triggers, predicates and constraints | Explicitly distinguish before-action policy from after-action assurance | Promise prevention of unsafe actions by a post-run checker |
| OpenAI Agents SDK guardrails | Input/output/tool validation and human approval boundaries | Fail-closed boundary design; complement tool guardrails | Duplicate agent SDK or tool execution |
| in-toto attestations + SLSA | Standard representations of artifact identity and signed supply-chain provenance | Subject-digest binding and optional attestations | Assert signed provenance without real key management and signature verification |
| OpenTelemetry GenAI conventions | Interoperable spans for agents and tools | Map uni events to stable attributes in an optional adapter | Make remote tracing a core dependency |
| SWE-bench Pro | Harder, realistic coding benchmark cases | Independent hidden tests, task stratification and decontamination | Advertise SWE-bench performance without running a valid study |

**Competitive positioning:** Most tools answer how an agent behaved, or how frequently it succeeds on a dataset. UNI answers whether particular claims about *this deliverable* have sufficiently strong evidence *under a declared policy*. This differentiation is useful but not automatically a moat: deterministic evaluators and CI already exist. UNI must win on evidence validity over time, usability, and measurable reduction in bad merges.

## Current shipped UNI assets (v0.9.4)

- Rust CLI, .uni DSL, compilation/lint, trusted registry, filesystem evidence, deterministic decisions, GitHub action.
- Proof invalidation on tracked content, commit, contract, verifier configuration, authorization and expiry.
- Local reports, explanations, journaling and export.
- Identity binding via offline JWT verification against pinned issuer keys.
- Baseline docs/current-status.md records the small corrected n=6 real-agent study and artificial drift benchmarks; never turn fixture-specific results into broad claims.

## Ranked improvements

### P0 — establish a rigorous evaluation claim

1. Define a task corpus with independently held-out owner acceptance criteria. Do not expose the hidden oracle to the coding agent or let an implementing agent edit the oracle.
2. Run baseline GitHub CI and UNI on the same revisions, policies and machines. Record blind spots and false positives, not only wins.
3. Track false accepts, false rejects, eligible-claim coverage, stale-detection precision/recall, latency, compute cost and human contract-authoring time.
4. Label synthetic failures, adversarial scripted tasks and real-agent failures separately; do not present pooled percentages.
5. Record exact task, agent, model, code revision, contract, verifier revision, environment, and adjudication procedure.

### P1 — evidence and verification trust boundary

1. Explicitly model covered artifact subjects and uncovered subjects; empty coverage must not silently become ACCEPTED.
2. Add focused regression cases: test/registry tampering, symlinks escaping watched roots, changed untracked files, stale commit, moved contract, changed authorization and expired proof.
3. Evaluate hermetic or isolated verifier execution; distinguish a command that the agent can edit from an owner-controlled oracle.
4. Support multiple independent verifiers for a claim *only* when evidence independence is real and documented.
5. Maintain fail-closed semantics on unknown DSL features and malformed evidence.
6. Evaluate declaration-only Git diff scope coverage; never advertise whole-repository assurance from selected globs.

### P2 — adoption and interoperability

1. Make first run convincing on Node/TypeScript, Python and Rust with small explicit starter registries, not Cargo-only defaults.
2. Improve PR annotations: artifact path, claim, why evidence ceased to apply, and precise reproduction command.
3. Generate a concise agent handoff and PR result; an agent should know what will be checked before writing code.
4. Add optional in-toto-compatible outcome attestations bound to subject digests; signatures/A4 only after independent producer and verifier exist.
5. Export OTel GenAI-compatible trace correlations separately from evidence decisions.
6. Validate integrations first with CUESHEET; let Drop consume UNI decisions as an optional downstream signal, never as a silent blocker until proven.

## What counts as success

A v1.0 product candidate should satisfy a **published, reproducible** matrix across representative repositories; every accepted claim has explicit subject, trusted verifier and policy provenance; stale evidence reliably rejects declared revisions; CI works on all supported platforms; the quickstart takes minutes; and limitations are visible in every report. Do not claim 100% correctness or a universal ability to verify subjective tasks.

## Key reading (accessed 2026-10-12)

- OpenAI, Agent Evals: https://developers.openai.com/api/docs/guides/agent-evals
- OpenAI, SDK guardrails and approval: https://developers.openai.com/api/docs/guides/agents/guardrails-approvals
- UK AISI Inspect: https://inspect.aisi.org.uk/
- Pydantic Evals: https://pydantic.dev/docs/ai/evals/evals/
- LangSmith evaluation types: https://docs.langchain.com/langsmith/evaluation-types
- Arize Phoenix: https://github.com/Arize-ai/phoenix
- AgentSpec paper: https://arxiv.org/abs/2503.18666
- in-toto Statement: https://github.com/in-toto/attestation/blob/main/spec/v1/statement.md
- in-toto framework: https://github.com/in-toto/attestation/blob/main/spec/README.md
- SLSA specification: https://slsa.dev/spec/v1.2/
- OpenTelemetry GenAI agent spans (development status): https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-agent-spans.md
- OpenAI, why SWE-bench Verified is no longer recommended (2026): https://openai.com/index/why-we-no-longer-evaluate-swe-bench-verified/
- GitHub protected branches: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches
