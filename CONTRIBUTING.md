# Contributing to UNI

Thanks for looking. This is a small, opinionated project. Read the constitution,
run the tests, then coordinate the change through GitHub Issues.

## What UNI is, in three sentences

UNI is the outcome assurance layer: a declarative contract says what must be
true, a trusted registry says how to prove it, verifiers produce evidence bound
to a revision, and a deterministic engine decides whether the result is
acceptable. No language model takes part in the path from verification to
decision. `constitution.md` is the twelve rules everything else answers to, and
`CONTEXT.md` is the glossary.

## Set up

```bash
git clone https://github.com/memolabs-dev/uni-protocol
cd uni-protocol
cargo build --release
./target/release/uni verify examples/hello/hello.uni   # end to end, no network
cargo test
```

Before opening a pull request, run what CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## The workflow worth knowing

Work is contract first, then proof. A change is done when a verifier proves it,
never when a description says so:

1. Write or edit the `.uni` contract (or an adapter, or a test).
2. Prove it: `uni verify <contract>`, then `uni explain`.
3. Open a pull request. The check in the pull request is the argument.

Two habits make review quick. First, a new behaviour gets a test at the level it
lives: the low crates (`uni-parser`, `uni-ir`, `uni-evidence`, `uni-verify`,
`uni-decision`) take unit tests beside the code, the CLI takes an integration
test under `crates/uni-cli/tests/`. Second, if a claim in prose is not backed by
a verifier, it does not belong in a document as a fact.

## House rules that are not negotiable

These come from the constitution, and a pull request that breaks one will be
asked to change even if it works:

- **Contracts are untrusted, the registry is trusted.** A contract may only name
  verifiers. Anything that lets a contract introduce a command, or lets a token
  be trusted from a URL fetched at runtime, is a security regression.
- **No inert semantics.** The vocabulary is closed. If a keyword is not
  implemented, it is a located hard error naming its line, never silently
  ignored text. `REJECT WHEN` and `ESCALATE WHEN` are reserved and must keep
  failing.
- **The decision stays deterministic.** Identical contract, evidence and policy
  must yield an identical decision. Nothing in the decision path reads the
  network, the clock, or randomness. The two clock reads that exist (evidence
  expiry, token `exp`) gate availability, not the verdict.
- **Evidence must fully cover what it claims.** Partial stdout, the wrong
  revision, the wrong environment, or an expired proof invalidates. If your
  change narrows what a verifier observes, say so in the pull request.

## Where help is most useful

Ordered by what unblocks the most:

- **Independent reproductions.** Re-run a published study arm, preserve the
  result row and evidence bundle, and report any disagreement before changing
  the harness. The scripted adversarial arms prove that the mechanism fires on
  two fixtures. They do not establish a model failure rate.
- **A real non-conforming implementer.** The study's honest gap remains: the
  committed wrong deliveries are scripted. A run from a real implementer that
  reports completion while missing a hidden owner invariant would add evidence
  the current study does not have.
- **Verifier adapters.** The shell verifier is the only executor today. A
  first-class adapter for a real runtime is a contained, well-tested change:
  `docs/writing-verifiers.md` has the interface and the `file-hash` adapter is
  the smallest example.
- **Documentation that is wrong.** A doc that describes behaviour the binary does
  not have is a bug. `docs/index.md` says which doc owns which subject.

## Out of scope on purpose

These are decisions, not oversights. A pull request adding them will be declined
until the constitution changes: orchestration and multi-agent frameworks, a
control plane, a policy engine (OPA is wired through the `PolicyProvider` trait
and UNI never reimplements one), Postgres and Protobuf in the core, and MCP or
A2A servers. See
`specs/001-uni-software-v01/spec.md` for the v0.1 boundary.

## Pull requests

Small and focused beats large and complete. For anything larger than a bug fix,
open a GitHub issue first so the outcome and proof can be agreed before you
write it. Check for an existing issue before creating a new one. In
the pull request, say what would have to be true for the change to be wrong
(that is the useful review question), and include the exact command you ran when
the answer is not obvious from the diff.

Public contribution work is coordinated in GitHub Issues. The maintainer may
keep local notes for solo work, but contributors are not expected to discover
or update those notes.

## Security

Please do not open a public issue for a vulnerability. See `SECURITY.md`.

Participation in the project is governed by `CODE_OF_CONDUCT.md`.

## Licence

UNI is dual licensed under MIT or Apache-2.0, at your option, and contributions
are accepted under the same terms. By opening a pull request you confirm you
wrote the change and that you have the right to submit it under that licence.
