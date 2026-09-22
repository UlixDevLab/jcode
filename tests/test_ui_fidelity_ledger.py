#!/usr/bin/env python3
"""Contract tests for the W5 approved UI fidelity ledger consumer."""

from __future__ import annotations

import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CONSUMER = ROOT / "scripts" / "desktop2_visual_check.sh"
FIXTURES = ROOT / "tests" / "fixtures" / "ui_fidelity_ledger"


class ApprovedUiFidelityLedgerTest(unittest.TestCase):
    def run_consumer(self, approval_name: str, observed_name: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                str(CONSUMER),
                "--approval-ledger",
                str(FIXTURES / approval_name),
                "--observed",
                str(FIXTURES / observed_name),
                "--surface",
                "desktop",
            ],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
        )

    def test_silently_omitted_approved_element_blocks_through_visual_check_consumer(self) -> None:
        result = self.run_consumer("approved-desktop.json", "observed-missing-E2.json")

        self.assertEqual(1, result.returncode, result.stdout + result.stderr)
        self.assertIn("BLOCK", result.stdout)
        self.assertIn("E2", result.stdout)

    def test_declared_deviation_passes_and_reports_consumed_metadata(self) -> None:
        result = self.run_consumer("approved-desktop.json", "observed-deviation-E2.json")

        self.assertEqual(0, result.returncode, result.stdout + result.stderr)
        self.assertIn("PASS", result.stdout)
        self.assertIn("artifact.kind=html-mock", result.stdout)
        self.assertIn("artifact.hash=sha256:" + "a" * 64, result.stdout)
        self.assertIn("escape_hatches=warn-only-lint,review-band-drift,migrate-budget", result.stdout)
        self.assertIn("deviations=1", result.stdout)

    def test_browser_mock_artifact_cannot_pass_for_desktop_surface(self) -> None:
        result = self.run_consumer("approved-browser-mock.json", "observed-all-present.json")

        self.assertEqual(1, result.returncode, result.stdout + result.stderr)
        self.assertIn("BLOCK", result.stdout)
        self.assertIn("browser-mock", result.stdout)


if __name__ == "__main__":
    unittest.main()
