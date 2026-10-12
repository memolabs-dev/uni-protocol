#!/usr/bin/env python3
"""Read-only validation of Aftershock native evidence/v2 receipts.

A PASS proves internal integrity of the *observed* receipt, not source truth,
agent authorization or delivery to Langfuse.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sqlite3
from urllib.parse import urlsplit

PUBLISHED = {"WATCH", "TEST"}
NON_PUBLISHED = {"REJECT", "REVISE"}
TRACE = re.compile(r"^[0-9a-f]{32}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")


def _note(root, path):
    """Translate only an established /depot path to a detached snapshot."""
    if not isinstance(path, str) or not path:
        return None
    original = Path(path)
    if original.is_absolute():
        if original.resolve().is_relative_to(root):
            candidate = original
        elif original == Path("/depot") or original.is_relative_to(Path("/depot")):
            candidate = root / original.relative_to(Path("/depot"))
        else:
            return None
    else:
        candidate = root / original
    if candidate.is_symlink() or not candidate.resolve().is_relative_to(root):
        return None
    return candidate if candidate.is_file() else None


def _note_checks(text, subject, verdict, critic, urls):
    issues = []
    if not text.startswith("---\n") or "\n---\n" not in text[4:]:
        return ["NOTE_METADATA_MISSING"]
    header = text.split("\n---\n", 1)[0].removeprefix("---\n")
    fields = dict(line.split(": ", 1) for line in header.splitlines() if ": " in line)
    if fields.get("sujet") != subject or fields.get("critique") != verdict:
        issues.append("NOTE_IDENTITY_MISMATCH")
    if not fields.get("date_arret") or not fields.get("genere_le"):
        issues.append("NOTE_DATES_MISSING")
    if "## Critique independante" not in text or "## Ce que la critique a reproche" not in text:
        issues.append("CRITIC_SECTION_MISSING")
    else:
        section = text.split("## Critique independante", 1)[1].split(
            "## Ce que la critique a reproche", 1)[0].strip()
        try:
            parsed = json.loads(section)
            if parsed != critic:
                issues.append("CRITIC_NOTE_MISMATCH")
        except (ValueError, TypeError):
            issues.append("CRITIC_NOTE_INVALID_JSON")
    for source in urls:
        if source not in text:
            issues.append("SOURCE_NOT_IN_NOTE")
            break
    return issues


def verify(db_file, notes_root):
    report = {"schema": "uni.aftershock.native/v2", "status": "UNKNOWN",
              "scope": "NATIVE_RECEIPT_INTEGRITY_NOT_FACTUAL_TRUTH",
              "cycles": [], "coverage": {"verified": 0, "total": 0}}
    if not Path(db_file).is_file():
        return report
    try:
        # Read-only URI prevents creating the SQLite file.
        with sqlite3.connect(Path(db_file).resolve().as_uri() + "?mode=ro", uri=True) as db:
            rows = db.execute(
                "SELECT id,sujet,verdict,ecrit FROM cycle "
                "WHERE TRIM(COALESCE(sujet,''))<>'' ORDER BY id"
            ).fetchall()
            evidence = dict(db.execute("SELECT cycle_id,payload FROM cycle_evidence").fetchall())
    except (sqlite3.Error, OSError):
        return report
    if not rows:
        return report
    root = Path(notes_root).resolve()
    for ident, subject, verdict, written in rows:
        entry = {"cycle_id": ident, "status": "UNKNOWN", "issues": []}
        report["cycles"].append(entry)
        issues = entry["issues"]
        if ident not in evidence:
            issues.append("MISSING_NATIVE_EVIDENCE")
            continue
        try:
            item = json.loads(evidence[ident])
        except (TypeError, ValueError):
            issues.append("INVALID_EVIDENCE_RECORD")
            entry["status"] = "FAIL"
            continue
        if not isinstance(item, dict):
            issues.append("INVALID_EVIDENCE_RECORD")
            entry["status"] = "FAIL"
            continue
        if item.get("schema") != "aftershock.evidence/v2":
            issues.append("LEGACY_OR_UNSUPPORTED_RECEIPT")
        if item.get("cycle_id") != ident:
            issues.append("EVIDENCE_IDENTITY_MISMATCH")
        if not isinstance(item.get("trace_id"), str) or not TRACE.fullmatch(item["trace_id"]):
            issues.append("TRACE_ID_MISSING_OR_INVALID")
        if item.get("critic_verdict") != verdict or verdict not in PUBLISHED | NON_PUBLISHED:
            issues.append("CRITIC_VERDICT_MISMATCH")
        critic = item.get("critic_payload")
        if not isinstance(critic, dict) or not critic:
            issues.append("CRITIC_EVIDENCE_MISSING")
        else:
            digest = hashlib.sha256(json.dumps(critic, sort_keys=True, ensure_ascii=False).encode()).hexdigest()
            if digest != item.get("critic_sha256"):
                issues.append("CRITIC_DIGEST_MISMATCH")
            if critic.get("verdict") != verdict:
                issues.append("CRITIC_CONTENT_VERDICT_MISMATCH")
        urls = item.get("source_urls")
        if not isinstance(urls, list) or any(
            not isinstance(u, str) or urlsplit(u).scheme != "https" or not urlsplit(u).hostname
            for u in urls
        ):
            issues.append("SOURCE_POINTER_INVALID")
            urls = []
        if verdict in PUBLISHED and written == "1":
            note = _note(root, item.get("note_path"))
            if note is None:
                issues.append("NOTE_MISSING_OR_UNSAFE")
            else:
                raw = note.read_bytes()
                if not isinstance(item.get("note_sha256"), str) or not HEX64.fullmatch(item["note_sha256"]):
                    issues.append("NOTE_DIGEST_MISSING")
                elif hashlib.sha256(raw).hexdigest() != item["note_sha256"]:
                    issues.append("NOTE_DIGEST_MISMATCH")
                issues.extend(_note_checks(raw.decode("utf-8", "replace"), subject, verdict, critic, urls))
            if not urls:
                issues.append("SOURCE_POINTER_MISSING")
        elif verdict in NON_PUBLISHED and written == "0":
            if item.get("note_path") is not None or item.get("note_sha256") is not None:
                issues.append("UNEXPECTED_NOTE")
        else:
            issues.append("DEPOSIT_POLICY_MISMATCH")
        entry["status"] = "FAIL" if issues else "PASS"
    report["coverage"]["total"] = len(rows)
    report["coverage"]["verified"] = sum(c["status"] == "PASS" for c in report["cycles"])
    report["status"] = "PASS" if report["coverage"]["verified"] == len(rows) else "FAIL"
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", required=True)
    parser.add_argument("--notes-root", required=True)
    args = parser.parse_args()
    result = verify(args.db, args.notes_root)
    print(json.dumps(result, ensure_ascii=False, sort_keys=True))
    return 0 if result["status"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
