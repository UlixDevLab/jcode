"""Regression: the boolean --skip-mcp-npm-ci flag must work in any option order.

See jcode-lite/tests/test_build_cli_parser.py for the canonical description;
this file mirrors it for the free edition, which carries the same parser bug.
"""

import os
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BUILD_SH = ROOT / "build.sh"
MISSING_BINARY = "/nonexistent-jcode-binary-for-parser-test"


def _run_build(args):
    return subprocess.run(
        [str(BUILD_SH), *args],
        cwd=str(ROOT),
        env={**os.environ, "PATH": "/usr/bin:/bin"},
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


class SkipMcpNpmCiParserTest(unittest.TestCase):
    """Verify --skip-mcp-npm-ci parses cleanly regardless of position."""

    def _assert_parser_succeeded(self, result):
        combined = (result.stdout + result.stderr).lower()
        self.assertNotIn(
            "unknown argument",
            combined,
            msg=f"parser rejected valid flag combination: {result.stderr}",
        )
        self.assertNotIn(
            "shift: positional parameter",
            combined,
            msg=f"parser failed shift on boolean flag: {result.stderr}",
        )
        self.assertIn(
            "jcode binary not found",
            combined,
            msg=f"expected parser to accept args and reach binary check: {result.stderr}",
        )

    def test_skip_before_output(self):
        result = _run_build([
            "--platform", "macos",
            "--binary", MISSING_BINARY,
            "--skip-mcp-npm-ci",
            "--output", "/tmp/jcode-lite-free-parser-skip-before.zip",
        ])
        self._assert_parser_succeeded(result)

    def test_skip_after_output(self):
        result = _run_build([
            "--platform", "macos",
            "--binary", MISSING_BINARY,
            "--output", "/tmp/jcode-lite-free-parser-skip-after.zip",
            "--skip-mcp-npm-ci",
        ])
        self._assert_parser_succeeded(result)

    def test_skip_as_last_argument(self):
        result = _run_build([
            "--platform", "macos",
            "--binary", MISSING_BINARY,
            "--skip-mcp-npm-ci",
        ])
        self._assert_parser_succeeded(result)

    def test_skip_between_value_flags(self):
        result = _run_build([
            "--platform", "macos",
            "--skip-mcp-npm-ci",
            "--binary", MISSING_BINARY,
            "--output", "/tmp/jcode-lite-free-parser-skip-middle.zip",
        ])
        self._assert_parser_succeeded(result)


if __name__ == "__main__":
    unittest.main()
