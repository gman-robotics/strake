"""Mutator N/A when a diff has no production mutation sites (GMA-614)."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from test_ub_gates_fix1 import _env, _git, _init_repo, _outcome, _run_ci, _write_exe


_PRODUCTION = """fn test_step() -> i32 {
    1
}
"""

_TEST_ONLY = """fn test_step() -> i32 {
    1
}

#[cfg(test)]
fn checks_production() -> i32 {
    2
}
"""

_TEST_ONLY_NEXT = """fn test_step() -> i32 {
    1
}

#[cfg(test)]
fn checks_production() -> i32 {
    3
}
"""

_DIFF_FAIL_GIT = """#!/usr/bin/env bash
if [[ "$1" == "diff" ]]; then
  echo "diff failed" >&2
  exit 1
fi
exec /usr/bin/git "$@"
"""


def _edn(forms: list[str]) -> str:
    body = "\n".join(forms)
    return (
        "{:version 1\n"
        ' :source "src/lib.rs"\n'
        ' :namespace "crate"\n'
        " :forms [\n"
        f"{body}\n"
        "]}\n"
    )


def _form(name: str, line: int, end: int, sites: int, killed: int = 0, survived: int = 0, uncovered: int = 0) -> str:
    return (
        f'  {{:id "defn/{name}" :file "src/lib.rs" :line {line} :end-line {end}'
        f" :killed {killed} :survived {survived} :uncovered {uncovered} :sites {sites}}}"
    )


class MutatorNaTests(unittest.TestCase):
    def _repo(self, tmp: str, before: str, after: str) -> tuple[Path, Path]:
        repo = Path(tmp)
        _init_repo(repo)
        src = repo / "src"
        src.mkdir()
        (src / "lib.rs").write_text(before, encoding="utf-8")
        _git(repo, "add", "src/lib.rs")
        _git(repo, "commit", "-m", "base")
        _git(repo, "checkout", "-b", "feat")
        (src / "lib.rs").write_text(after, encoding="utf-8")
        _git(repo, "add", "src/lib.rs")
        _git(repo, "commit", "-m", "change")
        bin_dir = repo / "bin"
        bin_dir.mkdir()
        return repo, bin_dir

    def _tools(self, repo: Path, bin_dir: Path, edn: str, status: str) -> dict[str, str]:
        _write_exe(
            bin_dir / "crapper",
            "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:entries []}' > .metrics/crap.edn\nexit 0\n",
        )
        _write_exe(
            bin_dir / "dryer",
            "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:candidates []}' > .metrics/dry.edn\nexit 0\n",
        )
        _write_exe(
            bin_dir / "mutator",
            "#!/usr/bin/env bash\n"
            "mkdir -p .metrics/mutate\n"
            f"printf '%s\\n' {json.dumps(status)}\n"
            "cat > .metrics/mutate/crate.edn <<'EDN'\n"
            + edn
            + "EDN\nexit 0\n",
        )
        span_path = repo / "spans.json"
        span_path.write_text("[]\n", encoding="utf-8")
        return _env(
            bin_dir,
            UB_GATES_ADVISORY="0",
            UB_GATES_BASE_REF="main",
            UB_GATES_MUTATOR_BUDGET_SECS="30",
            UB_SCOPE_FUNCTIONS_JSON=str(span_path),
            UB_GATES_OUTCOME_FILE=str(repo / ".metrics" / "ub-gates-outcome.txt"),
        )

    def test_test_only_diff_passes_with_mutator_na(self) -> None:
        # checks_production is #[cfg(test)]. test_step is production and is not
        # in the diff. The name test_step is not what makes a site a test.
        edn = _edn(
            [
                _form("test_step", 1, 3, 4),
                _form("checks_production", 6, 8, 2, uncovered=2),
            ]
        )
        with tempfile.TemporaryDirectory() as tmp:
            repo, bin_dir = self._repo(tmp, _TEST_ONLY, _TEST_ONLY_NEXT)
            env = self._tools(repo, bin_dir, edn, "UNCOVERED src/lib.rs:7 2 -> 3")
            proc = _run_ci(repo, env)
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, text)
            outcome = _outcome(repo)
            self.assertIn("outcome=ran_clean", outcome)
            self.assertIn("mutator_na=1", outcome)
            self.assertIn("ub-gates: mutator N/A (0 production mutation sites)", text)
            self.assertNotIn("outcome=mutator_no_coverage", outcome)
            self.assertNotIn("covered no sites", text)

    def test_production_diff_with_no_coverage_fails(self) -> None:
        # The function is named test_step and it is still production code.
        edn = _edn([_form("test_step", 1, 3, 4, uncovered=4)])
        before = _PRODUCTION
        after = _PRODUCTION.replace("    1\n", "    2\n", 1)
        with tempfile.TemporaryDirectory() as tmp:
            repo, bin_dir = self._repo(tmp, before, after)
            env = self._tools(repo, bin_dir, edn, "UNCOVERED src/lib.rs:2 1 -> 2")
            proc = _run_ci(repo, env)
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            outcome = _outcome(repo)
            self.assertIn("outcome=mutator_no_coverage", outcome)
            self.assertIn("blocking mode — mutator covered no sites", text)
            self.assertIn("uncovered on a changed line", text)
            self.assertNotIn("mutator N/A", text)
            self.assertNotIn("mutator_na=1", outcome)

    def test_diff_error_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _init_repo(repo)
            src = repo / "src"
            src.mkdir()
            (src / "lib.rs").write_text(_PRODUCTION, encoding="utf-8")
            _git(repo, "add", "src/lib.rs")
            _git(repo, "commit", "-m", "base")
            bin_dir = repo / "bin"
            bin_dir.mkdir()
            _write_exe(bin_dir / "git", _DIFF_FAIL_GIT)
            env = _env(
                bin_dir,
                UB_GATES_ADVISORY="0",
                UB_GATES_BASE_REF="main",
                UB_GATES_OUTCOME_FILE=str(repo / ".metrics" / "ub-gates-outcome.txt"),
            )
            proc = _run_ci(repo, env)
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            outcome = _outcome(repo)
            self.assertIn("outcome=tool_error", outcome)
            self.assertIn("git diff --name-only failed", text)
            self.assertNotIn("mutator N/A", text)


if __name__ == "__main__":
    unittest.main()
