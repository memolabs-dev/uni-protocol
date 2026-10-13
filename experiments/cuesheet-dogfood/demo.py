#!/usr/bin/env python3
"""Bounded, owner-pinned UNI acceptance demo against a CUESHEET source revision.

This is a controlled fixture, not a production gate or a model benchmark.
The CUESHEET source is copied to a disposable directory; never edit the input.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from time import monotonic
import os

OWNER = Path(__file__).resolve().parent
SOURCE = "src/adapters/code-worker-loop.ts"
# Drop the forbidden "finish" tool from the denylist: the owner test must
# reject this unsafe worker change without waiting for an inference timeout.
DRIFT_FROM = "'git','finish','prepare_workspace'"
DRIFT_TO = "'git','prepare_workspace'"


def run(command, *, cwd, env=None, check=True):
    result = subprocess.run(
        [str(part) for part in command], cwd=cwd, env=env,
        text=True, capture_output=True, check=False, timeout=120,
    )
    if check and result.returncode:
        raise RuntimeError(
            f"{' '.join(map(str, command))} exited {result.returncode}: "
            f"{result.stderr[-1200:]} {result.stdout[-500:]}"
        )
    return result


def uni(uni_bin, workspace, env, *args):
    result = run([uni_bin, "--json", *args], cwd=workspace, env=env, check=False)
    try:
        output = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"uni {' '.join(args)} did not return JSON: {result.stdout} {result.stderr}") from exc
    return result.returncode, output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cuesheet", type=Path, required=True, help="clean CUESHEET source checkout")
    parser.add_argument("--uni", type=Path, required=True, help="path to built UNI CLI")
    parser.add_argument("--expected-revision", help="optionally require this CUESHEET commit SHA")
    args = parser.parse_args()
    source = args.cuesheet.resolve(strict=True)
    binary = args.uni.resolve(strict=True)
    if not (source / SOURCE).is_file() or not (source / "test/code-worker-loop.test.ts").is_file():
        parser.error("not a checkout containing the required CUESHEET source and test")
    revision = run(["git", "rev-parse", "HEAD"], cwd=source).stdout.strip()
    if args.expected_revision and revision != args.expected_revision:
        parser.error(f"CUESHEET revision mismatch: expected {args.expected_revision}, got {revision}")
    if run(["git", "status", "--porcelain"], cwd=source).stdout.strip():
        parser.error("refusing a dirty CUESHEET checkout; use a clean pinned revision")
    owner_bytes = (OWNER / "acceptance.toml").read_bytes()
    pin = hashlib.sha256(owner_bytes).hexdigest()
    started = monotonic()
    with tempfile.TemporaryDirectory(prefix="uni-cuesheet-dogfood-") as temp:
        workspace = Path(temp) / "worktree"
        shutil.copytree(
            source, workspace,
            ignore=shutil.ignore_patterns(".git", "node_modules", "dist", ".cuesheet", ".cuesheet-project", "__pycache__"),
        )
        (workspace / ".uni").mkdir(exist_ok=True)
        shutil.copyfile(OWNER / "config.toml", workspace / ".uni/config.toml")
        shutil.copyfile(OWNER / "contract.uni", workspace / "owner.uni")
        shutil.copyfile(OWNER / "acceptance.toml", workspace / "owner.toml")
        run(["git", "init", "-q"], cwd=workspace)
        run(["git", "add", "-A"], cwd=workspace)
        run(["git", "-c", "user.email=demo@example.invalid", "-c", "user.name=UNI Dogfood",
             "commit", "-qm", "owner-approved baseline"], cwd=workspace)
        setup_ms = round((monotonic() - started) * 1000)
        env = dict(os.environ, UNI_ACCEPTANCE_SHA256=pin)
        coverage_code, coverage = uni(binary, workspace, env, "coverage", "owner.uni",
                                      "--manifest", "owner.toml")
        if coverage_code or coverage.get("status") != "COMPLETE" or coverage.get("revision_pin") != "MATCHED":
            raise RuntimeError(f"trusted acceptance manifest not covered/pinned: {coverage}")
        verify_start = monotonic()
        accepted_code, accepted = uni(binary, workspace, env, "verify", "owner.uni",
                                      "--acceptance", "owner.toml")
        accepted_ms = round((monotonic() - verify_start) * 1000)
        if accepted_code or accepted.get("decision") != "Accepted":
            raise RuntimeError(f"bounded CUESHEET baseline not accepted: {accepted}")
        path = workspace / SOURCE
        content = path.read_text()
        if content.count(DRIFT_FROM) != 1:
            raise RuntimeError("unsupported CUESHEET revision: drift target changed")
        path.write_text(content.replace(DRIFT_FROM, DRIFT_TO, 1))
        drift_start = monotonic()
        changed_code, changed = uni(binary, workspace, env, "verify", "owner.uni",
                                    "--acceptance", "owner.toml")
        changed_ms = round((monotonic() - drift_start) * 1000)
        annotations = run([binary, "explain", "--annotations"], cwd=workspace, env=env, check=False)
        if changed_code == 0 or changed.get("decision") == "Accepted":
            raise RuntimeError(f"deliberately broken CUESHEET task was incorrectly accepted: {changed}")
        if f"file={SOURCE}" not in annotations.stdout:
            raise RuntimeError(f"stale-source annotation missing: {annotations.stdout} {annotations.stderr}")
        for line in annotations.stdout.splitlines():
            if line.startswith("::warning"):
                print(line)
        result = {
            "cuesheet_revision": revision,
            "owner_manifest_sha256": pin,
            "assurance_scope": "DECLARED_CRITERIA_ONLY",
            "baseline_decision": accepted["decision"],
            "deliberate_drift_decision": changed["decision"],
            "stale_annotation": True,
            "setup_ms": setup_ms,
            "accepted_verify_ms": accepted_ms,
            "drift_verify_ms": changed_ms,
            "controlled_clean_false_alerts": 0,
            "controlled_clean_samples": 1,
            "real_agent_trials": 0,
        }
        print(json.dumps(result, sort_keys=True))
        return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, OSError, subprocess.TimeoutExpired) as exc:
        print(f"CUESHEET dogfood invalid: {exc}", file=sys.stderr)
        raise SystemExit(2)
