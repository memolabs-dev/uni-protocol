#!/usr/bin/env python3
"""Verify explicit Aftershock cycle->note->source->critic links, without guessing."""
import argparse
from datetime import datetime
import hashlib
import json
from pathlib import Path
import sqlite3
import sys
from urllib.parse import urlsplit

VALID = {"REJECT", "REVISE", "WATCH", "TEST"}
PUBLISHED = {"WATCH", "TEST"}


def verify(db_path, notes_root, manifest_path):
    findings = []
    checked = []
    def issue(cycle_id, kind, description):
        findings.append({"cycle_id": cycle_id, "kind": kind, "detail": description})
    try:
        manifest = json.loads(Path(manifest_path).read_text(encoding="utf-8"))
        if manifest.get("schema") != "uni.aftershock.links/v1" or not isinstance(manifest.get("cycles"), list):
            raise ValueError("bad manifest schema")
        rows = manifest["cycles"]
        if not rows:
            raise ValueError("empty manifest")
        with sqlite3.connect(Path(db_path).resolve().as_uri() + "?mode=ro", uri=True) as db:
            db_rows = {
                row[0]: row for row in
                db.execute("SELECT id,vu_le,sujet,verdict,ecrit FROM cycle WHERE TRIM(COALESCE(sujet,'')) <> ''")
            }
            dossier_available = "dossier" in {r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
            dossier = db.execute("SELECT vu_le,sujet,verdict,source,objections FROM dossier").fetchall() if dossier_available else []
    except (OSError, ValueError, KeyError, sqlite3.Error) as exc:
        return {"status": "UNKNOWN", "findings": [{"kind": "input_unavailable", "detail": type(exc).__name__}], "checked": []}

    seen = set()
    root = Path(notes_root).resolve()
    for item in rows:
        if not isinstance(item, dict):
            issue(None, "invalid_mapping", "mapping is not an object")
            continue
        if set(item) - {"cycle_id", "note", "sha256", "source_urls", "critic_verdict", "dossier_vu_le"}:
            issue(item.get("cycle_id"), "unknown_mapping_fields", "unexpected fields in mapping")
        ident = item.get("cycle_id")
        if type(ident) is not int or ident <= 0 or ident in seen:
            issue(ident, "duplicate_or_invalid_id", "cycle_id must be unique positive integer")
            continue
        seen.add(ident)
        row = db_rows.get(ident)
        if row is None:
            issue(ident, "missing_business_cycle", "cycle ID not present in source database")
            continue
        _, when, subject, verdict, written = row
        verdict = (verdict or "").strip()
        checked.append(ident)
        if verdict not in VALID:
            issue(ident, "invalid_verdict", "unrecognized critic verdict")
        if item.get("critic_verdict") != verdict:
            issue(ident, "verdict_mismatch", "link manifest differs from database critic verdict")
        if written not in {"0", "1"}:
            issue(ident, "unknown_deposit", "deposit flag not understood")
        if written == "1" and verdict not in PUBLISHED:
            issue(ident, "unexpected_deposit", "this verdict should not publish a note")
        note = item.get("note")
        if written == "1":
            if not isinstance(note, str) or not note or Path(note).is_absolute() or ".." in Path(note).parts:
                issue(ident, "missing_note_link", "published cycle requires a safe explicit relative note path")
                continue
            path = root / note
            if path.is_symlink() or not path.is_file() or not path.resolve().is_relative_to(root):
                issue(ident, "missing_note", "note missing, unsafe or outside notes root")
                continue
            raw = path.read_bytes()
            actual = hashlib.sha256(raw).hexdigest()
            if item.get("sha256") != actual:
                issue(ident, "note_digest_mismatch", "note does not match explicit SHA256")
            text = raw.decode("utf-8", errors="replace")
            front = text.split("---", 2)
            if len(front) < 3:
                issue(ident, "note_metadata_missing", "note lacks Aftershock frontmatter")
            else:
                attrs = dict(line.split(": ", 1) for line in front[1].splitlines() if ": " in line)
                if attrs.get("sujet") != subject or attrs.get("critique") != verdict:
                    issue(ident, "note_identity_mismatch", "subject or critic verdict conflicts with database")
                if not attrs.get("date_arret") or not attrs.get("genere_le"):
                    issue(ident, "note_dates_missing", "stop/generated dates absent")
                try:
                    generated = datetime.fromisoformat(attrs["genere_le"].replace("Z","+00:00"))
                    cycle_time = datetime.fromisoformat(when.replace("Z","+00:00"))
                    if generated.tzinfo is None or cycle_time.tzinfo is None or generated > cycle_time:
                        issue(ident, "note_timing_mismatch", "note timestamp is missing TZ or later than cycle")
                except (ValueError, TypeError, KeyError):
                    issue(ident, "note_timing_unknown", "invalid timestamps")
            for marker in ("## Brouillon du cycle", "## Critique independante", "## Ce que la critique a reproche"):
                if marker not in text:
                    issue(ident, "missing_critic_section", f"missing {marker}")
            critique = text.split("## Critique independante", 1)[-1].split("## Ce que la critique a reproche", 1)[0].strip()
            if len(critique) < 12 or "(la critique n'a pas pu etre lue)" in critique:
                issue(ident, "empty_critic", "critic text is empty or placeholder")
            urls = item.get("source_urls")
            if not isinstance(urls, list) or not urls or len(set(map(str, urls))) != len(urls):
                issue(ident, "sources_unlinked", "published note requires explicit unique source URLs")
            else:
                for url in urls:
                    if not isinstance(url, str) or urlsplit(url).scheme != "https" or not urlsplit(url).hostname:
                        issue(ident, "invalid_source_url", "source URL must be a valid HTTPS URL")
                    elif url not in text:
                        issue(ident, "source_not_in_note", "declared source URL absent from the note")
        elif note is not None or item.get("sha256") is not None:
            issue(ident, "unexpected_note", "cycle without a deposit must not claim a published note")
        # Dossier has no foreign key to cycle. Timestamp/subject alone is NOT
        # sufficient: require an explicitly named unique dossier row.
        ref = item.get("dossier_vu_le")
        if ref:
            matches = [d for d in dossier if d[0] == ref and d[1] == subject and d[2] == verdict]
            if len(matches) != 1:
                issue(ident, "dossier_link_ambiguous", "no uniquely matching dossier record")
            elif written == "1":
                source = matches[0][3]
                if source and source not in item.get("source_urls", []):
                    issue(ident, "dossier_source_mismatch", "dossier source missing from explicit source list")
        else:
            issue(ident, "dossier_unlinked", "no explicit dossier record association")
    # A manifest may be a deliberate subset, but should never claim "all cycles"
    # unless its IDs actually cover every business cycle.
    if manifest.get("all_cycles") is True and seen != set(db_rows):
        issue(None, "cycles_omitted", "all_cycles asserted but some business cycle IDs are absent")
    return {"status": "FAIL" if findings else "PASS", "checked": checked, "findings": findings,
            "scope": "EXPLICIT_LINKS_ONLY_NOT_SOURCE_TRUTH"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", required=True)
    parser.add_argument("--notes-root", required=True)
    parser.add_argument("--manifest", required=True)
    args = parser.parse_args()
    result = verify(args.db, args.notes_root, args.manifest)
    print(json.dumps(result, sort_keys=True))
    return 0 if result["status"] == "PASS" else 2


if __name__ == "__main__":
    sys.exit(main())
