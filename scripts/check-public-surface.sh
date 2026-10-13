#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "public-surface check: $*" >&2
  exit 1
}

version="$(awk -F '"' '/^version = "/ { print $2; exit }' Cargo.toml)"
[ -n "$version" ] || fail "workspace version not found"

action_version="$(awk '
  $1 == "version:" { in_version = 1; next }
  in_version && $1 == "default:" { gsub(/"/, "", $2); print $2; exit }
' adapters/github/action.yml)"
[ "$action_version" = "v$version" ] ||
  fail "action default $action_version does not match workspace v$version"

grep -Fq "latest published binary is v$version" README.md ||
  fail "README does not name the published workspace version"
grep -Fq "latest published release v$version" docs/current-status.md ||
  fail "current status does not name the published workspace version"

grep -Fq '.uni/contracts/candidate-checkout.uni' docs/quickstart.md ||
  fail "quickstart does not use the importer's candidate path"
grep -Fq '.join(format!("candidate-{intent_id}.uni"))' crates/uni-cli/src/cmd/speckit.rs ||
  fail "importer candidate path changed without a quickstart review"
grep -Fq '.join(format!("candidate-{intent_id}.brief.md"))' crates/uni-cli/src/cmd/speckit.rs ||
  fail "importer brief path changed without a quickstart review"

if grep -Fq 'complete report' README.md docs/index.md; then
  fail "a historical report is still presented as current"
fi

if grep -Eiq 'lab VPS|production host|machine de production|lab-infra|docker group|stacks.*\.env|org disabled|absent on this machine' \
  .github/workflows/uni.yml docs/github-integration.md docs/REPORT-2026-09-14.md \
  experiments/study-50/README.md .scratch/tickets.md; then
  fail "private operational detail returned to a public document"
fi

if grep -En '[—–]|&mdash;|&ndash;' README.md CONTRIBUTING.md SECURITY.md \
  CODE_OF_CONDUCT.md docs/current-status.md docs/index.md docs/quickstart.md \
  docs/github-integration.md; then
  fail "public prose contains a banned dash"
fi

grep -Fq 'magic="$(od -An -tx1 -N2 "$archive"' adapters/github/action.yml ||
  fail "action no longer checks the downloaded archive format"
grep -Fq 'python scripts/release_archive.py package' \
  .github/workflows/release.yml ||
  fail "release workflow no longer uses cross-platform deterministic gzip tar packaging"
grep -Fq 'python scripts/release_archive.py verify' \
  .github/workflows/release.yml ||
  fail "release workflow no longer validates archive extraction and binary bytes"
grep -Fq 'uni-${{ matrix.target }}.tar.gz.sha256' \
  .github/workflows/release.yml ||
  fail "release workflow no longer publishes a checksum for each archive"
grep -Fq 'examples/artifact/digest.txt text eol=lf' .gitattributes ||
  fail "file-hash smoke fixture can change bytes on Windows checkout"

for path in \
  CODE_OF_CONDUCT.md \
  CONTRIBUTING.md \
  SECURITY.md \
  docs/current-status.md \
  docs/quickstart.md \
  experiments/stale-bench/RESULTS-2026-09-14.md \
  experiments/study-50/RESULTS-2026-09-14.md \
  experiments/study-50/RESULTS-2026-09-19-adversarial-arm.md \
  experiments/study-50/RESULTS-2026-09-28-drift-arm.md; do
  [ -f "$path" ] || fail "linked public file is missing: $path"
done

echo "public-surface check: ok (v$version)"
