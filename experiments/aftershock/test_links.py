import importlib.util
import hashlib
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

modpath = Path(__file__).with_name("links.py")
spec = importlib.util.spec_from_file_location("aftershock_links", modpath)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


class LinksTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.db = self.root / "aftershock.db"
        self.notes = self.root / "notes"
        self.notes.mkdir()
        self.manifest = self.root / "links.json"
        with sqlite3.connect(self.db) as db:
            db.execute("CREATE TABLE cycle (id INTEGER PRIMARY KEY, vu_le TEXT, sujet TEXT, verdict TEXT, ecrit TEXT)")
            db.execute("CREATE TABLE dossier (vu_le TEXT, sujet TEXT, verdict TEXT, fait TEXT, rupture TEXT, source TEXT, objections TEXT)")
            db.execute("INSERT INTO cycle VALUES (7, '2026-10-12T14:02:00+00:00','radar','WATCH','1')")
            db.execute("INSERT INTO dossier VALUES ('2026-10-12T14:02:01+00:00','radar','WATCH','f','r','https://example.org/a','[]')")
        self.note = self.notes / "cycle.md"
        self.note.write_text("""---
sujet: radar
mode: radar
date_arret: 2026-10-12
genere_le: 2026-10-12T14:01:00+00:00
critique: WATCH
source: aftershock (lab), cycle automatique
---
# AFTERSHOCK : radar
## Brouillon du cycle

{"source_url": "https://example.org/a"}

## Critique independante

{"verdict": "WATCH", "raisons": "critique lisible et utile"}

## Ce que la critique a reproche

- Chiffre a verifier
""")
        self.mapping = {"schema": "uni.aftershock.links/v1", "all_cycles": True,
                        "cycles": [{"cycle_id": 7, "note": "cycle.md",
                                    "sha256": hashlib.sha256(self.note.read_bytes()).hexdigest(),
                                    "source_urls": ["https://example.org/a"],
                                    "critic_verdict": "WATCH",
                                    "dossier_vu_le": "2026-10-12T14:02:01+00:00"}]}
        self.write()

    def write(self):
        self.manifest.write_text(json.dumps(self.mapping))

    def check(self):
        self.write()
        return mod.verify(self.db, self.notes, self.manifest)

    def kinds(self):
        return {x["kind"] for x in self.check()["findings"]}

    def test_complete_proof_of_linkage_passes(self):
        self.assertEqual(self.check()["status"], "PASS")

    def test_changed_note_is_rejected_by_digest(self):
        self.note.write_text(self.note.read_text() + "\nchanged")
        self.assertIn("note_digest_mismatch", self.kinds())

    def test_missing_note_is_not_reconstructed_from_subject(self):
        self.mapping["cycles"][0]["note"] = "other.md"
        self.assertIn("missing_note", self.kinds())

    def test_ambiguous_dossier_is_rejected(self):
        with sqlite3.connect(self.db) as db:
            db.execute("INSERT INTO dossier SELECT * FROM dossier")
        self.assertIn("dossier_link_ambiguous", self.kinds())

    def test_source_does_not_match_note(self):
        self.mapping["cycles"][0]["source_urls"] = ["https://example.org/other"]
        self.assertIn("source_not_in_note", self.kinds())

    def test_no_critic_section(self):
        self.note.write_text(self.note.read_text().replace("## Critique independante", "## Lost critique"))
        self.mapping["cycles"][0]["sha256"] = hashlib.sha256(self.note.read_bytes()).hexdigest()
        self.assertIn("missing_critic_section", self.kinds())

    def test_note_subject_mismatch(self):
        self.note.write_text(self.note.read_text().replace("sujet: radar", "sujet: foreign"))
        self.mapping["cycles"][0]["sha256"] = hashlib.sha256(self.note.read_bytes()).hexdigest()
        self.assertIn("note_identity_mismatch", self.kinds())

    def test_cycle_omission_detected_when_complete_scope_declared(self):
        with sqlite3.connect(self.db) as db:
            db.execute("INSERT INTO cycle VALUES (8,'2026-10-12T15:00:00+00:00','other','REJECT','0')")
        self.assertIn("cycles_omitted", self.kinds())

    def test_undeclared_links_are_not_guessed(self):
        self.mapping["cycles"][0].pop("dossier_vu_le")
        self.assertIn("dossier_unlinked", self.kinds())

    def test_unpublished_reject_does_not_need_note(self):
        with sqlite3.connect(self.db) as db:
            db.execute("UPDATE cycle SET verdict='REJECT',ecrit='0' WHERE id=7")
            db.execute("UPDATE dossier SET verdict='REJECT'")
        self.mapping["cycles"][0] = {"cycle_id":7,"note":None,"source_urls":[],
                                     "critic_verdict":"REJECT",
                                     "dossier_vu_le":"2026-10-12T14:02:01+00:00"}
        self.assertEqual(self.check()["status"], "PASS")


if __name__ == "__main__":
    unittest.main()
