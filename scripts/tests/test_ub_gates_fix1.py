"""Regression checks for the GMA-505 round-1 ub-gates fixes."""

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCOPE_PY = ROOT / "scripts" / "ub_changed_scope.py"
CI_SH = ROOT / "scripts" / "run_ub_gates_ci.sh"
CONTAINER_SH = ROOT / "scripts" / "ub_gates_container.sh"
ACTION = ROOT / ".github" / "actions" / "ub-gates" / "action.yml"
WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
README = ROOT / "README.md"

_CRAP = """{:entries [
  {:name "changed", :namespace "pkg", :complexity 1, :coverage 0.0, :crap 2.0}
  {:name "old", :namespace "pkg", :complexity 4, :coverage 0.0, :crap 30.0}
]}
"""
_DRY_OLD = """{:candidates [
 {:score 0.91
  :language "python"
  :left {:file "lib/a.py", :start-line 4, :end-line 5}
  :right {:file "lib/a.py", :start-line 4, :end-line 5}}
]}
"""
_DRY_CHANGED = """{:candidates [
 {:score 0.91
  :language "python"
  :left {:file "lib/a.py", :start-line 1, :end-line 2}
  :right {:file "lib/a.py", :start-line 1, :end-line 2}}
]}
"""
_SPANS = [
    {
        "namespace": "pkg",
        "name": "changed",
        "start_line": 1,
        "end_line": 2,
        "path": "lib/a.py",
    },
    {
        "namespace": "pkg",
        "name": "old",
        "start_line": 4,
        "end_line": 5,
        "path": "lib/a.py",
    },
]
_TIMEOUT_SHIM = """#!/usr/bin/env bash
secs="$1"
shift
exec python3 - "$secs" "$@" <<'PY'
import os
import signal
import subprocess
import sys

secs = float(sys.argv[1])
cmd = sys.argv[2:]
proc = subprocess.Popen(cmd, start_new_session=True)
try:
    code = proc.wait(timeout=secs)
except subprocess.TimeoutExpired:
    os.killpg(proc.pid, signal.SIGTERM)
    proc.wait()
    sys.exit(124)
sys.exit(code)
PY
"""


def _load_scope():
    spec = importlib.util.spec_from_file_location("ub_changed_scope", SCOPE_PY)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load ub_changed_scope.py")
    module = importlib.util.module_from_spec(spec)
    import sys

    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _git(repo: Path, *args: str) -> None:
    subprocess.run(
        ["git", "-C", str(repo), *args],
        check=True,
        capture_output=True,
        text=True,
    )


def _init_repo(repo: Path) -> None:
    repo.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["git", "init", "-b", "main"],
        cwd=repo,
        check=True,
        capture_output=True,
        text=True,
    )
    _git(repo, "config", "user.email", "ub-gates@example.com")
    _git(repo, "config", "user.name", "ub-gates")


def _write_exe(path: Path, body: str) -> None:
    path.write_text(body, encoding="utf-8")
    path.chmod(0o755)


def _env(bin_dir: Path, **extra: str) -> dict[str, str]:
    kept = {
        "PATH": f"{bin_dir}{os.pathsep}{os.environ.get('PATH', '/usr/bin:/bin')}",
        "HOME": os.environ.get("HOME", "/tmp"),
        "TMPDIR": os.environ.get("TMPDIR", "/tmp"),
        "LANG": os.environ.get("LANG", "C"),
    }
    kept.update(extra)
    return kept


def _run_ci(repo: Path, env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", str(CI_SH)],
        cwd=repo,
        env=env,
        capture_output=True,
        text=True,
    )


def _outcome(repo: Path) -> str:
    return (repo / ".metrics" / "ub-gates-outcome.txt").read_text(encoding="utf-8")


class LinesForPathTests(unittest.TestCase):
    def test_many_paths_do_not_recurse(self) -> None:
        scope = _load_scope()
        changed = {f"p{i}.py": [i] for i in range(2000)}
        changed["hit.py"] = [4, 5]
        self.assertEqual(scope.lines_for_path("p0.py", changed), [0])
        self.assertEqual(scope.lines_for_path("p1999.py", changed), [1999])
        self.assertEqual(scope.lines_for_path("hit.py", changed), [4, 5])
        self.assertEqual(scope.lines_for_path("missing.py", changed), [])


class OverlayTests(unittest.TestCase):
    def test_cleared_destination_matches_mounted_tree(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "src-repo"
            _init_repo(src)
            (src / "scripts").mkdir()
            (src / "src").mkdir()
            (src / "scripts" / "run.sh").write_text("echo hi\n", encoding="utf-8")
            (src / "src" / "lib.rs").write_text("fn main() {}\n", encoding="utf-8")
            _git(src, "add", ".")
            _git(src, "commit", "-m", "base")
            (src / "notes.txt").write_text("untracked\n", encoding="utf-8")
            (src / "target").mkdir()
            (src / "target" / "junk").write_text("nope\n", encoding="utf-8")
            (src / ".metrics").mkdir()
            (src / ".metrics" / "x").write_text("nope\n", encoding="utf-8")
            dest = root / "clone"
            subprocess.run(
                ["git", "clone", "--quiet", str(src), str(dest)],
                check=True,
                capture_output=True,
                text=True,
            )
            (dest / "stale.txt").write_text("stale\n", encoding="utf-8")
            (dest / "src" / "src").mkdir()
            (dest / "src" / "src" / "nested.rs").write_text("nested\n", encoding="utf-8")
            subprocess.run(
                [
                    "bash",
                    "-c",
                    'source "$1"; overlay_worktree "$2" "$3"',
                    "_",
                    str(CONTAINER_SH),
                    str(src),
                    str(dest),
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            self.assertFalse((dest / "src" / "src").exists())
            self.assertFalse((dest / "scripts" / "scripts").exists())
            self.assertFalse((dest / "stale.txt").exists())
            self.assertFalse((dest / "target").exists())
            self.assertFalse((dest / ".metrics").exists())
            self.assertTrue((dest / ".git").exists())
            self.assertEqual((dest / "scripts" / "run.sh").read_text(encoding="utf-8"), "echo hi\n")
            self.assertEqual((dest / "src" / "lib.rs").read_text(encoding="utf-8"), "fn main() {}\n")
            self.assertEqual((dest / "notes.txt").read_text(encoding="utf-8"), "untracked\n")
            status = subprocess.run(
                ["git", "-C", str(dest), "status", "--porcelain"],
                check=True,
                capture_output=True,
                text=True,
            )
            self.assertEqual(status.stdout.strip(), "?? notes.txt")


class AdvisoryGateTests(unittest.TestCase):
    def _repo(self, tmp: str) -> tuple[Path, Path]:
        repo = Path(tmp)
        _init_repo(repo)
        lib = repo / "lib"
        lib.mkdir()
        (lib / "a.py").write_text("def changed():\n    return 1\n\ndef old():\n    return 0\n", encoding="utf-8")
        _git(repo, "add", "lib/a.py")
        _git(repo, "commit", "-m", "base")
        _git(repo, "checkout", "-b", "feat")
        (lib / "a.py").write_text("def changed():\n    return 2\n\ndef old():\n    return 0\n", encoding="utf-8")
        _git(repo, "add", "lib/a.py")
        _git(repo, "commit", "-m", "change")
        bin_dir = repo / "bin"
        bin_dir.mkdir()
        _write_exe(bin_dir / "timeout", _TIMEOUT_SHIM)
        return repo, bin_dir

    def _tools(
        self,
        repo: Path,
        bin_dir: Path,
        *,
        mutator: str,
        dry: str = _DRY_OLD,
        spans: list[dict[str, object]] | None = None,
    ) -> dict[str, str]:
        log = repo / "mutator.log"
        _write_exe(
            bin_dir / "crapper",
            "#!/usr/bin/env bash\nmkdir -p .metrics\ncat > .metrics/crap.edn <<'EDN'\n"
            + _CRAP
            + "EDN\nexit 0\n",
        )
        _write_exe(
            bin_dir / "dryer",
            "#!/usr/bin/env bash\nmkdir -p .metrics\ncat > .metrics/dry.edn <<'EDN'\n"
            + dry
            + "EDN\nexit 0\n",
        )
        _write_exe(bin_dir / "mutator", mutator.replace("LOG", str(log)))
        span_path = repo / "spans.json"
        span_path.write_text(json.dumps(spans if spans is not None else _SPANS), encoding="utf-8")
        return _env(
            bin_dir,
            UB_GATES_ADVISORY="1",
            UB_GATES_BASE_REF="main",
            UB_GATES_MUTATOR_BUDGET_SECS="1200",
            UB_GATES_MUTATOR_TEST_CMD="cargo test --locked",
            UB_SCOPE_FUNCTIONS_JSON=str(span_path),
            UB_GATES_OUTCOME_FILE=str(repo / ".metrics" / "ub-gates-outcome.txt"),
        )

    def test_advisory_joins_scope_and_budgets_mutator(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo, bin_dir = self._repo(tmp)
            env = self._tools(
                repo,
                bin_dir,
                mutator="#!/usr/bin/env bash\necho \"$@\" >> LOG\nexit 0\n",
            )
            proc = _run_ci(repo, env)
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            text = proc.stdout + proc.stderr
            self.assertIn("scope join crap findings=0 debt=1", text)
            self.assertIn("scope join dry findings=0 debt=1", text)
            self.assertIn("mutator budget 1200s", text)
            outcome = _outcome(repo)
            self.assertIn("outcome=ran_clean", outcome)
            self.assertIn("advisory=1", outcome)
            self.assertIn("crapper_debt=1", outcome)
            self.assertIn("dryer_debt=1", outcome)
            self.assertIn("crapper_findings=0", outcome)
            self.assertIn("dryer_findings=0", outcome)
            self.assertIn("mutator_budget_secs=1200", outcome)
            logged = (repo / "mutator.log").read_text(encoding="utf-8")
            self.assertIn("--lines", logged)
            self.assertIn("--test-command", logged)
            self.assertIn("cargo test --locked", logged)
            self.assertIn("lib/a.py", logged)

    def test_advisory_changed_line_finding_exits_zero(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo, bin_dir = self._repo(tmp)
            env = self._tools(
                repo,
                bin_dir,
                dry=_DRY_CHANGED,
                mutator="#!/usr/bin/env bash\necho \"$@\" >> LOG\nexit 0\n",
            )
            proc = _run_ci(repo, env)
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            outcome = _outcome(repo)
            self.assertIn("outcome=ran_with_findings", outcome)
            self.assertIn("dryer_findings=1", outcome)

    def test_advisory_mutator_exit_one_is_tool_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo, bin_dir = self._repo(tmp)
            env = self._tools(
                repo,
                bin_dir,
                mutator="#!/usr/bin/env bash\necho \"$@\" >> LOG\nexit 1\n",
            )
            proc = _run_ci(repo, env)
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            outcome = _outcome(repo)
            self.assertIn("outcome=tool_error", outcome)
            self.assertIn("mutator_exit=1", outcome)
            self.assertIn("tool_error=1", outcome)
            env["UB_GATES_ADVISORY"] = "0"
            blocking = _run_ci(repo, env)
            self.assertEqual(blocking.returncode, 1, blocking.stdout + blocking.stderr)
            self.assertIn("outcome=tool_error", _outcome(repo))
            self.assertIn("mutator_exit=1", _outcome(repo))

    def test_advisory_mutator_timeout_does_not_fail(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo, bin_dir = self._repo(tmp)
            lib = repo / "lib"
            (lib / "b.py").write_text("def other():\n    return 1\n", encoding="utf-8")
            _git(repo, "add", "lib/b.py")
            _git(repo, "commit", "-m", "second")
            env = self._tools(
                repo,
                bin_dir,
                dry="{:candidates []}\n",
                mutator="#!/usr/bin/env bash\necho \"$@\" >> LOG\nsleep 30\nexit 0\n",
            )
            env["UB_GATES_MUTATOR_BUDGET_SECS"] = "1"
            proc = _run_ci(repo, env)
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            text = proc.stdout + proc.stderr
            self.assertIn("mutator budget 1s", text)
            self.assertIn("non-blocking", text)
            outcome = _outcome(repo)
            self.assertIn("mutator_timeout=1", outcome)
            self.assertIn("outcome=ran_clean", outcome)
            logged = (repo / "mutator.log").read_text(encoding="utf-8").strip().splitlines()
            self.assertEqual(len(logged), 1)
            self.assertIn("--lines", logged[0])
            self.assertNotIn("lib/b.py", logged[0])


class WorkflowTextTests(unittest.TestCase):
    def test_shell_scripts_parse(self) -> None:
        for path in (CI_SH, CONTAINER_SH):
            subprocess.run(["bash", "-n", str(path)], check=True)

    def test_hidden_metrics_and_hub_path_env(self) -> None:
        text = ACTION.read_text(encoding="utf-8")
        self.assertIn("include-hidden-files: true", text)
        self.assertIn("path: .metrics/", text)
        self.assertIn("UB_GATES_HUB_PATH: ${{ inputs.hub-path }}", text)
        runs = text.split("runs:", 1)[1]
        for part in runs.split("run: |")[1:]:
            script = part.split("\n    - ")[0]
            self.assertNotIn("inputs.hub-path", script)

    def test_locked_coverage_budget_and_unittest_step(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        container = CONTAINER_SH.read_text(encoding="utf-8")
        readme = README.read_text(encoding="utf-8")
        self.assertIn("timeout-minutes: 45", workflow)
        self.assertIn(
            "cargo llvm-cov --locked --lcov --output-path target/coverage/rust/lcov.info",
            workflow,
        )
        self.assertIn(
            "cargo llvm-cov --locked --lcov --output-path target/coverage/rust/lcov.info",
            container,
        )
        self.assertIn("python3 -m unittest discover -s scripts/tests", workflow)
        self.assertIn("UB_GATES_MUTATOR_BUDGET_SECS=1200", readme)
        self.assertIn("rustup component add llvm-tools-preview", readme)
        self.assertIn("`uv`", readme)
        self.assertIn("`cargo install`", readme)
        self.assertIn("20 minutes", readme)


if __name__ == "__main__":
    unittest.main()
