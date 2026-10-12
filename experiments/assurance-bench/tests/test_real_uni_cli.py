"""Run the actual UNI CLI against a deliberately limited visible-test contract.

The independent oracle must expose a blind spot without faking a UNI verdict.
This is a scripted synthetic case, not a real-agent effectiveness study.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
REPO = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "fixtures"
CLI = REPO / "target" / "release" / ("uni.exe" if sys.platform == "win32" else "uni")
spec = importlib.util.spec_from_file_location("uni_benchmark_live_harness", ROOT / "harness.py")
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)

VISIBLE_TESTS = """import unittest
import ledger

class VisibleLedgerTests(unittest.TestCase):
    def test_credit(self):
        self.assertEqual(ledger.apply(100, 25), 125)

    def test_debit(self):
        self.assertEqual(ledger.apply(100, -20), 80)
"""

CONTRACT = """VERSION 0.1
DOMAIN software
INTENT synthetic.ledger
GOAL
  Verify the two visible positive-behavior cases only.
CLAIM visible REQUIRED
  ENSURE regular credits and debits behave as shown
VERIFY visible
  USING visible.tests
ACCEPT WHEN
  required_claims == VERIFIED
  AND critical_failures == 0
"""

REGISTRY = """[verifiers]
"visible.tests" = "python3 -m unittest discover -s . -p 'test_visible.py'"
"""


class RealUniSyntheticStudy(unittest.TestCase):
    def _evaluate_candidate(self, variant: str):
        self.assertTrue(CLI.is_file(), f"Build UNI before running this study: {CLI}")
        with tempfile.TemporaryDirectory(prefix="uni-synthetic-study-") as temporary:
            base = Path(temporary)
            candidate = base / "candidate"
            candidate.mkdir()
            shutil.copyfile(FIXTURES / variant / "ledger.py", candidate / "ledger.py")
            (candidate / "test_visible.py").write_text(VISIBLE_TESTS)
            (candidate / "demo.uni").write_text(CONTRACT)
            (candidate / ".uni").mkdir()
            (candidate / ".uni/config.toml").write_text(REGISTRY)
            report = base / "uni-result.json"
            env = os.environ.copy()
            env["PYTHONDONTWRITEBYTECODE"] = "1"
            process = subprocess.run(
                [str(CLI), "--json", "verify", "demo.uni"],
                cwd=candidate, capture_output=True, text=True, env=env, timeout=30,
                check=False,
            )
            self.assertEqual(process.returncode, 0, process.stderr)
            report.write_text(process.stdout)
            self.assertEqual(json.loads(report.read_text())["decision"], "Accepted")
            return bench.evaluate(argparse.Namespace(
                baseline=FIXTURES / "baseline",
                candidate=candidate,
                oracle=FIXTURES / "owner-oracle",
                case_id=f"synthetic-{variant}",
                run_id="single",
                kind="scripted",
                timeout=30,
                uni_json=report,
                agent_output=None,
                ci_decision="UNOBSERVED",
            ))

    def test_real_uni_can_accept_incomplete_visible_contract(self):
        delivered = self._evaluate_candidate("plausible-wrong")
        self.assertEqual(delivered["uni_decision"], "ACCEPTED")
        self.assertFalse(delivered["oracle_accept"])
        self.assertIsNotNone(delivered["uni_report_sha256"])
        self.assertEqual(delivered["agent_self_report"], "UNOBSERVED")
        score = bench.measure([delivered], "UNI")
        self.assertEqual(score["false_accepts"], 1)

    def test_same_contract_accepts_a_correct_delivery_too(self):
        delivered = self._evaluate_candidate("correct")
        self.assertEqual(delivered["uni_decision"], "ACCEPTED")
        self.assertTrue(delivered["oracle_accept"])
        self.assertEqual(bench.measure([delivered], "UNI")["true_accepts"], 1)


if __name__ == "__main__":
    unittest.main()
