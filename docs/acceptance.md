# Owner acceptance coverage (opt-in)

UNI only checks claims explicitly stated in a contract. The optional acceptance
manifest introduces a separately reviewed list of criteria and identifies
missing, weakened or unverified claims **before running an agent or verifier**.

This checks declared scope only. It does not discover requirements omitted from
the manifest, authenticate who wrote it, or guarantee a verifier actually tests
an ENSURE statement. Reports identify the authority as
CALLER_SUPPLIED_NOT_AUTHENTICATED and scope as DECLARED_CRITERIA_ONLY.

## Example

An owner maintains a TOML manifest separate from the implementing agent's
write permissions:

~~~toml
version = 1
intent = "ledger"

[[criteria]]
id = "regular"
claim = "regular"
ensure = "regular credits and debits work"
verifier = "owner.valid"

[[criteria]]
id = "overdraft"
claim = "overdraft"
ensure = "overdraft must be refused"
verifier = "owner.overdraft"
~~~

Each criterion must map to a distinct required or critical claim and a trusted
named verifier registered in .uni/config.toml. Its ENSURE text must match the
contract text. The manifest and its approved revision remain under owner
control; the digest in the report is an identification aid, not an approval
signature.

## Commands

~~~sh
uni --json coverage contract.uni --manifest owner.toml
uni verify contract.uni --acceptance owner.toml
uni run contract.uni --acceptance owner.toml -- echo finished
~~~

The coverage command inspects declarations without running commands. The
verify and run gates reject incomplete coverage with nonzero exit status.
The run gate checks before starting the executor and again after delivery.

Existing usage without an acceptance manifest remains unchanged. A production
repository must make the acceptance-aware invocation a required CI check: an
agent can otherwise omit the optional flag. Treat edits to the criteria and
trusted verifier registry as sensitive changes requiring human review.

The word COMPLETE means all **declared** criteria have matching contract
mappings. It is not an assurance of all unstated requirements or independent
proof of correctness. See issue #13 and the reproducible synthetic ledger
counterexample in experiments/assurance-bench.

## Optional external revision pin

Set `UNI_ACCEPTANCE_SHA256` to the exact SHA-256 of the reviewed manifest bytes before calling `uni coverage`, `uni verify --acceptance` or `uni run --acceptance`. UNI refuses an invalid or mismatched configured pin. A matching hash reports `revision_pin = MATCHED`; without a configured pin it reports `UNPINNED`. An empty configured value is invalid, not equivalent to no pin.

For CI, provision the digest through an owner-controlled repository or organization variable and require the acceptance-aware check. A caller can forge its own digest, so hash matching by itself **does not establish who authorized the manifest**; use your platform's access controls and reviews for that. Update the protected expected digest only after explicitly approving a new manifest revision.
