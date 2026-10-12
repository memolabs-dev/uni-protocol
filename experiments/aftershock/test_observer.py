import importlib.util
import sqlite3
from datetime import datetime, timezone
from pathlib import Path
import tempfile
import unittest

SOURCE = Path(__file__).with_name("observer.py")
spec = importlib.util.spec_from_file_location("aftershock_observer", SOURCE)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
NOW = datetime(2026, 10, 12, 12, tzinfo=timezone.utc)


class ObserverTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.db = Path(self.temp.name) / "cycle.db"
        with sqlite3.connect(self.db) as db:
            db.execute("create table cycle (id integer primary key, vu_le text, sujet text, verdict text, ecrit text)")

    def add(self, when, subject="", verdict="", deposit="0"):
        with sqlite3.connect(self.db) as db:
            db.execute("insert into cycle (vu_le,sujet,verdict,ecrit) values (?,?,?,?)",
                       (when, subject, verdict, deposit))

    def report(self):
        return module.observed(self.db, NOW)

    def test_fresh_heartbeat_does_not_qualify_as_a_delivery(self):
        self.add("2026-10-12T11:00:00+00:00")
        report = self.report()
        self.assertEqual(report["status"], "UNKNOWN")
        self.assertEqual(report["checks"][0]["state"], "PASS")
        self.assertEqual(report["checks"][1]["state"], "UNKNOWN")

    def test_a_business_cycle_with_a_critique_is_reported(self):
        self.add("2026-10-08T12:00:00+00:00", "research", "WATCH", "1")
        self.add("2026-10-12T11:00:00+00:00")
        report = self.report()
        self.assertEqual(report["status"], "PASS")
        self.assertTrue(report["delivery"]["deposit_recorded"])

    def test_weekly_cadence_is_not_misjudged_against_48h(self):
        self.add("2026-10-07T12:00:00+00:00", "research", "REJECT", "0")
        self.add("2026-10-12T11:00:00+00:00")
        self.assertEqual(self.report()["status"], "PASS")

    def test_old_business_cycle_stays_fail_despite_fresh_heartbeat(self):
        self.add("2026-09-10T12:00:00+00:00", "research", "TEST", "1")
        self.add("2026-10-12T11:00:00+00:00")
        self.assertEqual(self.report()["status"], "FAIL")

    def test_business_without_critique_cannot_pass(self):
        self.add("2026-10-12T10:00:00+00:00", "research", "", "1")
        self.assertEqual(self.report()["status"], "FAIL")

    def test_missing_database_does_not_create_a_file(self):
        absent = self.db.parent / "absent.db"
        self.assertEqual(module.observed(absent, NOW)["status"], "UNKNOWN")
        self.assertFalse(absent.exists())

    def test_missing_schema_is_unknown_and_not_created(self):
        with sqlite3.connect(self.db) as db:
            db.execute("drop table cycle")
        self.assertEqual(self.report()["status"], "UNKNOWN")
        with sqlite3.connect(self.db) as db:
            self.assertEqual(db.execute("select count(*) from sqlite_master where name='cycle'").fetchone()[0], 0)

    def test_invalid_timestamp_is_unknown_not_recent(self):
        self.add("unparseable", "research", "TEST", "1")
        self.assertEqual(self.report()["status"], "UNKNOWN")

    def test_future_heartbeat_is_not_healthy(self):
        self.add("2026-10-20T12:00:00+00:00", "research", "TEST", "1")
        self.assertEqual(self.report()["status"], "FAIL")


if __name__ == "__main__":
    unittest.main()
