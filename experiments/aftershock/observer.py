#!/usr/bin/env python3
"""Read-only Aftershock delivery observer; never treats a heartbeat as a cycle."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import sqlite3
import sys


VALID_VERDICTS = {"REJECT", "REVISE", "WATCH", "TEST"}


def observed(db_file: Path, now: datetime, max_cycle_hours: float = 192) -> dict:
    checks = []
    result = {
        "schema": "uni.aftershock.observation/v1",
        "status": "UNKNOWN",
        "checks": checks,
        "scope": "READ_ONLY_SQLITE_METADATA",
    }

    def add(name, state, detail):
        checks.append({"id": name, "state": state, "detail": detail})

    if not db_file.is_file():
        add("database", "UNKNOWN", "database not available")
        return result

    try:
        # Read-only URI prevents implicit database creation and all writes.
        with sqlite3.connect(db_file.resolve().as_uri() + "?mode=ro", uri=True) as db:
            tables = {row[0] for row in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
            if "cycle" not in tables:
                add("cycle_schema", "UNKNOWN", "cycle table missing")
                return result
            cols = {row[1] for row in db.execute("PRAGMA table_info(cycle)")}
            if not {"id", "vu_le", "sujet", "verdict", "ecrit"} <= cols:
                add("cycle_schema", "UNKNOWN", "cycle columns missing")
                return result
            last = db.execute(
                "SELECT vu_le,sujet,verdict,ecrit FROM cycle ORDER BY id DESC LIMIT 1"
            ).fetchone()
            business = db.execute(
                "SELECT vu_le,sujet,verdict,ecrit FROM cycle "
                "WHERE TRIM(COALESCE(sujet,'')) <> '' ORDER BY id DESC LIMIT 1"
            ).fetchone()
    except (sqlite3.Error, OSError, ValueError) as exc:
        add("database", "UNKNOWN", f"unable to read metadata: {type(exc).__name__}")
        return result

    if not last:
        add("heartbeat", "UNKNOWN", "no cycle records at all")
        add("delivery", "UNKNOWN", "no business cycle records")
        return result

    def age(value):
        stamp = datetime.fromisoformat(str(value).replace("Z", "+00:00"))
        if stamp.tzinfo is None:
            raise ValueError("naive timestamp")
        return (now - stamp.astimezone(timezone.utc)).total_seconds() / 3600

    try:
        beat_age = age(last[0])
        add("heartbeat", "PASS" if 0 <= beat_age <= 48 else "FAIL",
            "latest row is a heartbeat or business cycle; age_hours=" + str(round(beat_age, 2)))
    except (ValueError, TypeError, OverflowError):
        add("heartbeat", "UNKNOWN", "invalid heartbeat date")

    if not business:
        add("delivery", "UNKNOWN", "heartbeat is not a completed research cycle")
    else:
        try:
            cycle_age = age(business[0])
            fresh = 0 <= cycle_age <= max_cycle_hours
            add("business_cycle_fresh", "PASS" if fresh else "FAIL",
                "age_hours=" + str(round(cycle_age, 2)) + ", policy_hours=" + str(max_cycle_hours))
        except (ValueError, TypeError, OverflowError):
            add("business_cycle_fresh", "UNKNOWN", "invalid business cycle timestamp")
        verdict = str(business[2] or "").strip().upper()
        add("critique_verdict", "PASS" if verdict in VALID_VERDICTS else "FAIL",
            f"verdict={verdict or '<empty>'}")
        output = str(business[3] or "")
        add("deposit_recorded", "PASS" if output in {"0", "1"} else "UNKNOWN",
            "database reports ecrit=" + output)
        # Zero output is permissible: REJECT/REVISE can legitimately refrain
        # from publishing. The database alone cannot prove file existence.
        result["delivery"] = {"subject": str(business[1]), "verdict": verdict,
                              "deposit_recorded": output == "1"}
    states = {c["state"] for c in checks}
    result["status"] = "FAIL" if "FAIL" in states else "UNKNOWN" if "UNKNOWN" in states else "PASS"
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", type=Path, required=True)
    parser.add_argument("--at", help="ISO8601 clock, for reproducible tests")
    parser.add_argument("--max-cycle-hours", type=float, default=192)
    args = parser.parse_args()
    if not 0 < args.max_cycle_hours <= 24 * 30:
        parser.error("max-cycle-hours must be between 0 and 720")
    try:
        now = datetime.fromisoformat(args.at.replace("Z", "+00:00")) if args.at else datetime.now(timezone.utc)
        if now.tzinfo is None:
            parser.error("--at requires an explicit timezone")
    except ValueError:
        parser.error("--at must be an ISO8601 date-time with timezone")
    report = observed(args.db, now.astimezone(timezone.utc), args.max_cycle_hours)
    print(json.dumps(report, sort_keys=True))
    return 0 if report["status"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
