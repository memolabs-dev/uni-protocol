# UNI public CLI proof — 24 October 2026

**Purpose:** give recruiters and external maintainers a truthful, short entry point to a reproducible CLI run, not a general product-readiness certificate.

- Tested release: [v0.9.4](https://github.com/memolabs-dev/uni-protocol/releases/tag/v0.9.4), downloaded from official GitHub release assets.
- Binary: `uni-aarch64-apple-darwin.tar.gz`.
- Host: macOS / Apple Silicon; isolated temporary Git clone of `memolabs-dev/uni-protocol`, source commit `cad0071c90edd7b383e552ae189221ce7c5a5cdc`.
- Video: [42-second captioned reconstruction](https://github.com/memolabs-dev/uni-protocol/releases/download/v0.9.4/uni-cli-real-execution-20261024.mp4) generated from saved **real** process output. It is not a screen recording and does not simulate additional success.
- The test's output and all commands are from the published release, not from a locally modified CLI build.

## Reproduce

```sh
git clone https://github.com/memolabs-dev/uni-protocol.git
cd uni-protocol
cargo build --release
./target/release/uni doctor
./target/release/uni lint examples/hello/hello.uni
./target/release/uni verify examples/hello/hello.uni
./target/release/uni report
```

The demonstration itself used the **published v0.9.4** binary, rather than the local build shown for ease of reproduction.

## Recorded observations

| Command | Observed exit | Relevant output |
| --- | --- | --- |
| `uni doctor` | 0 | `healthy: true`; 14 registry verifiers, git `dirty=false` at preflight |
| `uni lint examples/hello/hello.uni` | 0 | `errors=0 warnings=0` |
| `uni verify examples/hello/hello.uni` | 0 | `binary-builds PASS`; `Decision: Accepted` |
| `uni report` | 0 | `Claims 1/1 verified`; `Assurance A2` |

The reported A2 assurance explicitly states **independent actor: NO, identity: SELF-DECLARED**. An `Accepted` decision means this specific example satisfied its registered checks; it does **not** establish third-party attestation, deployment readiness, identity provenance, security, or successful outcomes in other projects.

This is a documentation/demonstration artifact. No production integration or externally commissioned independent assessment took place.
