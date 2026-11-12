"""Fixture checks for scripts/check_pr_body.py, the pr-body CI command."""

from __future__ import annotations

import os
import subprocess
import unittest
from pathlib import Path


def _repo_root() -> Path:
    cwd = Path.cwd()
    if (cwd / "scripts" / "check_pr_body.py").is_file():
        return cwd
    return Path(__file__).resolve().parents[2]


ROOT = _repo_root()
CHECK = ROOT / "scripts" / "check_pr_body.py"
FIXTURES = Path(__file__).resolve().parent / "fixtures" / "pr-body"
WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"


def _run(args: list[str], env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["python3", str(CHECK), *args],
        cwd=ROOT,
        env=env,
        capture_output=True,
        text=True,
    )


class PrBodyTests(unittest.TestCase):
    def test_complete_fixture_exits_zero(self) -> None:
        proc = _run([str(FIXTURES / "complete.md")])
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(proc.stderr, "")

    def test_missing_heading_exits_one(self) -> None:
        proc = _run([str(FIXTURES / "missing-tier.md")])
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("missing Tier", proc.stderr)

    def test_empty_section_exits_one(self) -> None:
        proc = _run([str(FIXTURES / "empty-summary.md")])
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("empty Summary", proc.stderr)

    def test_empty_first_section_exits_one(self) -> None:
        proc = _run([str(FIXTURES / "empty-tier.md")])
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("empty Tier", proc.stderr)

    def test_env_body_matches_the_file_contract(self) -> None:
        text = (FIXTURES / "complete.md").read_text(encoding="utf-8")
        env = dict(os.environ)
        env["PR_BODY"] = text
        proc = _run([], env)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        env["PR_BODY"] = ""
        empty = _run([], env)
        self.assertEqual(empty.returncode, 1, empty.stderr)
        self.assertIn("missing Tier", empty.stderr)

    def test_workflow_runs_the_checker_on_pull_requests(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("\n  pr-body:\n", workflow)
        self.assertIn("name: pr-body\n", workflow)
        self.assertIn("if: github.event_name == 'pull_request'", workflow)
        self.assertIn("python3 scripts/check_pr_body.py", workflow)
        self.assertNotIn("continue-on-error", workflow)
