#!/usr/bin/env python3
"""Independent-oracle grading for UNI, without pretending to run a model.

The implementer receives a candidate checkout only; the oracle is supplied
later, from a disjoint directory. Run only trusted oracle scripts and disposable
candidate fixtures on an isolated runner. This is NOT a sandbox.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


DECISIONS = {"ACCEPTED", "REJECTED", "EVIDENCE_REQUIRED", "ESCALATED"}
KINDS = {"scripted", "real-agent"}


class InvalidCase(ValueError):
    pass


def inside(path: Path, directory: Path) -> bool:
    return path == directory or directory in path.parents


def check_isolation(baseline: Path, candidate: Path, oracle: Path) -> None:
    paths = [p.resolve(strict=True) for p in (baseline, candidate, oracle)]
    if not all(p.is_dir() for p in paths):
        raise InvalidCase("baseline, candidate and oracle must be directories")
    a, b, c = paths
    if inside(a, b) or inside(b, a):
        raise InvalidCase("baseline and candidate workspaces overlap")
    if any(inside(c, work) or inside(work, c) for work in (a, b)):
        raise InvalidCase("owner oracle must be outside both implementer workspaces")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def candidate_digest(directory: Path) -> str:
    """Stable digest of the candidate, excluding only runtime/build metadata.

    Symlinks are forbidden here: a fixture must be an explicit set of files.
    """
    items = []
    ignored = {".git", ".uni", "__pycache__", ".pytest_cache", "target", "node_modules"}
    for base, dirs, files in os.walk(directory, followlinks=False):
        dirs[:] = sorted(d for d in dirs if d not in ignored)
        for name in sorted(files):
            path = Path(base) / name
            if path.is_symlink():
                raise InvalidCase(f"symlinked candidate path: {path}")
            rel = path.relative_to(directory).as_posix()
            items.append((rel, sha256(path.read_bytes())))
        for name in dirs:
            if (Path(base) / name).is_symlink():
                raise InvalidCase(f"symlinked candidate directory: {Path(base) / name}")
    if not items:
        raise InvalidCase("candidate is empty")
    return sha256(json.dumps(sorted(items), separators=(",", ":")).encode())


def oracle_run(oracle: Path, subject: Path, timeout: int) -> dict:
    env = os.environ.copy()
    env["UNI_BENCH_SUBJECT"] = str(subject)
    env["PYTHONNOUSERSITE"] = "1"
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    try:
        result = subprocess.run(
            [sys.executable, "-m", "unittest", "discover", "-s", str(oracle),
             "-p", "test_*.py", "-v"],
            cwd=oracle, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            timeout=timeout, text=True, check=False,
        )
    except subprocess.TimeoutExpired as exc:
        raise InvalidCase(f"oracle timed out after {timeout}s") from exc
    output = result.stdout
    if not ("Ran " in output and ("OK" in output or "FAILED (" in output)):
        raise InvalidCase("oracle did not produce a recognizable unittest result")
    return {"passed": result.returncode == 0, "output_sha256": sha256(output.encode())}


def parse_uni_report(path: Path | None) -> tuple[str, str | None]:
    if path is None:
        return "UNOBSERVED", None
    data = path.read_bytes()
    try:
        doc = json.loads(data)
    except json.JSONDecodeError as exc:
        raise InvalidCase(f"invalid UNI JSON: {path}") from exc
    if not isinstance(doc, dict) or not isinstance(doc.get("decision"), str):
        raise InvalidCase("UNI report must contain a decision string")
    decision = doc["decision"].upper()
    if decision not in DECISIONS:
        raise InvalidCase(f"unknown UNI decision: {decision}")
    return decision, sha256(data)


def parse_agent_report(path: Path | None) -> tuple[str, str | None]:
    if path is None:
        return "UNOBSERVED", None
    data = path.read_bytes()
    last = next((line.strip().upper()
                 for line in reversed(data.decode("utf-8").splitlines()) if line.strip()), "")
    return last if last in {"DONE", "FAILED"} else "UNPARSED", sha256(data)


def evaluate(args: argparse.Namespace) -> dict:
    baseline = args.baseline.resolve(strict=True)
    candidate = args.candidate.resolve(strict=True)
    oracle = args.oracle.resolve(strict=True)
    check_isolation(baseline, candidate, oracle)
    if not args.case_id.strip() or not args.run_id.strip() or args.kind not in KINDS:
        raise InvalidCase("case-id and run kind required")
    if args.kind == "real-agent" and args.agent_output is None:
        raise InvalidCase("real-agent runs require the worker's actual output")
    baseline_digest = candidate_digest(baseline)
    candidate_sha = candidate_digest(candidate)
    oracle_sha = candidate_digest(oracle)
    base = oracle_run(oracle, baseline, args.timeout)
    if candidate_digest(baseline) != baseline_digest:
        raise InvalidCase("baseline changed during owner-oracle execution")
    if candidate_digest(oracle) != oracle_sha:
        raise InvalidCase("owner oracle changed during baseline evaluation")
    if base["passed"]:
        raise InvalidCase("invalid baseline: owner oracle already passes on the starting task")
    delivery = oracle_run(oracle, candidate, args.timeout)
    # An oracle verdict is only meaningful for the exact source bytes named
    # in the recorded result. A fixture, worker or test may mutate them.
    if candidate_digest(candidate) != candidate_sha:
        raise InvalidCase("candidate changed during owner-oracle execution")
    if candidate_digest(oracle) != oracle_sha:
        raise InvalidCase("owner oracle changed during candidate evaluation")
    if candidate_digest(baseline) != baseline_digest:
        raise InvalidCase("baseline changed during candidate evaluation")
    uni, uni_sha = parse_uni_report(args.uni_json)
    agent, agent_sha = parse_agent_report(args.agent_output)
    return {
        "schema_version": 1,
        "case_id": args.case_id,
        "run_id": args.run_id,
        "run_kind": args.kind,
        "oracle_sha256": oracle_sha,
        "baseline_sha256": baseline_digest,
        "candidate_sha256": candidate_sha,
        "oracle_baseline_accept": base["passed"],
        "oracle_accept": delivery["passed"],
        "oracle_baseline_output_sha256": base["output_sha256"],
        "oracle_output_sha256": delivery["output_sha256"],
        "uni_decision": uni,
        "uni_report_sha256": uni_sha,
        "agent_self_report": agent,
        "agent_output_sha256": agent_sha,
        "ci_decision": args.ci_decision,
    }


def measure(rows: list[dict], method: str) -> dict:
    tp = tn = fa = fr = missing = 0
    for row in rows:
        if method == "UNI":
            result = row["uni_decision"]
            accept = result == "ACCEPTED"
            observed = result in DECISIONS
        elif method == "CI":
            result = row["ci_decision"]
            accept = result == "PASS"
            observed = result in {"PASS", "FAIL"}
        else:
            result = row["agent_self_report"]
            accept = result == "DONE"
            observed = result in {"DONE", "FAILED"}
        if not observed:
            missing += 1
            continue
        oracle = row["oracle_accept"]
        if accept and oracle:
            tp += 1
        elif accept and not oracle:
            fa += 1
        elif not accept and oracle:
            fr += 1
        else:
            tn += 1
    return {
        "observed": tp + tn + fa + fr,
        "unobserved": missing,
        "true_accepts": tp,
        "true_rejects": tn,
        "false_accepts": fa,
        "false_rejects": fr,
        "false_accept_rate": (fa / (fa + tn)) if fa + tn else None,
        "false_reject_rate": (fr / (fr + tp)) if fr + tp else None,
    }


def summarize(paths: list[Path]) -> dict:
    records = []
    seen = set()
    for path in paths:
        for number, line in enumerate(path.read_text().splitlines(), 1):
            if not line.strip():
                continue
            row = json.loads(line)
            required = ("schema_version", "case_id", "run_id", "run_kind", "oracle_accept",
                        "oracle_baseline_accept", "uni_decision", "agent_self_report",
                        "agent_output_sha256", "ci_decision", "oracle_sha256",
                        "baseline_sha256", "candidate_sha256", "oracle_output_sha256")
            if any(k not in row for k in required):
                raise InvalidCase(f"missing required result fields: {path}:{number}")
            if row["schema_version"] != 1 or row["run_kind"] not in KINDS:
                raise InvalidCase(f"unknown result schema or category: {path}:{number}")
            if type(row["oracle_accept"]) is not bool or row["oracle_baseline_accept"] is not False:
                raise InvalidCase("oracle verdict must be a boolean and baseline must fail")
            if row["uni_decision"] not in (DECISIONS | {"UNOBSERVED"}):
                raise InvalidCase("unknown UNI verdict in a recorded result")
            if row["agent_self_report"] not in {"DONE", "FAILED", "UNPARSED", "UNOBSERVED"}:
                raise InvalidCase("unknown agent self-report in a recorded result")
            if row["ci_decision"] not in {"PASS", "FAIL", "UNOBSERVED"}:
                raise InvalidCase("unknown CI verdict in a recorded result")
            if row["run_kind"] == "real-agent" and not row["agent_output_sha256"]:
                raise InvalidCase("real-agent record lacks actual worker output")
            for key_name in ("oracle_sha256", "baseline_sha256", "candidate_sha256"):
                digest = row[key_name]
                if not isinstance(digest, str) or len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
                    raise InvalidCase(f"invalid {key_name}")
            key = (row["run_kind"], row["case_id"], row["run_id"])
            if key in seen:
                raise InvalidCase(f"duplicate run: {key}")
            seen.add(key)
            records.append(row)
    return {
        kind: {
            "total": len(group := [r for r in records if r["run_kind"] == kind]),
            "oracle_accepted": sum(r["oracle_accept"] for r in group),
            "UNI": measure(group, "UNI"),
            "CI": measure(group, "CI"),
            "agent_self_report": measure(group, "agent"),
        }
        for kind in ("scripted", "real-agent")
    }


def main() -> int:
    cli = argparse.ArgumentParser(description=__doc__)
    sub = cli.add_subparsers(dest="cmd", required=True)
    run = sub.add_parser("evaluate", help="run held-out owner oracle on delivered code")
    run.add_argument("--case-id", required=True)
    run.add_argument("--run-id", default="single",
                     help="explicit attempt identifier for repeated model runs")
    run.add_argument("--kind", choices=sorted(KINDS), required=True)
    for flag in ("baseline", "candidate", "oracle"):
        run.add_argument(f"--{flag}", type=Path, required=True)
    run.add_argument("--uni-json", type=Path)
    run.add_argument("--agent-output", type=Path)
    run.add_argument("--ci-decision", choices=["PASS", "FAIL", "UNOBSERVED"],
                     default="UNOBSERVED")
    run.add_argument("--timeout", type=int, default=30)
    run.add_argument("--out", type=Path)
    report = sub.add_parser("summarize", help="calculate grouped outcome confusion matrices")
    report.add_argument("results", nargs="+", type=Path)
    args = cli.parse_args()
    try:
        if args.cmd == "evaluate":
            if not (1 <= args.timeout <= 300):
                raise InvalidCase("timeout must be 1..300 seconds")
            record = evaluate(args)
            out = json.dumps(record, sort_keys=True)
            if args.out:
                args.out.write_text(out + "\n")
            else:
                print(out)
        else:
            print(json.dumps(summarize(args.results), indent=2, sort_keys=True))
        return 0
    except (InvalidCase, OSError, ValueError) as exc:
        print(f"invalid benchmark: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
