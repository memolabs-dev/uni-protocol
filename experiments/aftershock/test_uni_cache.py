"""Regression: cached UNI proof must be invalidated by a changed note."""
from pathlib import Path
import hashlib
import json
import shutil
import sqlite3
import subprocess
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
CLI = HERE.parents[1] / "target/release/uni"


class LiveUniReceiptCacheTests(unittest.TestCase):
    def test_changed_note_invalidates_recent_uni_evidence(self):
        self.assertTrue(CLI.is_file(), f"build UNI release first: {CLI}")
        with tempfile.TemporaryDirectory(prefix="uni-aftershock-cache-") as tmp:
            root = Path(tmp)
            notes = root / "snapshots/notes"
            notes.mkdir(parents=True)
            src = root / "experiments/aftershock"
            src.mkdir(parents=True)
            shutil.copyfile(HERE / "native.py", src / "native.py")
            shutil.copyfile(HERE / "native.uni", src / "native.uni")
            note = notes / "cycle.md"
            critic = {"verdict": "WATCH", "objections": [{"objection": "check source"}]}
            note.write_text(
                "---\nsujet: radar\nmode: radar\ndate_arret: 2026-10-12\n"
                "genere_le: 2026-10-12T12:00:00+00:00\ncritique: WATCH\n"
                "source: aftershock (lab), cycle automatique\n---\n"
                "# Aftershock\n## Brouillon du cycle\n"
                '{"source_url":"https://example.org/a"}\n'
                "## Critique independante\n"
                + json.dumps(critic, ensure_ascii=False, indent=2)
                + "\n## Ce que la critique a reproche\n- check source\n",
                encoding="utf-8",
            )
            dbpath = root / "snapshots/aftershock.db"
            with sqlite3.connect(dbpath) as db:
                db.execute("create table cycle (id integer primary key, sujet text, verdict text, ecrit text)")
                db.execute("create table cycle_evidence (cycle_id integer primary key, payload text)")
                db.execute("insert into cycle values (1, 'radar', 'WATCH', '1')")
                receipt = {
                    "schema": "aftershock.evidence/v2",
                    "cycle_id": 1,
                    "trace_id": "a" * 32,
                    "note_path": "/depot/cycle.md",
                    "note_sha256": hashlib.sha256(note.read_bytes()).hexdigest(),
                    "critic_verdict": "WATCH",
                    "critic_payload": critic,
                    "critic_sha256": hashlib.sha256(
                        json.dumps(critic, sort_keys=True, ensure_ascii=False).encode()
                    ).hexdigest(),
                    "source_urls": ["https://example.org/a"],
                }
                db.execute("insert into cycle_evidence values (?, ?)", (1, json.dumps(receipt)))
            config = root / ".uni"
            config.mkdir()
            (config / "config.toml").write_text(
                '[verifiers."aftershock.native"]\n'
                'run = "python3 experiments/aftershock/native.py --db snapshots/aftershock.db --notes-root snapshots/notes"\n'
                'expect = "\\"status\\": \\"PASS\\""\n'
                'files = ["snapshots/aftershock.db", "snapshots/notes/**", "experiments/aftershock/native.py"]\n'
                'max_age_hours = 1\n'
            )

            def verify():
                p = subprocess.run([str(CLI), "--json", "verify",
                                    "experiments/aftershock/native.uni"],
                                   cwd=root, capture_output=True, text=True, timeout=35)
                self.assertTrue(p.stdout, p.stderr)
                return p.returncode, json.loads(p.stdout)

            rc, first = verify()
            self.assertEqual(rc, 0, first)
            self.assertEqual(first["decision"], "Accepted")
            note.write_text(note.read_text() + "\nCHANGED AFTER RECEIPT")
            rc, second = verify()
            self.assertNotEqual(rc, 0, second)
            self.assertNotEqual(second["decision"], "Accepted")


if __name__ == "__main__":
    unittest.main()
