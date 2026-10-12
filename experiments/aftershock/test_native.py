import hashlib
import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

path = Path(__file__).with_name("native.py")
spec = importlib.util.spec_from_file_location("aftershock_native", path)
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)


class NativeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.notes = self.root / "notes"
        self.notes.mkdir()
        self.note = self.notes / "cycle.md"
        self.critic = {"verdict": "WATCH", "objections": [{"objection": "check data"}]}
        self.note.write_text(self.note_text(), encoding="utf-8")
        self.db = self.root / "aftershock.db"
        with sqlite3.connect(self.db) as db:
            db.execute("create table cycle (id integer primary key, sujet text, verdict text, ecrit text)")
            db.execute("create table cycle_evidence (cycle_id integer primary key, payload text)")
            db.execute("insert into cycle values (1,'radar','WATCH','1')")
        self.item = {"schema": "aftershock.evidence/v2", "cycle_id": 1,
                     "trace_id": "0123456789abcdef" * 2,
                     "note_path": "/depot/cycle.md",
                     "note_sha256": hashlib.sha256(self.note.read_bytes()).hexdigest(),
                     "critic_verdict": "WATCH",
                     "critic_payload": self.critic,
                     "critic_sha256": self.critic_digest(),
                     "source_urls": ["https://example.org/a"]}
        self.write()

    def note_text(self):
        return (
            "---\nsujet: radar\nmode: radar\ndate_arret: 2026-10-12\n"
            "genere_le: 2026-10-12T12:00:00+00:00\ncritique: WATCH\n"
            "source: aftershock (lab), cycle automatique\n---\n"
            "# AFTERSHOCK\n## Brouillon du cycle\n"
            '{"source_url": "https://example.org/a"}\n'
            "## Critique independante\n"
            + json.dumps(self.critic, ensure_ascii=False, indent=2)
            + "\n## Ce que la critique a reproche\n- check data\n"
        )

    def critic_digest(self):
        return hashlib.sha256(json.dumps(self.critic, sort_keys=True, ensure_ascii=False).encode()).hexdigest()

    def write(self):
        with sqlite3.connect(self.db) as db:
            db.execute("insert or replace into cycle_evidence values (?,?)",
                       (1, json.dumps(self.item)))

    def result(self):
        return native.verify(self.db, self.notes)

    def issues(self):
        self.write()
        return self.result()["cycles"][0]["issues"]

    def test_v2_valid_note_trace_source_and_critic_pass(self):
        report = self.result()
        self.assertEqual(report["status"], "PASS")
        self.assertEqual(report["coverage"], {"verified": 1, "total": 1})

    def test_changed_note_fails(self):
        self.note.write_text(self.note.read_text() + "changed")
        self.assertIn("NOTE_DIGEST_MISMATCH", self.issues())

    def test_critic_tampering_fails(self):
        self.item["critic_payload"]["verdict"] = "REJECT"
        self.assertIn("CRITIC_CONTENT_VERDICT_MISMATCH", self.issues())
        self.assertIn("CRITIC_DIGEST_MISMATCH", self.issues())

    def test_note_critic_mismatch_even_with_rehashed_note(self):
        self.note.write_text(self.note.read_text().replace("check data", "fake data"))
        self.item["note_sha256"] = hashlib.sha256(self.note.read_bytes()).hexdigest()
        self.assertIn("CRITIC_NOTE_MISMATCH", self.issues())

    def test_missing_trace_is_not_v2_complete(self):
        self.item["trace_id"] = None
        self.assertIn("TRACE_ID_MISSING_OR_INVALID", self.issues())

    def test_legacy_v1_is_not_silently_accepted(self):
        self.item["schema"] = "aftershock.evidence/v1"
        self.assertIn("LEGACY_OR_UNSUPPORTED_RECEIPT", self.issues())

    def test_source_not_in_note_fails(self):
        self.item["source_urls"] = ["https://example.org/other"]
        self.assertIn("SOURCE_NOT_IN_NOTE", self.issues())

    def test_symlink_escape_fails(self):
        self.item["note_path"] = "/depot/../secret.md"
        self.assertIn("NOTE_MISSING_OR_UNSAFE", self.issues())

    def test_missing_record_is_unknown_not_pass(self):
        with sqlite3.connect(self.db) as db:
            db.execute("delete from cycle_evidence")
        self.assertEqual(self.result()["cycles"][0]["status"], "UNKNOWN")
        self.assertEqual(self.result()["status"], "FAIL")

    def test_rejected_without_note_passes(self):
        with sqlite3.connect(self.db) as db:
            db.execute("update cycle set verdict='REJECT', ecrit='0'")
        self.critic = {"verdict": "REJECT"}
        self.item.update({"critic_verdict": "REJECT", "note_path": None,
                          "note_sha256": None, "critic_payload": self.critic,
                          "critic_sha256": self.critic_digest(), "source_urls": []})
        self.write()
        self.assertEqual(self.result()["status"], "PASS")

    def test_missing_database_unknown(self):
        self.assertEqual(native.verify(self.root / "missing.db", self.notes)["status"], "UNKNOWN")
        self.assertFalse((self.root / "missing.db").exists())


if __name__ == "__main__":
    unittest.main()
