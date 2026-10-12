#!/usr/bin/env python3
"""Read-only validation of Aftershock's native cycle_evidence rows."""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3

PUBLISHED = {"WATCH", "TEST"}
NON_PUBLISHED = {"REJECT", "REVISE"}


def verify(db_file, notes_root):
    report = {"schema": "uni.aftershock.native/v1", "status": "UNKNOWN",
              "scope": "CYCLE_EVIDENCE_INTEGRITY_NOT_FACTUAL_TRUTH", "cycles": []}
    if not Path(db_file).is_file():
        return report
    try:
        with sqlite3.connect(Path(db_file).resolve().as_uri() + "?mode=ro", uri=True) as db:
            rows = db.execute(
                "SELECT id, sujet, verdict, ecrit FROM cycle "
                "WHERE TRIM(COALESCE(sujet,''))<>'' ORDER BY id"
            ).fetchall()
            evidence = dict(db.execute("SELECT cycle_id,payload FROM cycle_evidence").fetchall())
    except sqlite3.Error:
        return report
    if not rows:
        return report
    root = Path(notes_root).resolve()
    has_problem = False
    for ident, subject, verdict, written in rows:
        entry = {"cycle_id": ident, "status": "UNKNOWN", "issues": []}
        report["cycles"].append(entry)
        if ident not in evidence:
            entry["issues"].append("MISSING_NATIVE_EVIDENCE")
            has_problem = True
            continue
        try:
            item = json.loads(evidence[ident])
        except (TypeError, ValueError):
            entry["issues"].append("INVALID_EVIDENCE_RECORD")
            has_problem = True
            entry["status"] = "FAIL"
            continue
        if not isinstance(item, dict) or item.get("schema") != "aftershock.evidence/v1" or item.get("cycle_id") != ident:
            entry["issues"].append("EVIDENCE_IDENTITY_MISMATCH")
        if item.get("critic_verdict") != verdict or verdict not in PUBLISHED | NON_PUBLISHED:
            entry["issues"].append("CRITIC_VERDICT_MISMATCH")
        if not isinstance(item.get("critic_sha256"), str) or len(item["critic_sha256"]) != 64:
            entry["issues"].append("CRITIC_EVIDENCE_MISSING")
        if verdict in PUBLISHED and written == "1":
            path = item.get("note_path")
            if not isinstance(path, str):
                entry["issues"].append("NOTE_LINK_MISSING")
            else:
                note = Path(path)
                if not note.is_absolute():
                    note = root / note
                resolved = note.resolve()
                if not resolved.is_relative_to(root) or not resolved.is_file() or resolved.is_symlink():
                    entry["issues"].append("NOTE_MISSING_OR_UNSAFE")
                else:
                    if hashlib.sha256(resolved.read_bytes()).hexdigest() != item.get("note_sha256"):
                        entry["issues"].append("NOTE_DIGEST_MISMATCH")
            urls = item.get("source_urls")
            if not isinstance(urls, list) or not urls or any(not isinstance(x, str) or not x.startswith("https://") for x in urls):
                entry["issues"].append("SOURCE_POINTER_MISSING")
        elif verdict in NON_PUBLISHED and written == "0":
            if item.get("note_path") is not None:
                entry["issues"].append("UNEXPECTED_NOTE")
        else:
            entry["issues"].append("DEPOSIT_POLICY_MISMATCH")
        entry["status"] = "FAIL" if entry["issues"] else "PASS"
        has_problem |= bool(entry["issues"])
    report["status"] = "FAIL" if has_problem else "PASS"
    return report


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--db", required=True)
    p.add_argument("--notes-root", required=True)
    args = p.parse_args()
    result = verify(args.db, args.notes_root)
    print(json.dumps(result, ensure_ascii=False, sort_keys=True))
    return 0 if result["status"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
