# UNI release integrity and provenance — v0.9.5 candidate

This document separates **archive correctness**, **asset integrity**,
**build reproducibility**, **provenance** and **owner authorization**. None
alone proves all the others.

## Five release targets

The tag-triggered release workflow builds:
`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-apple-darwin`, `aarch64-apple-darwin`, and
`x86_64-pc-windows-msvc`. Each uploaded pair is:

- `uni-<target>.tar.gz`: genuine gzip stream containing **one** ordinary
  file, `uni` on Unix or `uni.exe` on Windows.
- `uni-<target>.tar.gz.sha256`: two-column SHA-256 checksum with filename.

`scripts/release_archive.py` creates deterministic tar headers (zero mtime,
stable owner/permissions) and a gzip header with no original filename or
timestamp. It **re-opens every archive**, checks magic, member name, count,
type and bytes against the just-built binary before generating the checksum.
The cross-platform CI job runs its fixture tests and fails rather than
uploading a malformed asset. Deterministic **packaging** does not imply
bit-for-bit reproducible builds across Rust compiler or platform versions.

## Consumer verification

For a downloaded archive and sidecar, run in the same directory:

```bash
shasum -a 256 -c uni-aarch64-apple-darwin.tar.gz.sha256
tar -tzf uni-aarch64-apple-darwin.tar.gz
```

On Windows, PowerShell's `Get-FileHash -Algorithm SHA256` can compare the
digest to the sidecar, and `tar -tzf` should print only `uni.exe`.
The Python verifier can check the archive structure without unpacking:

```bash
python scripts/release_archive.py verify --target aarch64-apple-darwin \
  --archive uni-aarch64-apple-darwin.tar.gz
```

Legacy v0.9.4 Windows assets were ZIP payloads under a `.tar.gz` name.
The old composite GitHub Action retains an explicitly restricted fallback for
that known tag. **v0.9.5+ release archives must be real gzip tarballs**.

## Trust boundary and attestation roadmap

Checksums downloaded from the **same release page** detect accidental
corruption, not a compromised release publisher that replaces both assets.
Git tags, GitHub-hosted runners, third-party Actions, Rust toolchains and
repository release permissions are separate trust boundaries.

A future signed provenance stage should:

1. Use GitHub OIDC with `actions/attest-build-provenance` on each exact asset
   SHA-256; grant only required `id-token: write` and `attestations: write`
   to that job. Pin external action revisions for release-critical paths.
2. Publish an in-toto/SLSA predicate that names the source repository,
   immutable commit, workflow identity, tag and subject digest.
3. Require consumers to validate issuer, repository/workflow, signer identity
   and digest (e.g. with `gh attestation verify`) before claiming provenance.
4. Define signing trust root, key/issuer rotation, revocation, independent
   owner review and reproducible-build checks before labeling UNI evidence A4.

**Current state:** checksum + archive validation only; no independent human
signature or verified artifact attestation in this workflow. Do not call
these bundles 'signed A4'. Review the permission and lifecycle design before
enabling provenance enforcement.
