# Quickstart (60 seconds)

## Build UNI and verify the bundled example

```bash
git clone https://github.com/memolabs-dev/uni-protocol.git
cd uni-protocol
cargo build --release
./target/release/uni lint   examples/hello/hello.uni   # preflight, no execution
./target/release/uni verify examples/hello/hello.uni   # trusted verifiers
./target/release/uni report                            # stable PR/CI view
./target/release/uni explain                           # per-claim evidence detail
```

Exit codes: 0 = ACCEPTED, 1 = REJECTED / EVIDENCE_REQUIRED / ESCALATED.

## Start in your own repository

Initialize the local registry and runtime directories, choosing the starter
commands for your stack:

```bash
uni init                   # Rust/Cargo (the default)
uni init --profile node    # npm scripts: typecheck, test, build
uni init --profile python  # Python: compileall and unittest discovery
```

Choose only **one** profile for a fresh repository. If `.uni/config.toml`
already exists, `uni init` never overwrites it, even with a different profile.

The profile seeds **trusted verifier commands**, not assertions that the
project supports them. Review `.uni/config.toml` before executing a contract,
adjust missing scripts or Python test runners, and keep missing checks as
failures rather than silently skipping them. The examples also include Python
`unittest`, Node `node --test`, POSIX shell checks, and file hashing.

Choose one contract path.

### Import a Spec Kit feature

Given a feature directory named `checkout`:

```bash
uni import-speckit path/to/checkout/
candidate=.uni/contracts/candidate-checkout.uni
brief=.uni/contracts/candidate-checkout.brief.md
uni lint "$candidate"
```

The import writes both files. Review the candidate contract and the brief before
any implementation starts. The importer extracts a candidate from `spec.md`,
`plan.md`, and `tasks.md`; it does not approve the result.

### Write a contract by hand

```bash
mkdir -p uni/intents
cp examples/hello/hello.uni uni/intents/feature.uni
uni brief uni/intents/feature.uni --out brief.md
uni lint uni/intents/feature.uni
```

After the contract and trusted registry match the outcome you want:

```bash
uni verify uni/intents/feature.uni
uni report
uni explain
```

The registry maps stable verifier names to commands. The contract names those
verifiers but cannot introduce a new command.
