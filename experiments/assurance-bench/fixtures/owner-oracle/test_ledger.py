"""Owner-only acceptance tests: supplied after the worker delivers a patch.

The agent workspace contains only ledger.py, never this file.
"""

import importlib.util
import os
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location(
    "candidate_ledger", Path(os.environ["UNI_BENCH_SUBJECT"]) / "ledger.py"
)
ledger = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ledger)


class LedgerOwnerCriteria(unittest.TestCase):
    def test_regular_credit(self):
        self.assertEqual(ledger.apply(100, 25), 125)

    def test_regular_debit(self):
        self.assertEqual(ledger.apply(100, -20), 80)

    def test_overdraft_must_be_refused(self):
        with self.assertRaises(ValueError):
            ledger.apply(5, -10)


if __name__ == "__main__":
    unittest.main()
