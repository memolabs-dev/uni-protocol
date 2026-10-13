# Independent oracle benchmark scaffold

This reproducible harness is a **study design and validation tool**, not an
evaluation result. The bundled cases are **scripted synthetic fixtures, never
real agent runs**. They demonstrate a plausible incorrect patch that passes
visible positive behavior but fails an owner invariant.

## Separation and security

The implementer receives a **candidate-only** working directory. The owner
holds acceptance tests in a disjoint directory and runs them after delivery.
The harness refuses overlapping baseline, candidate and oracle paths,
rejects a base task that already passes owner tests, and rehashes all three
source trees around oracle execution. If baseline, candidate or owner oracle
changes while a test runs, the evaluation is **invalid**, not a scored result.
The check compares before/after file-tree SHA-256 hashes; it cannot promise an
atomic filesystem snapshot or detect a file changed and restored between reads.
It never invents a worker self-report: that comes only from the actual last
line of the supplied worker
output. It never invents a UNI verdict: absent `uni --json verify` output is
`UNOBSERVED`, not `REJECTED`.

**This is not a sandbox.** Python candidate code and owner test code execute
as processes. For actual untrusted agent submissions use disposable,
network-restricted CI runners/containers with explicit resource caps. Never
execute untrusted candidates on a maintainer machine. This harness only runs
the included controlled fixtures in UNI's regular CI.

## Reproduce the bundled scripted fixture

Run from the UNI repository root:

```bash
python3 -m unittest discover -s experiments/assurance-bench/tests -p 'test_*.py'

python3 experiments/assurance-bench/harness.py evaluate \
  --case-id ledger-plausible-wrong --kind scripted \
  --baseline experiments/assurance-bench/fixtures/baseline \
  --candidate experiments/assurance-bench/fixtures/plausible-wrong \
  --oracle experiments/assurance-bench/fixtures/owner-oracle \
  --out /tmp/uni-bench-wrong.jsonl
python3 experiments/assurance-bench/harness.py evaluate \
  --case-id ledger-correct --kind scripted \
  --baseline experiments/assurance-bench/fixtures/baseline \
  --candidate experiments/assurance-bench/fixtures/correct \
  --oracle experiments/assurance-bench/fixtures/owner-oracle \
  --out /tmp/uni-bench-correct.jsonl
python3 experiments/assurance-bench/harness.py summarize \
  /tmp/uni-bench-wrong.jsonl /tmp/uni-bench-correct.jsonl
```

The scripted wrong candidate keeps positive behavior working but violates the
negative-balance invariant. The corrected candidate satisfies all three held-out
tests. Neither synthetic case has a UNI report or authentic worker output, so
the grader reports **unobserved** UNI and agent outcomes rather than inventing
counts. No advantage over baseline CI is claimed from these fixtures.

## Protocol for future real-agent runs

1. Freeze and hash the starting baseline, owner oracle and acceptance criteria.
   Keep oracle files outside the worker checkout and reject a baseline that
   already passes. The implementer can receive a contract and a briefing, but
   not the owner's hidden invariant test source.
2. Deliver exactly the candidate revision and preserve the worker's raw output,
   actual model/agent version and run conditions. Do not infer DONE from success
   of the harness; parse the worker's final line or mark UNPARSED.
3. Produce the UNI verdict from a real `uni --json verify` execution. Pass its
   raw JSON using `--uni-json`; preserve CLI exit code and revision identity in
   the study manifest. Record CI independently, never infer it from UNI.
4. Run the trusted oracle *after* implementation, in a disposable isolated
   environment, with identical acceptance conditions for baseline and delivery.
   If tests cannot execute, mark the observation invalid, not failed.
5. Aggregate using `summarize`, separately for `scripted` and `real-agent`.
   False acceptance = oracle rejects while mechanism accepts; false rejection =
   oracle accepts while mechanism declines. Unknown observations are excluded
   from rates but explicitly counted. Preserve failures and report denominators.

Future work (#3): real-world multi-file tasks, independent test verification,
adjudication notes, repeated runs across models and stacks, coverage, CI
overheads, confidence intervals, and publication of the oracle revision hashes.
This is a methodology baseline, not a population estimate or product claim.

## An actual UNI CLI result on the synthetic fixture

The integrity test `tests/test_real_uni_cli.py` builds a minimal visible-test
contract in a disposable copy of each candidate and invokes the **real**
`uni --json verify` binary. The intentionally incomplete delivery and the
correct delivery both pass those two visible ledger checks, so UNI reports
`Accepted` on both. The disjoint owner oracle rejects the former because
it violates the negative-balance invariant. This demonstrates a **coverage
limit**, not an agent failure rate or a defect in a claim UNI was asked to
verify. It makes the limit repeatable rather than relying on a fabricated
`uni_decision` field. CI builds UNI before the harness tests.

This is precisely why the next study needs externally reviewed contracts and
explicit coverage criteria; adding more generated checks is not the same as
proving the owner's full desired outcome.
