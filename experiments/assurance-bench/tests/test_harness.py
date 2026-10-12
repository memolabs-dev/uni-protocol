import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "harness.py"
FIX = ROOT / "fixtures"
spec = importlib.util.spec_from_file_location("uni_benchmark_harness", SCRIPT)
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class IndependentOracleTests(unittest.TestCase):
    def _run(self, candidate: str, *extra: str):
        return subprocess.run([
            sys.executable, str(SCRIPT), "evaluate",
            "--case-id", f"ledger-{candidate}", "--kind", "scripted",
            "--baseline", str(FIX / "baseline"),
            "--candidate", str(FIX / candidate),
            "--oracle", str(FIX / "owner-oracle"),
            *extra,
        ], capture_output=True, text=True)

    def test_oracle_separates_correct_and_plausible_wrong_fix(self):
        bad = self._run("plausible-wrong")
        good = self._run("correct")
        self.assertEqual(bad.returncode, 0, bad.stderr)
        self.assertEqual(good.returncode, 0, good.stderr)
        wrong_record = json.loads(bad.stdout)
        correct_record = json.loads(good.stdout)
        self.assertFalse(wrong_record["oracle_accept"])
        self.assertTrue(correct_record["oracle_accept"])
        self.assertEqual(wrong_record["uni_decision"], "UNOBSERVED")
        self.assertEqual(wrong_record["agent_self_report"], "UNOBSERVED")
        self.assertEqual(wrong_record["run_kind"], "scripted")
        self.assertEqual(len(wrong_record["oracle_sha256"]), 64)
        self.assertEqual(wrong_record["run_id"], "single")

    def test_preflight_rejects_already_fixed_baseline(self):
        with tempfile.TemporaryDirectory() as dirname:
            other = Path(dirname)
            # baseline=correct; candidate=plausible-wrong, oracle outside both.
            result = subprocess.run([
                sys.executable, str(SCRIPT), "evaluate",
                "--case-id", "already-green", "--kind", "scripted",
                "--baseline", str(FIX / "correct"),
                "--candidate", str(FIX / "plausible-wrong"),
                "--oracle", str(FIX / "owner-oracle"),
            ], capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn("already passes", result.stderr)

    def test_oracle_cannot_be_inside_candidate(self):
        with self.assertRaises(bench.InvalidCase):
            bench.check_isolation(
                FIX / "baseline", FIX / "correct", FIX / "correct"
            )

    def test_real_agent_requires_worker_output(self):
        result = subprocess.run([
            sys.executable, str(SCRIPT), "evaluate", "--case-id", "no-worker",
            "--kind", "real-agent", "--baseline", str(FIX / "baseline"),
            "--candidate", str(FIX / "correct"),
            "--oracle", str(FIX / "owner-oracle")
        ], capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("actual output", result.stderr)

    def test_summarize_does_not_invent_unobserved_results(self):
        wrong = json.loads(self._run("plausible-wrong").stdout)
        correct = json.loads(self._run("correct").stdout)
        stats = bench.summarize([])["scripted"]
        self.assertEqual(stats["total"], 0)
        with tempfile.TemporaryDirectory() as dirname:
            p = Path(dirname) / "results.jsonl"
            p.write_text(json.dumps(wrong) + "\n" + json.dumps(correct) + "\n")
            stats = bench.summarize([p])["scripted"]
        self.assertEqual(stats["total"], 2)
        self.assertEqual(stats["oracle_accepted"], 1)
        self.assertEqual(stats["UNI"]["unobserved"], 2)
        self.assertEqual(stats["UNI"]["observed"], 0)
        self.assertIsNone(stats["UNI"]["false_accept_rate"])

    def test_summarize_rejects_untrusted_claimed_verdicts(self):
        wrong = json.loads(self._run("plausible-wrong").stdout)
        wrong["uni_decision"] = "MAGIC"
        with tempfile.TemporaryDirectory() as dirname:
            path = Path(dirname) / "invalid.jsonl"
            path.write_text(json.dumps(wrong) + "\n")
            with self.assertRaises(bench.InvalidCase):
                bench.summarize([path])

    def test_attempt_id_allows_repeated_runs_without_overwriting(self):
        wrong = json.loads(self._run("plausible-wrong").stdout)
        again = dict(wrong)
        again["run_id"] = "second"
        with tempfile.TemporaryDirectory() as dirname:
            path = Path(dirname) / "attempts.jsonl"
            path.write_text(json.dumps(wrong) + "\n" + json.dumps(again) + "\n")
            self.assertEqual(bench.summarize([path])["scripted"]["total"], 2)

    def test_report_parser_rejects_missing_or_unknown_decision(self):
        with tempfile.TemporaryDirectory() as dirname:
            file = Path(dirname) / "not-a-report.json"
            file.write_text('{"decision": "MAGIC"}')
            with self.assertRaises(bench.InvalidCase):
                bench.parse_uni_report(file)

    def test_agent_self_report_is_read_from_actual_last_line(self):
        with tempfile.TemporaryDirectory() as dirname:
            file = Path(dirname) / "agent.txt"
            file.write_text("working...\nDONE\n")
            self.assertEqual(bench.parse_agent_report(file)[0], "DONE")
            file.write_text("DONE\nother output after\n")
            self.assertEqual(bench.parse_agent_report(file)[0], "UNPARSED")

    def test_confusion_counts_ignore_unknown(self):
        rows = [
            {"uni_decision": "ACCEPTED", "ci_decision": "PASS",
             "agent_self_report": "DONE", "oracle_accept": False},
            {"uni_decision": "EVIDENCE_REQUIRED", "ci_decision": "FAIL",
             "agent_self_report": "FAILED", "oracle_accept": True},
            {"uni_decision": "UNOBSERVED", "ci_decision": "UNOBSERVED",
             "agent_self_report": "UNOBSERVED", "oracle_accept": True},
        ]
        matrix = bench.measure(rows, "UNI")
        self.assertEqual((matrix["false_accepts"], matrix["false_rejects"]), (1, 1))
        self.assertEqual(matrix["unobserved"], 1)
        self.assertEqual(matrix["false_accept_rate"], 1)
        self.assertEqual(matrix["false_reject_rate"], 1)


if __name__ == "__main__":
    unittest.main()
