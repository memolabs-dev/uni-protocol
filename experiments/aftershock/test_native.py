import hashlib
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
import importlib.util

path = Path(__file__).with_name("native.py")
spec = importlib.util.spec_from_file_location("aftershock_native", path)
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)


class NativeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.note = self.root / "cycle.md"
        self.note.write_text("research result", encoding="utf-8")
        self.db = self.root / "aftershock.db"
        self.item = {"schema": "aftershock.evidence/v1", "cycle_id": 1,
                     "note_path": str(self.note),
                     "note_sha256": hashlib.sha256(self.note.read_bytes()).hexdigest(),
                     "critic_verdict": "WATCH",
                     "critic_payload": {"verdict": "WATCH"},
                     "critic_sha256": hashlib.sha256(json.dumps({"verdict": "WATCH"}, sort_keys=True, ensure_ascii=False).encode()).hexdigest(),
                     "source_urls": ["https://example.org/a"]}
        with sqlite3.connect(self.db) as db:
            db.execute("create table cycle (id integer primary key, sujet text, verdict text, ecrit text)")
            db.execute("create table cycle_evidence (cycle_id integer primary key, payload text)")
            db.execute("insert into cycle values (1,'radar','WATCH','1')")
        self.write()

    def write(self):
        with sqlite3.connect(self.db) as db:
            db.execute("insert or replace into cycle_evidence values (?,?)", (1, json.dumps(self.item)))

    def result(self):
        return native.verify(self.db, self.root)

    def test_matching_cycle_passes(self):
        self.assertEqual(self.result()["status"], "PASS")

    def test_changed_note_fails(self):
        self.note.write_text("changed")
        self.assertEqual(self.result()["cycles"][0]["issues"], ["NOTE_DIGEST_MISMATCH"])

    def test_critic_missing_fails(self):
        self.item["critic_payload"] = None
        self.write()
        self.assertIn("CRITIC_EVIDENCE_MISSING", self.result()["cycles"][0]["issues"])

    def test_tampered_critic_content_fails(self):
        self.item["critic_payload"]["verdict"] = "REJECT"
        self.write()
        issues = self.result()["cycles"][0]["issues"]
        self.assertIn("CRITIC_DIGEST_MISMATCH", issues)
        self.assertIn("CRITIC_CONTENT_VERDICT_MISMATCH", issues)

    def test_missing_record_is_unknown_not_pass(self):
        with sqlite3.connect(self.db) as db:
            db.execute("delete from cycle_evidence")
        self.assertEqual(self.result()["cycles"][0]["status"], "UNKNOWN")
        self.assertEqual(self.result()["status"], "FAIL")

    def test_rejected_unpublished_cycle_passes(self):
        with sqlite3.connect(self.db) as db:
            db.execute("update cycle set verdict='REJECT',ecrit='0'")
        self.item.update({"critic_verdict": "REJECT", "note_path": None, "note_sha256": None,
                          "critic_payload": {"verdict": "REJECT"}})
        self.item["critic_sha256"] = hashlib.sha256(json.dumps(self.item["critic_payload"], sort_keys=True, ensure_ascii=False).encode()).hexdigest()
        self.write()
        self.assertEqual(self.result()["status"], "PASS")

    def test_legacy_rejected_but_published_fails(self):
        with sqlite3.connect(self.db) as db:
            db.execute("update cycle set verdict='REJECT'")
        self.item["critic_verdict"] = "REJECT"
        self.write()
        self.assertIn("DEPOSIT_POLICY_MISMATCH", self.result()["cycles"][0]["issues"])


if __name__ == "__main__":
    unittest.main()
