"""Regression checks for the GMA-505 round-1 ub-gates fixes."""

from __future__ import annotations

import importlib.util
import json
import os
import shlex
import subprocess
import tempfile
import unittest
from pathlib import Path


def _repo_root() -> Path:
    cwd = Path.cwd()
    if (cwd / "scripts" / "run_ub_gates_ci.sh").is_file():
        return cwd
    return Path(__file__).resolve().parents[2]


ROOT = _repo_root()
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
_TIMEOUT_AFTER_LOG = """#!/usr/bin/env python3
import os
import signal
import subprocess
import sys
import time

cmd = sys.argv[2:]
log = LOG_PATH
proc = subprocess.Popen(cmd, start_new_session=True)
deadline = time.time() + 15
while time.time() < deadline:
    if os.path.isfile(log) and os.path.getsize(log) > 0:
        break
    if proc.poll() is not None:
        break
    time.sleep(0.05)
try:
    os.killpg(proc.pid, signal.SIGTERM)
except ProcessLookupError:
    pass
proc.wait()
if not (os.path.isfile(log) and os.path.getsize(log) > 0):
    sys.stderr.write("mutator log missing\\n")
    sys.exit(2)
sys.exit(124)
"""
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
    for key in ("COVERAGE_PROCESS_CONFIG", "COVERAGE_PROCESS_START", "COVERAGE_FILE"):
        if key in os.environ:
            kept[key] = os.environ[key]
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
            # A 1-second wall clock can end inside the scope-lines process,
            # before the mutator writes its log. This stub returns 124 only
            # after that log exists; the budget is large enough to start.
            _write_exe(
                bin_dir / "timeout",
                _TIMEOUT_AFTER_LOG.replace("LOG_PATH", repr(str(repo / "mutator.log"))),
            )
            env["UB_GATES_MUTATOR_BUDGET_SECS"] = "60"
            proc = _run_ci(repo, env)
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            text = proc.stdout + proc.stderr
            self.assertIn("mutator budget 60s", text)
            self.assertIn("non-blocking", text)
            outcome = _outcome(repo)
            self.assertIn("mutator_timeout=1", outcome)
            self.assertIn("outcome=ran_clean", outcome)
            logged = (repo / "mutator.log").read_text(encoding="utf-8").strip().splitlines()
            self.assertEqual(len(logged), 1)
            self.assertIn("--lines", logged[0])
            self.assertNotIn("lib/b.py", logged[0])

    def test_blocking_mutator_timeout_fails(self) -> None:
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
            _write_exe(
                bin_dir / "timeout",
                _TIMEOUT_AFTER_LOG.replace("LOG_PATH", repr(str(repo / "mutator.log"))),
            )
            env["UB_GATES_ADVISORY"] = "0"
            env["UB_GATES_MUTATOR_BUDGET_SECS"] = "60"
            proc = _run_ci(repo, env)
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("blocking", text)
            self.assertNotIn("non-blocking", text)
            outcome = _outcome(repo)
            self.assertIn("outcome=mutator_timeout", outcome)
            self.assertNotIn("outcome=ran_clean", outcome)
            self.assertIn("mutator_timeout=1", outcome)
            self.assertIn("advisory=0", outcome)


def _edn(rows: list[tuple[str, object, int | None]]) -> str:
    lines = ["{:entries ["]
    for name, crap, complexity in rows:
        token = "nil" if crap is None else str(crap)
        extra = ""
        if complexity is not None:
            extra = f", :complexity {complexity}"
        lines.append(
            f'  {{:name "{name}", :namespace "pkg"{extra}, :coverage 0.0, :crap {token}}}'
        )
    lines.append("]}")
    return "\n".join(lines) + "\n"


_SCOPE_SPANS = [
    {"namespace": "pkg", "name": "hot", "start_line": 1, "end_line": 2, "path": "lib/a.py"},
    {"namespace": "pkg", "name": "old", "start_line": 4, "end_line": 5, "path": "lib/a.py"},
]


class CrapperChangedScopeTests(unittest.TestCase):
    def _run(
        self,
        tmp: str,
        head_rows: list[tuple[str, object, int | None]],
        base_rows: list[tuple[str, object, int | None]],
    ) -> subprocess.CompletedProcess[str]:
        repo = Path(tmp)
        _init_repo(repo)
        lib = repo / "lib"
        lib.mkdir()
        before = "def hot():\n    return 1\n\ndef old():\n    return 0\n"
        after = "def hot():\n    return 2\n\ndef old():\n    return 0\n"
        (lib / "a.py").write_text(before, encoding="utf-8")
        _git(repo, "add", "lib/a.py")
        _git(repo, "commit", "-m", "base")
        _git(repo, "checkout", "-b", "feat")
        (lib / "a.py").write_text(after, encoding="utf-8")
        _git(repo, "add", "lib/a.py")
        _git(repo, "commit", "-m", "change")
        bin_dir = repo / "bin"
        bin_dir.mkdir()
        head_path = repo / "head.edn"
        base_path = repo / "base.edn"
        head_path.write_text(_edn(head_rows), encoding="utf-8")
        base_path.write_text(_edn(base_rows), encoding="utf-8")
        _write_exe(
            bin_dir / "crapper",
            "#!/usr/bin/env bash\n"
            "mkdir -p .metrics\n"
            f"src={str(head_path)!r}\n"
            'case "$PWD" in\n'
            f"  *ub-crap-base*) src={str(base_path)!r} ;;\n"
            "esac\n"
            'cp "$src" .metrics/crap.edn\n'
            "exit 0\n",
        )
        _write_exe(
            bin_dir / "dryer",
            "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:candidates []}' > .metrics/dry.edn\nexit 0\n",
        )
        _write_exe(bin_dir / "mutator", "#!/usr/bin/env bash\nexit 0\n")
        span_path = repo / "spans.json"
        span_path.write_text(json.dumps(_SCOPE_SPANS), encoding="utf-8")
        env = _env(
            bin_dir,
            UB_GATES_ADVISORY="0",
            UB_GATES_BASE_REF="main",
            UB_GATES_MUTATOR_BUDGET_SECS="30",
            UB_SCOPE_FUNCTIONS_JSON=str(span_path),
            UB_GATES_OUTCOME_FILE=str(repo / ".metrics" / "ub-gates-outcome.txt"),
        )
        return _run_ci(repo, env)

    def test_changed_function_over_threshold_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(
                tmp,
                [("hot", 30.0, 4), ("old", 213.0, 20)],
                [("old", 213.0, 20)],
            )
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("outcome=ran_with_findings", _outcome(Path(tmp)))
            self.assertIn("crapper findings pkg hot crap=30.0", text)
            self.assertIn("crapper debt pkg old crap=213.0", text)

    def test_untouched_preexisting_debt_passes_and_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(
                tmp,
                [("hot", 2.0, 1), ("old", 213.0, 20)],
                [("hot", 2.0, 1), ("old", 213.0, 20)],
            )
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, text)
            outcome = _outcome(Path(tmp))
            self.assertIn("outcome=ran_clean", outcome)
            self.assertIn("crapper_findings=0", outcome)
            self.assertIn("crapper_debt=1", outcome)
            self.assertIn("crapper debt pkg old crap=213.0", text)
            self.assertNotIn("crapper findings pkg old", text)

    def test_worsened_preexisting_debt_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(
                tmp,
                [("hot", 40.0, 8), ("old", 213.0, 20)],
                [("hot", None, 6), ("old", 213.0, 20)],
            )
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("outcome=ran_with_findings", _outcome(Path(tmp)))
            self.assertIn("crapper findings pkg hot crap=40.0", text)
            self.assertIn("crapper debt pkg old crap=213.0", text)

    def test_touched_preexisting_debt_that_did_not_rise_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(
                tmp,
                [("hot", 40.0, 8), ("old", 213.0, 20)],
                [("hot", None, 8), ("old", 213.0, 20)],
            )
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, text)
            outcome = _outcome(Path(tmp))
            self.assertIn("outcome=ran_clean", outcome)
            self.assertIn("crapper_findings=0", outcome)
            self.assertIn("crapper debt pkg hot crap=40.0", text)
            self.assertIn("crapper debt pkg old crap=213.0", text)

    def test_missing_head_complexity_with_base_complexity_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(
                tmp,
                [("hot", 30.0, None), ("old", 213.0, 20)],
                [("hot", None, 8), ("old", 213.0, 20)],
            )
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("outcome=ran_with_findings", _outcome(Path(tmp)))
            self.assertIn("crapper findings pkg hot crap=30.0", text)

    def test_equal_numeric_crap_is_not_worse(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(
                tmp,
                [("hot", 30.0, 4), ("old", 213.0, 20)],
                [("hot", 30.0, 4), ("old", 213.0, 20)],
            )
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, text)
            self.assertIn("crapper debt pkg hot crap=30.0", text)
            self.assertNotIn("crapper findings pkg hot", text)

    def test_missing_base_complexity_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(
                tmp,
                [("hot", 30.0, 4), ("old", 213.0, 20)],
                [("hot", None, None), ("old", 213.0, 20)],
            )
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("crapper findings pkg hot crap=30.0", text)


def _mutator_edn(killed: int, survived: int, uncovered: int, sites: int) -> str:
    return (
        "{:version 1\n"
        ' :source "lib/a.py"\n'
        " :namespace \"lib.a\"\n"
        " :forms [{:id \"defn/hot\""
        f" :killed {killed} :survived {survived} :uncovered {uncovered} :sites {sites}"
        "}]}\n"
    )


class MutatorCoverageTests(unittest.TestCase):
    def _run(
        self,
        tmp: str,
        edn: str,
        *,
        advisory: str = "0",
        with_tests: bool = False,
        status: str = "",
    ) -> subprocess.CompletedProcess[str]:
        repo = Path(tmp)
        _init_repo(repo)
        lib = repo / "lib"
        lib.mkdir()
        (lib / "a.py").write_text("def hot():\n    return 1\n", encoding="utf-8")
        _git(repo, "add", "lib/a.py")
        _git(repo, "commit", "-m", "base")
        _git(repo, "checkout", "-b", "feat")
        (lib / "a.py").write_text("def hot():\n    return 2\n", encoding="utf-8")
        _git(repo, "add", "lib/a.py")
        _git(repo, "commit", "-m", "change")
        bin_dir = repo / "bin"
        bin_dir.mkdir()
        if with_tests:
            tests = repo / "scripts" / "tests"
            tests.mkdir(parents=True)
            (tests / "test_noop.py").write_text("import unittest\n\nclass T(unittest.TestCase):\n    def test_ok(self):\n        self.assertTrue(True)\n", encoding="utf-8")
            _write_exe(
                bin_dir / "coverage",
                "#!/usr/bin/env bash\n"
                "printf '%s\\n' \"$*\" >> COVERAGE_LOG\n"
                "if [[ \"$1\" == \"lcov\" ]]; then\n"
                "  out=target/coverage/python/lcov.info\n"
                "  prev=\n"
                "  for arg in \"$@\"; do\n"
                "    if [[ \"${prev}\" == \"-o\" ]]; then out=\"${arg}\"; fi\n"
                "    prev=\"${arg}\"\n"
                "  done\n"
                "  mkdir -p \"$(dirname \"$out\")\"\n"
                "  printf '%s\\n' 'SF:lib/a.py' 'DA:1,1' 'end_of_record' > \"${out}\"\n"
                "fi\n"
                "exit 0\n",
            )
        _write_exe(
            bin_dir / "crapper",
            "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:entries []}' > .metrics/crap.edn\nexit 0\n",
        )
        _write_exe(
            bin_dir / "dryer",
            "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:candidates []}' > .metrics/dry.edn\nexit 0\n",
        )
        status_line = ""
        if status:
            status_line = f"printf '%s\\n' {shlex.quote(status)}\n"
        _write_exe(
            bin_dir / "mutator",
            "#!/usr/bin/env bash\n"
            "mkdir -p .metrics/mutate\n"
            "printf '%s\\n' \"$*\" >> MUTATOR_LOG\n"
            + status_line
            + "cat > .metrics/mutate/lib.a.edn <<'EDN'\n"
            + edn
            + "EDN\nexit 0\n",
        )
        span_path = repo / "spans.json"
        span_path.write_text(
            json.dumps([{"namespace": "pkg", "name": "hot", "start_line": 1, "end_line": 2, "path": "lib/a.py"}]),
            encoding="utf-8",
        )
        env = _env(
            bin_dir,
            UB_GATES_ADVISORY=advisory,
            UB_GATES_BASE_REF="main",
            UB_GATES_MUTATOR_BUDGET_SECS="30",
            UB_GATES_MUTATOR_TEST_CMD="cargo test --locked",
            UB_SCOPE_FUNCTIONS_JSON=str(span_path),
            UB_GATES_OUTCOME_FILE=str(repo / ".metrics" / "ub-gates-outcome.txt"),
        )
        return _run_ci(repo, env)

    def test_blocking_zero_covered_sites_fail(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(tmp, _mutator_edn(0, 0, 4, 4))
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("outcome=mutator_no_coverage", _outcome(Path(tmp)))
            self.assertIn("mutator sites=4 covered=0 killed=0 survived=0 uncovered=4", text)
            self.assertIn("blocking mode — mutator covered no sites", text)
            self.assertIn("not_run=0", text)

    def test_file_with_sites_and_no_kills_fails(self) -> None:
        # A changed file with sites and killed + survived == 0 is a coverage
        # miss even when the uncovered line is outside the diff. Another file's
        # kills cannot cover this one.
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(
                tmp,
                _mutator_edn(0, 0, 1, 47),
                status="UNCOVERED lib/a.py:99 0 -> 1",
            )
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("outcome=mutator_no_coverage", _outcome(Path(tmp)))
            self.assertIn("mutator sites=47 covered=0 killed=0 survived=0 uncovered=1 not_run=46", text)
            self.assertIn("blocking mode — mutator covered no sites", text)
            self.assertIn("killed + survived == 0", text)

    def test_killed_mutant_with_unrun_sites_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(
                tmp,
                _mutator_edn(1, 0, 1, 47),
                status="KILLED    lib/a.py:2 0 -> 1\nUNCOVERED lib/a.py:99 true -> false",
            )
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, text)
            self.assertIn("outcome=ran_clean", _outcome(Path(tmp)))
            self.assertIn("mutator sites=47 covered=1 killed=1 survived=0 uncovered=1 not_run=45", text)

    def test_uncovered_changed_line_with_no_run_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(
                tmp,
                _mutator_edn(0, 0, 1, 47),
                status="UNCOVERED lib/a.py:2 true -> false",
            )
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("outcome=mutator_no_coverage", _outcome(Path(tmp)))
            self.assertIn("blocking mode — mutator covered no sites", text)
            self.assertIn("uncovered on a changed line", text)

    def test_covered_sites_pass(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(tmp, _mutator_edn(4, 0, 0, 4))
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, text)
            outcome = _outcome(Path(tmp))
            self.assertIn("outcome=ran_clean", outcome)
            self.assertIn("mutator_sites=4", outcome)
            self.assertIn("mutator_covered=4", outcome)
            self.assertIn("mutator_killed=4", outcome)
            self.assertIn("mutator_survived=0", outcome)

    def test_python_scope_measures_coverage_and_uses_script_tests(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(tmp, _mutator_edn(2, 0, 0, 2), with_tests=True)
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, text)
            repo = Path(tmp)
            coverage_log = (repo / "COVERAGE_LOG").read_text(encoding="utf-8")
            self.assertIn("run --rcfile", coverage_log)
            self.assertIn("-m unittest discover -s scripts/tests", coverage_log)
            self.assertIn("lcov --rcfile", coverage_log)
            mutator_log = (repo / "MUTATOR_LOG").read_text(encoding="utf-8")
            self.assertIn("python3 -m unittest discover -s scripts/tests", mutator_log)
            self.assertIn("--use-existing-coverage", mutator_log)
            self.assertNotIn("cargo test --locked", mutator_log)


class PythonLcovOrderTests(unittest.TestCase):
    """CI runs the Rust coverage command, then crapper. Python lcov has to be
    current before that score. These fixtures drive scripts/run_ub_gates_ci.sh
    the same way: advisory 0, UB_GATES_BASE_REF, and UB_GATES_COVERAGE_CMD.
    """

    def _drive(self, tmp: str, mode: str) -> subprocess.CompletedProcess[str]:
        repo = Path(tmp)
        _init_repo(repo)
        (repo / "README").write_text("base\n", encoding="utf-8")
        _git(repo, "add", "README")
        _git(repo, "commit", "-m", "base")
        _git(repo, "checkout", "-b", "feat")
        lib = repo / "lib"
        lib.mkdir()
        source = lib / "a.py"
        source.write_text("def hot():\n    return 1\n", encoding="utf-8")
        tests = repo / "scripts" / "tests"
        tests.mkdir(parents=True)
        (tests / "test_noop.py").write_text(
            "import unittest\n\nclass T(unittest.TestCase):\n    def test_ok(self):\n        self.assertTrue(True)\n",
            encoding="utf-8",
        )
        _git(repo, "add", "lib/a.py", "scripts/tests/test_noop.py")
        _git(repo, "commit", "-m", "change")
        if mode == "stale":
            lcov = repo / "target" / "coverage" / "python" / "lcov.info"
            lcov.parent.mkdir(parents=True)
            lcov.write_text("SF:lib/a.py\nDA:1,0\nend_of_record\n", encoding="utf-8")
            os.utime(lcov, (1, 1))
            (repo / "STALE_LCOV").write_text("1\n", encoding="utf-8")
        if mode == "missing":
            (repo / "MISSING_LCOV").write_text("1\n", encoding="utf-8")
        bin_dir = repo / "bin"
        bin_dir.mkdir()
        _write_exe(
            bin_dir / "coverage",
            "#!/usr/bin/env bash\n"
            "printf '%s\\n' \"$*\" >> COVERAGE_LOG\n"
            "if [[ \"$1\" != \"lcov\" ]]; then exit 0; fi\n"
            "printf '%s\\n' python >> ORDER\n"
            "if [[ -f STALE_LCOV || -f MISSING_LCOV ]]; then exit 0; fi\n"
            "out=target/coverage/python/lcov.info\n"
            "mkdir -p \"$(dirname \"$out\")\"\n"
            "printf '%s\\n' 'SF:lib/a.py' 'DA:1,1' 'end_of_record' > \"${out}\"\n"
            "exit 0\n",
        )
        _write_exe(
            bin_dir / "crapper",
            "#!/usr/bin/env bash\n"
            "printf '%s\\n' \"crapper $*\" >> ORDER\n"
            "mkdir -p .metrics\n"
            "printf '%s\\n' '{:entries []}' > .metrics/crap.edn\n"
            "exit 0\n",
        )
        _write_exe(
            bin_dir / "dryer",
            "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:candidates []}' > .metrics/dry.edn\nexit 0\n",
        )
        _write_exe(
            bin_dir / "mutator",
            "#!/usr/bin/env bash\n"
            "mkdir -p .metrics/mutate\n"
            "cat > .metrics/mutate/lib.a.edn <<'EDN'\n"
            + _mutator_edn(2, 0, 0, 2)
            + "EDN\nexit 0\n",
        )
        span_path = repo / "spans.json"
        span_path.write_text(
            json.dumps([{"namespace": "pkg", "name": "hot", "start_line": 1, "end_line": 2, "path": "lib/a.py"}]),
            encoding="utf-8",
        )
        env = _env(
            bin_dir,
            UB_GATES_ADVISORY="0",
            UB_GATES_BASE_REF="main",
            UB_GATES_COVERAGE_CMD=(
                "mkdir -p target/coverage/rust && "
                "printf '%s\\n' 'SF:unused.rs' 'DA:1,1' 'end_of_record' > target/coverage/rust/lcov.info && "
                "printf '%s\\n' rust >> ORDER"
            ),
            UB_GATES_MUTATOR_BUDGET_SECS="30",
            UB_GATES_MUTATOR_TEST_CMD="cargo test --locked",
            UB_SCOPE_FUNCTIONS_JSON=str(span_path),
            UB_GATES_OUTCOME_FILE=str(repo / ".metrics" / "ub-gates-outcome.txt"),
        )
        return _run_ci(repo, env)

    def test_missing_python_lcov_fails_closed_before_crapper(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._drive(tmp, "missing")
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            repo = Path(tmp)
            outcome = _outcome(repo)
            self.assertIn("outcome=tool_error", outcome)
            self.assertIn("python lcov missing or older than scored sources", text)
            self.assertTrue((repo / "target" / "coverage" / "rust" / "lcov.info").is_file())
            order = (repo / "ORDER").read_text(encoding="utf-8").splitlines()
            self.assertEqual(order, ["rust", "python"])
            self.assertNotIn("crapper exit", text)

    def test_python_lcov_older_than_source_fails_closed_before_crapper(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._drive(tmp, "stale")
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            repo = Path(tmp)
            self.assertIn("outcome=tool_error", _outcome(repo))
            self.assertIn("python lcov missing or older than scored sources", text)
            lcov = repo / "target" / "coverage" / "python" / "lcov.info"
            source = repo / "lib" / "a.py"
            self.assertLess(lcov.stat().st_mtime, source.stat().st_mtime)
            order = (repo / "ORDER").read_text(encoding="utf-8").splitlines()
            self.assertEqual(order, ["rust", "python"])
            self.assertNotIn("crapper exit", text)

    def test_fresh_python_lcov_is_scored_after_coverage(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._drive(tmp, "fresh")
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, text)
            repo = Path(tmp)
            self.assertIn("outcome=ran_clean", _outcome(repo))
            order = (repo / "ORDER").read_text(encoding="utf-8").splitlines()
            self.assertEqual(order[0], "rust")
            self.assertIn("python", order)
            self.assertLess(order.index("python"), order.index("crapper --threshold 10 --use-existing-coverage lib/a.py"))
            self.assertIn("--use-existing-coverage", order[-1])
            lcov = repo / "target" / "coverage" / "python" / "lcov.info"
            self.assertGreater(lcov.stat().st_size, 0)
            self.assertGreaterEqual(lcov.stat().st_mtime, (repo / "lib" / "a.py").stat().st_mtime)


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

    def test_ub_gates_job_blocks(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("\n    name: ub-gates\n", workflow)
        self.assertNotIn("ub-gates (advisory)", workflow)
        self.assertIn('advisory: "0"', workflow)
        self.assertNotIn("continue-on-error", workflow)

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
        self.assertNotIn("Eng is gated until a written re-review", readme)
        self.assertNotIn("sets advisory mode", readme)
        self.assertIn('blocking (`advisory: "0"`)', readme)
        self.assertIn("rustup component add llvm-tools-preview", readme)
        self.assertIn("`uv`", readme)
        self.assertIn("`cargo install`", readme)
        self.assertIn("20 minutes", readme)


def _crap_row(name: str, namespace: str, crap: object, complexity: int) -> str:
    token = "nil" if crap is None else str(crap)
    return (
        f'  {{:name "{name}", :namespace "{namespace}", '
        f":complexity {complexity}, :coverage 0.0, :crap {token}}}"
    )


def _crap_edn(rows: list[str]) -> str:
    return "{:entries [\n" + "\n".join(rows) + "\n]}\n"


class CrapperBaseJoinTests(unittest.TestCase):
    """Base rows join on file path and function name, and CRAP is compared to CRAP."""

    _FILE = "crates/strake/src/body/check.rs"
    _HEAD_NS = "strake::body::check"
    _BASE_NS = "crate::check"

    def _run(
        self,
        tmp: str,
        head_rows: list[str],
        base_rows: list[str],
        *,
        base_mode: str = "score",
        source_before: str | None = None,
        source_after: str | None = None,
        spans: list[dict[str, object]] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        repo = Path(tmp)
        _init_repo(repo)
        path = repo / self._FILE
        path.parent.mkdir(parents=True)
        before = source_before or "fn check_match() {\n    let x = 1;\n}\nfn check_if() {\n    let y = 1;\n}\n"
        after = source_after or "fn check_match() {\n    let x = 2;\n}\nfn check_if() {\n    let y = 2;\n}\n"
        path.write_text(before, encoding="utf-8")
        _git(repo, "add", self._FILE)
        _git(repo, "commit", "-m", "base")
        _git(repo, "checkout", "-b", "feat")
        path.write_text(after, encoding="utf-8")
        _git(repo, "add", self._FILE)
        _git(repo, "commit", "-m", "change")
        bin_dir = repo / "bin"
        bin_dir.mkdir()
        head_path = repo / "head.edn"
        base_path = repo / "base.edn"
        head_path.write_text(_crap_edn(head_rows), encoding="utf-8")
        base_path.write_text(_crap_edn(base_rows), encoding="utf-8")
        log_path = repo / "crapper.log"
        _write_exe(
            bin_dir / "crapper",
            "#!/usr/bin/env bash\n"
            f"printf '%s\\n' \"$PWD :: $*\" >> {shlex.quote(str(log_path))}\n"
            'if [[ " $* " == *" --no-coverage "* ]]; then\n'
            "  printf '%s\\n' 'unexpected --no-coverage' >&2\n"
            "  exit 3\n"
            "fi\n"
            'root="."\n'
            "prev=\n"
            'for arg in "$@"; do\n'
            '  if [[ "${prev}" == "--root" ]]; then root="${arg}"; fi\n'
            '  prev="${arg}"\n'
            "done\n"
            'mkdir -p "${root}/.metrics"\n'
            f"src={shlex.quote(str(head_path))}\n"
            'case "$PWD" in\n'
            f"  *ub-crap-base*) src={shlex.quote(str(base_path))} ;;\n"
            "esac\n"
            f"mode={shlex.quote(base_mode)}\n"
            'if [[ "$PWD" == *ub-crap-base* && "${mode}" == missing ]]; then\n'
            "  exit 0\n"
            "fi\n"
            'if [[ "$PWD" == *ub-crap-base* && "${mode}" == unreadable ]]; then\n'
            "  printf '%s\\n' 'not edn' > \"${root}/.metrics/crap.edn\"\n"
            "  exit 0\n"
            "fi\n"
            'cp "${src}" "${root}/.metrics/crap.edn"\n'
            "exit 0\n",
        )
        _write_exe(
            bin_dir / "dryer",
            "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:candidates []}' > .metrics/dry.edn\nexit 0\n",
        )
        _write_exe(bin_dir / "mutator", "#!/usr/bin/env bash\nexit 0\n")
        if spans is None:
            spans = [
                {
                    "namespace": self._HEAD_NS,
                    "name": "check_match",
                    "start_line": 1,
                    "end_line": 3,
                    "path": self._FILE,
                },
                {
                    "namespace": self._HEAD_NS,
                    "name": "check_if",
                    "start_line": 4,
                    "end_line": 6,
                    "path": self._FILE,
                },
            ]
        span_path = repo / "spans.json"
        span_path.write_text(json.dumps(spans), encoding="utf-8")
        env = _env(
            bin_dir,
            UB_GATES_ADVISORY="0",
            UB_GATES_BASE_REF="main",
            UB_GATES_MUTATOR_BUDGET_SECS="30",
            UB_SCOPE_FUNCTIONS_JSON=str(span_path),
            UB_GATES_OUTCOME_FILE=str(repo / ".metrics" / "ub-gates-outcome.txt"),
        )
        return _run_ci(repo, env)

    def _pair(self, name: str, base_crap: object, head_crap: object, base_cc: int, head_cc: int) -> tuple[str, str]:
        return (
            _crap_row(name, self._BASE_NS, base_crap, base_cc),
            _crap_row(name, self._HEAD_NS, head_crap, head_cc),
        )

    def test_risen_crap_still_fails_when_namespaces_differ(self) -> None:
        base_match, head_match = self._pair("check_match", 20.0, 28.0, 20, 28)
        base_if, head_if = self._pair("check_if", 19.0, 19.0, 19, 19)
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(tmp, [head_match, head_if], [base_match, base_if])
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("outcome=ran_with_findings", _outcome(Path(tmp)))
            self.assertIn(f"crapper findings {self._HEAD_NS} check_match crap=28.0", text)
            self.assertIn(f"crapper debt {self._HEAD_NS} check_if crap=19.0", text)
            self.assertIn(
                f"crapper join file={self._FILE} name=check_match namespace {self._BASE_NS} -> {self._HEAD_NS}",
                text,
            )
            self.assertNotIn("--no-coverage", text)

    def test_lower_or_equal_crap_is_debt_across_namespace_prefixes(self) -> None:
        # PR #46 replay: check_match 28 -> 26 and check_if 19 -> 19, joined on
        # the file path plus the function name. crate:: and strake::body:: are
        # the same functions.
        base_match, head_match = self._pair("check_match", 28.0, 26.0, 28, 26)
        base_if, head_if = self._pair("check_if", 19.0, 19.0, 19, 19)
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(tmp, [head_match, head_if], [base_match, base_if])
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, text)
            outcome = _outcome(Path(tmp))
            self.assertIn("outcome=ran_clean", outcome)
            self.assertIn("crapper_findings=0", outcome)
            self.assertIn("crapper_debt=2", outcome)
            self.assertIn(f"crapper debt {self._HEAD_NS} check_match crap=26.0", text)
            self.assertIn(f"crapper debt {self._HEAD_NS} check_if crap=19.0", text)
            self.assertNotIn("crapper findings", text)
            self.assertIn(
                f"crapper join file={self._FILE} name=check_match namespace {self._BASE_NS} -> {self._HEAD_NS}",
                text,
            )
            self.assertIn(
                f"crapper join file={self._FILE} name=check_if namespace {self._BASE_NS} -> {self._HEAD_NS}",
                text,
            )
            log = (Path(tmp) / "crapper.log").read_text(encoding="utf-8")
            self.assertIn("--use-existing-coverage", log)
            self.assertNotIn("--no-coverage", log)

    def test_new_function_over_threshold_fails(self) -> None:
        base_if, head_if = self._pair("check_if", 19.0, 19.0, 19, 19)
        fresh = _crap_row("fresh", self._HEAD_NS, 10.0, 10)
        spans = [
            {
                "namespace": self._HEAD_NS,
                "name": "check_if",
                "start_line": 1,
                "end_line": 3,
                "path": self._FILE,
            },
            {
                "namespace": self._HEAD_NS,
                "name": "fresh",
                "start_line": 4,
                "end_line": 6,
                "path": self._FILE,
            },
        ]
        before = "fn check_if() {\n    let y = 1;\n}\nfn fresh() {\n    let z = 1;\n}\n"
        after = "fn check_if() {\n    let y = 1;\n}\nfn fresh() {\n    let z = 2;\n}\n"
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(
                tmp,
                [head_if, fresh],
                [base_if],
                spans=spans,
                source_before=before,
                source_after=after,
            )
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("outcome=ran_with_findings", _outcome(Path(tmp)))
            self.assertIn(f"crapper findings {self._HEAD_NS} fresh crap=10.0", text)
            self.assertIn(f"crapper debt {self._HEAD_NS} check_if crap=19.0", text)

    def test_new_function_under_threshold_does_not_fail(self) -> None:
        base_if, head_if = self._pair("check_if", 19.0, 19.0, 19, 19)
        fresh = _crap_row("fresh", self._HEAD_NS, 9.0, 9)
        spans = [
            {
                "namespace": self._HEAD_NS,
                "name": "check_if",
                "start_line": 1,
                "end_line": 3,
                "path": self._FILE,
            },
            {
                "namespace": self._HEAD_NS,
                "name": "fresh",
                "start_line": 4,
                "end_line": 6,
                "path": self._FILE,
            },
        ]
        before = "fn check_if() {\n    let y = 1;\n}\nfn fresh() {\n    let z = 1;\n}\n"
        after = "fn check_if() {\n    let y = 1;\n}\nfn fresh() {\n    let z = 2;\n}\n"
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(
                tmp,
                [head_if, fresh],
                [base_if],
                spans=spans,
                source_before=before,
                source_after=after,
            )
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, text)
            outcome = _outcome(Path(tmp))
            self.assertIn("outcome=ran_clean", outcome)
            self.assertIn("crapper_findings=0", outcome)
            self.assertIn(f"crapper debt {self._HEAD_NS} check_if crap=19.0", text)
            self.assertNotIn("crapper findings", text)
            self.assertNotIn("fresh", text)

    def test_same_name_in_two_files_joins_on_the_file_path(self) -> None:
        repo_files = (
            ("crates/strake/src/body/check.rs", self._HEAD_NS, self._BASE_NS, 28.0, 26.0),
            ("crates/strake/src/body/lower.rs", "strake::body::lower", "crate::lower", 5.0, 40.0),
        )
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _init_repo(repo)
            spans = []
            before = "fn hot() {\n    let x = 1;\n}\n"
            after = "fn hot() {\n    let x = 2;\n}\n"
            for path, _head_ns, _base_ns, _base_crap, _head_crap in repo_files:
                file_path = repo / path
                file_path.parent.mkdir(parents=True, exist_ok=True)
                file_path.write_text(before, encoding="utf-8")
            _git(repo, "add", ".")
            _git(repo, "commit", "-m", "base")
            _git(repo, "checkout", "-b", "feat")
            for path, head_ns, _base_ns, _base_crap, _head_crap in repo_files:
                (repo / path).write_text(after, encoding="utf-8")
                spans.append(
                    {
                        "namespace": head_ns,
                        "name": "hot",
                        "start_line": 1,
                        "end_line": 3,
                        "path": path,
                    }
                )
            _git(repo, "add", ".")
            _git(repo, "commit", "-m", "change")
            bin_dir = repo / "bin"
            bin_dir.mkdir()
            case_lines = []
            for path, head_ns, base_ns, base_crap, head_crap in repo_files:
                head_edn = repo / (Path(path).stem + "-head.edn")
                base_edn = repo / (Path(path).stem + "-base.edn")
                head_edn.write_text(
                    _crap_edn([_crap_row("hot", head_ns, head_crap, int(head_crap))]),
                    encoding="utf-8",
                )
                base_edn.write_text(
                    _crap_edn([_crap_row("hot", base_ns, base_crap, int(base_crap))]),
                    encoding="utf-8",
                )
                case_lines.append(
                    f"  {shlex.quote(path)}) head={shlex.quote(str(head_edn))}; "
                    f"base={shlex.quote(str(base_edn))} ;;"
                )
            _write_exe(
                bin_dir / "crapper",
                "#!/usr/bin/env bash\n"
                'root="."\n'
                "prev=\n"
                "files=()\n"
                'for arg in "$@"; do\n'
                '  if [[ "${prev}" == "--root" ]]; then root="${arg}"; fi\n'
                '  if [[ "${prev}" != "--root" && "${prev}" != "--threshold" && "${arg}" != --* ]]; then\n'
                '    files+=("${arg}")\n'
                "  fi\n"
                '  prev="${arg}"\n'
                "done\n"
                'mkdir -p "${root}/.metrics"\n'
                'out="${root}/.metrics/crap.edn"\n'
                "printf '%s\\n' '{:entries [' > \"${out}\"\n"
                'for file in "${files[@]}"; do\n'
                "  head=\n"
                "  base=\n"
                '  case "$file" in\n'
                + "\n".join(case_lines)
                + "\n"
                "  esac\n"
                '  src="${head}"\n'
                '  case "$PWD" in\n'
                '    *ub-crap-base*) src="${base}" ;;\n'
                "  esac\n"
                "  sed -e '1d' -e '$d' \"${src}\" >> \"${out}\"\n"
                "done\n"
                "printf '%s\\n' ']}' >> \"${out}\"\n"
                "exit 0\n",
            )
            _write_exe(
                bin_dir / "dryer",
                "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:candidates []}' > .metrics/dry.edn\nexit 0\n",
            )
            _write_exe(bin_dir / "mutator", "#!/usr/bin/env bash\nexit 0\n")
            span_path = repo / "spans.json"
            span_path.write_text(json.dumps(spans), encoding="utf-8")
            env = _env(
                bin_dir,
                UB_GATES_ADVISORY="0",
                UB_GATES_BASE_REF="main",
                UB_GATES_MUTATOR_BUDGET_SECS="30",
                UB_SCOPE_FUNCTIONS_JSON=str(span_path),
                UB_GATES_OUTCOME_FILE=str(repo / ".metrics" / "ub-gates-outcome.txt"),
            )
            proc = _run_ci(repo, env)
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("outcome=ran_with_findings", _outcome(repo))
            self.assertIn("crapper debt strake::body::check hot crap=26.0", text)
            self.assertIn("crapper findings strake::body::lower hot crap=40.0", text)
            self.assertNotIn("crapper findings strake::body::check hot", text)
            self.assertIn(
                "crapper join file=crates/strake/src/body/check.rs name=hot namespace crate::check -> strake::body::check",
                text,
            )
            self.assertIn(
                "crapper join file=crates/strake/src/body/lower.rs name=hot namespace crate::lower -> strake::body::lower",
                text,
            )

    def test_missing_base_score_fails_closed(self) -> None:
        base_if, head_if = self._pair("check_if", 19.0, 19.0, 19, 19)
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(tmp, [head_if], [base_if], base_mode="missing")
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            outcome = _outcome(Path(tmp))
            self.assertIn("outcome=tool_error", outcome)
            self.assertNotIn("outcome=ran_clean", outcome)
            self.assertNotIn("outcome=skipped", outcome)
            self.assertIn("base crapper score failed", text)

    def test_unreadable_base_score_fails_closed(self) -> None:
        base_if, head_if = self._pair("check_if", 19.0, 19.0, 19, 19)
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(tmp, [head_if], [base_if], base_mode="unreadable")
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            outcome = _outcome(Path(tmp))
            self.assertIn("outcome=tool_error", outcome)
            self.assertNotIn("outcome=ran_clean", outcome)
            self.assertNotIn("outcome=skipped", outcome)
            self.assertNotIn("skipped_no_scope", text)
            self.assertNotIn("skipped_tools_missing", text)
            self.assertIn("align-base failed", text)
            self.assertIn("unreadable ", text)


_COVERAGE_SH = """#!/usr/bin/env bash
mkdir -p target/coverage/rust
mode=bare
if grep -q covered "$1"; then mode=full; fi
if grep -q partial "$1"; then mode=partial; fi
{
  printf '%s\\n' "SF:$1"
  if [[ "$mode" == full ]]; then
    printf '%s\\n' 'DA:2,1'
  elif [[ "$mode" == partial ]]; then
    printf '%s\\n' 'DA:1,1' 'DA:2,0' 'DA:3,0' 'DA:4,0' 'DA:5,0'
  else
    printf '%s\\n' 'DA:2,0'
  fi
  printf '%s\\n' end_of_record
} > target/coverage/rust/lcov.info
printf '%s\\n' "$PWD $mode" >> "$COVERAGE_LOG"
"""

_CRAP_FROM_LCOV = """#!/usr/bin/env python3
import os
import pathlib
import sys

root = pathlib.Path(".")
args = sys.argv[1:]
prev = ""
for arg in args:
    if prev == "--root":
        root = pathlib.Path(arg)
    prev = arg
infos = list(root.glob("target/coverage/**/lcov.info"))
infos.extend(root.glob("coverage/**/lcov.info"))
text = "\\n".join(path.read_text(encoding="utf-8") for path in infos)
hits = 0
total = 0
for line in text.splitlines():
    if not line.startswith("DA:"):
        continue
    total += 1
    if int(line.split(",")[1]) > 0:
        hits += 1
cov = 0.0
if total:
    cov = hits / total
cc = int(os.environ["UB_FIXTURE_CC"])
crap = cc * cc * (1.0 - cov) ** 3 + cc
name = os.environ.get("UB_FIXTURE_NAME", "hot")
metrics = root / ".metrics"
metrics.mkdir(parents=True, exist_ok=True)
(metrics / "crap.edn").write_text(
    "{:entries [\\n"
    f'  {{:name "{name}", :namespace "pkg", :complexity {cc}, :coverage {cov:.4f}, :crap {crap:.4f}}}\\n'
    "]}\\n",
    encoding="utf-8",
)
log = os.environ.get("UB_FIXTURE_LOG")
if log:
    with open(log, "a", encoding="utf-8") as handle:
        handle.write(f"{root.resolve()} hits={hits} total={total} crap={crap:.4f}\\n")
"""


class SeparateCoverageTests(unittest.TestCase):
    def _run(self, tmp: str, before: str, after: str, cc: str) -> subprocess.CompletedProcess[str]:
        repo = Path(tmp)
        _init_repo(repo)
        lib = repo / "lib"
        lib.mkdir()
        source = lib / "hot.py"
        source.write_text(before, encoding="utf-8")
        _git(repo, "add", "lib/hot.py")
        _git(repo, "commit", "-m", "base")
        _git(repo, "checkout", "-b", "feat")
        source.write_text(after, encoding="utf-8")
        _git(repo, "add", "lib/hot.py")
        _git(repo, "commit", "-m", "change")
        bin_dir = repo / "bin"
        bin_dir.mkdir()
        _write_exe(bin_dir / "crapper", _CRAP_FROM_LCOV)
        _write_exe(
            bin_dir / "dryer",
            "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:candidates []}' > .metrics/dry.edn\nexit 0\n",
        )
        _write_exe(bin_dir / "mutator", "#!/usr/bin/env bash\nexit 0\n")
        cover = bin_dir / "write-lcov"
        _write_exe(cover, _COVERAGE_SH)
        span = [{"namespace": "pkg", "name": "hot", "start_line": 1, "end_line": 2, "path": "lib/hot.py"}]
        (repo / "spans.json").write_text(json.dumps(span), encoding="utf-8")
        env = _env(
            bin_dir,
            UB_GATES_ADVISORY="0",
            UB_GATES_BASE_REF="main",
            UB_GATES_MUTATOR_BUDGET_SECS="30",
            UB_SCOPE_FUNCTIONS_JSON=str(repo / "spans.json"),
            UB_GATES_OUTCOME_FILE=str(repo / ".metrics" / "ub-gates-outcome.txt"),
            UB_GATES_COVERAGE_CMD=f"bash {cover} lib/hot.py",
            UB_FIXTURE_CC=cc,
            UB_FIXTURE_NAME="hot",
            UB_FIXTURE_LOG=str(repo / "crap-log.txt"),
            COVERAGE_LOG=str(repo / "coverage-log.txt"),
        )
        return _run_ci(repo, env)

    def test_lost_coverage_raises_crap_from_under_10(self) -> None:
        before = "def hot():\n    return 1  # covered\n"
        after = "def hot():\n    return 2  # bare\n"
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(tmp, before, after, "3")
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("outcome=ran_with_findings", _outcome(Path(tmp)))
            self.assertIn("crapper findings pkg hot crap=12.0", text)
            places = (Path(tmp) / "coverage-log.txt").read_text(encoding="utf-8")
            self.assertIn("full", places)
            self.assertIn("bare", places)
            self.assertIn("ub-crap-base", places)
            scores = (Path(tmp) / "crap-log.txt").read_text(encoding="utf-8")
            self.assertIn("crap=3.0", scores)
            self.assertIn("crap=12.0", scores)

    def test_coverage_drop_raises_crap_that_was_already_over_10(self) -> None:
        before = "def hot():\n    return 1  # partial\n"
        after = "def hot():\n    return 2  # bare\n"
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(tmp, before, after, "5")
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("outcome=ran_with_findings", _outcome(Path(tmp)))
            self.assertIn("crapper findings pkg hot", text)
            scores = (Path(tmp) / "crap-log.txt").read_text(encoding="utf-8")
            base = [line for line in scores.splitlines() if "ub-crap-base" in line][0]
            head = [line for line in scores.splitlines() if "ub-crap-base" not in line][0]
            base_crap = float(base.rsplit("crap=", 1)[1])
            head_crap = float(head.rsplit("crap=", 1)[1])
            self.assertGreater(base_crap, 10.0)
            self.assertGreater(head_crap, base_crap)
            self.assertIn("base coverage worktree", text)

    def test_failed_base_coverage_command_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _init_repo(repo)
            lib = repo / "lib"
            lib.mkdir()
            (lib / "hot.py").write_text("def hot():\n    return 1\n", encoding="utf-8")
            _git(repo, "add", "lib/hot.py")
            _git(repo, "commit", "-m", "base")
            _git(repo, "checkout", "-b", "feat")
            (lib / "hot.py").write_text("def hot():\n    return 2\n", encoding="utf-8")
            _git(repo, "add", "lib/hot.py")
            _git(repo, "commit", "-m", "change")
            bin_dir = repo / "bin"
            bin_dir.mkdir()
            _write_exe(
                bin_dir / "crapper",
                "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:entries []}' > .metrics/crap.edn\nexit 0\n",
            )
            _write_exe(
                bin_dir / "dryer",
                "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:candidates []}' > .metrics/dry.edn\nexit 0\n",
            )
            _write_exe(bin_dir / "mutator", "#!/usr/bin/env bash\nexit 0\n")
            span = [{"namespace": "pkg", "name": "hot", "start_line": 1, "end_line": 2, "path": "lib/hot.py"}]
            (repo / "spans.json").write_text(json.dumps(span), encoding="utf-8")
            env = _env(
                bin_dir,
                UB_GATES_ADVISORY="0",
                UB_GATES_BASE_REF="main",
                UB_GATES_MUTATOR_BUDGET_SECS="30",
                UB_SCOPE_FUNCTIONS_JSON=str(repo / "spans.json"),
                UB_GATES_OUTCOME_FILE=str(repo / ".metrics" / "ub-gates-outcome.txt"),
                UB_GATES_COVERAGE_CMD=(
                    "if [[ \"$PWD\" == *ub-crap-base* ]]; then exit 1; fi; "
                    "mkdir -p target/coverage/rust; "
                    "printf '%s\\n' 'SF:lib/hot.py' 'DA:2,1' end_of_record > target/coverage/rust/lcov.info"
                ),
            )
            proc = _run_ci(repo, env)
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("outcome=tool_error", _outcome(repo))
            self.assertIn("base coverage command failed", text)

    def test_base_lcov_missing_the_file_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _init_repo(repo)
            lib = repo / "lib"
            lib.mkdir()
            (lib / "hot.py").write_text("def hot():\n    return 1\n", encoding="utf-8")
            _git(repo, "add", "lib/hot.py")
            _git(repo, "commit", "-m", "base")
            _git(repo, "checkout", "-b", "feat")
            (lib / "hot.py").write_text("def hot():\n    return 2\n", encoding="utf-8")
            _git(repo, "add", "lib/hot.py")
            _git(repo, "commit", "-m", "change")
            bin_dir = repo / "bin"
            bin_dir.mkdir()
            _write_exe(
                bin_dir / "crapper",
                "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:entries []}' > .metrics/crap.edn\nexit 0\n",
            )
            _write_exe(
                bin_dir / "dryer",
                "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:candidates []}' > .metrics/dry.edn\nexit 0\n",
            )
            _write_exe(bin_dir / "mutator", "#!/usr/bin/env bash\nexit 0\n")
            span = [{"namespace": "pkg", "name": "hot", "start_line": 1, "end_line": 2, "path": "lib/hot.py"}]
            (repo / "spans.json").write_text(json.dumps(span), encoding="utf-8")
            env = _env(
                bin_dir,
                UB_GATES_ADVISORY="0",
                UB_GATES_BASE_REF="main",
                UB_GATES_MUTATOR_BUDGET_SECS="30",
                UB_SCOPE_FUNCTIONS_JSON=str(repo / "spans.json"),
                UB_GATES_OUTCOME_FILE=str(repo / ".metrics" / "ub-gates-outcome.txt"),
                UB_GATES_COVERAGE_CMD=(
                    "mkdir -p target/coverage/rust; "
                    "printf '%s\\n' 'SF:lib/other.py' 'DA:1,1' end_of_record > target/coverage/rust/lcov.info"
                ),
            )
            proc = _run_ci(repo, env)
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 1, text)
            self.assertIn("outcome=tool_error", _outcome(repo))
            self.assertIn("base lcov missing lib/hot.py", text)

    def test_suffix_dot_at_zero_is_that_extension(self) -> None:
        # A path whose only dot is the first character still has that suffix.
        # It is not the empty suffix, which would match every lcov record.
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _init_repo(repo)
            source = repo / ".rs"
            source.write_text("fn hot() {\n    return 1;\n}\n", encoding="utf-8")
            _git(repo, "add", "-f", ".rs")
            _git(repo, "commit", "-m", "base")
            _git(repo, "checkout", "-b", "feat")
            source.write_text("fn hot() {\n    return 2;\n}\n", encoding="utf-8")
            _git(repo, "add", "-f", ".rs")
            _git(repo, "commit", "-m", "change")
            bin_dir = repo / "bin"
            bin_dir.mkdir()
            _write_exe(
                bin_dir / "crapper",
                "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:entries []}' > .metrics/crap.edn\nexit 0\n",
            )
            _write_exe(
                bin_dir / "dryer",
                "#!/usr/bin/env bash\nmkdir -p .metrics\nprintf '%s\\n' '{:candidates []}' > .metrics/dry.edn\nexit 0\n",
            )
            _write_exe(bin_dir / "mutator", "#!/usr/bin/env bash\nexit 0\n")
            (repo / "spans.json").write_text("[]", encoding="utf-8")
            env = _env(
                bin_dir,
                UB_GATES_ADVISORY="0",
                UB_GATES_BASE_REF="main",
                UB_GATES_MUTATOR_BUDGET_SECS="30",
                UB_SCOPE_FUNCTIONS_JSON=str(repo / "spans.json"),
                UB_GATES_OUTCOME_FILE=str(repo / ".metrics" / "ub-gates-outcome.txt"),
                UB_GATES_COVERAGE_CMD=(
                    "mkdir -p target/coverage/rust; "
                    "printf '%s\\n' 'SF:lib/other.py' 'DA:1,1' end_of_record > target/coverage/rust/lcov.info"
                ),
            )
            proc = _run_ci(repo, env)
            text = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, text)
            self.assertIn("outcome=ran_clean", _outcome(repo))
            self.assertNotIn("base lcov missing", text)


class GateCommandTests(unittest.TestCase):
    """align-base and lcov-covers, invoked the way the gate invokes them."""

    def test_missing_args_fail(self) -> None:
        align = subprocess.run(
            ["python3", str(SCOPE_PY), "align-base"],
            capture_output=True,
            text=True,
        )
        self.assertEqual(align.returncode, 1, align.stderr)
        self.assertIn("align-base requires --manifest and --dest", align.stderr)
        lcov = subprocess.run(
            ["python3", str(SCOPE_PY), "lcov-covers"],
            capture_output=True,
            text=True,
        )
        self.assertEqual(lcov.returncode, 1, lcov.stderr)
        self.assertIn("lcov-covers requires --root and --list", lcov.stderr)

    def test_align_base_without_entries_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            base = root / "base.edn"
            head = root / "head.edn"
            dest = root / "out.edn"
            manifest = root / "manifest"
            base.write_text("not edn\n", encoding="utf-8")
            head.write_text("{:entries [\n]}\n", encoding="utf-8")
            manifest.write_text(f"lib/a.py\t{base}\t{head}\n", encoding="utf-8")
            proc = subprocess.run(
                [
                    "python3",
                    str(SCOPE_PY),
                    "align-base",
                    "--manifest",
                    str(manifest),
                    "--dest",
                    str(dest),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(proc.returncode, 1, proc.stderr)
            self.assertIn("unreadable " + str(base), proc.stderr)
            self.assertFalse(dest.exists())

    def test_lcov_covers_missing_list_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            missing = Path(tmp) / "no-such-list"
            proc = subprocess.run(
                [
                    "python3",
                    str(SCOPE_PY),
                    "lcov-covers",
                    "--root",
                    tmp,
                    "--list",
                    str(missing),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(proc.returncode, 1, proc.stderr)
            self.assertEqual(proc.stderr, "")


class LcovFreshTests(unittest.TestCase):
    """lcov-fresh, invoked the way run_ub_gates_ci.sh invokes it."""

    def _same_code(self, argv: list[str]) -> subprocess.CompletedProcess[str]:
        proc = subprocess.run(argv, capture_output=True, text=True)
        code = _load_scope().main(argv[2:])
        self.assertEqual(proc.returncode, code)
        return proc

    def _run(self, lcov: Path | None, *sources: Path) -> subprocess.CompletedProcess[str]:
        argv = ["python3", str(SCOPE_PY), "lcov-fresh"]
        if lcov is not None:
            argv.append(str(lcov))
        argv.extend(str(path) for path in sources)
        return self._same_code(argv)

    def _file(self, path: Path, body: str, stamp: int) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body, encoding="utf-8")
        os.utime(path, (stamp, stamp))

    def test_missing_args_fail(self) -> None:
        proc = self._same_code(["python3", str(SCOPE_PY), "lcov-fresh"])
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("lcov-fresh requires an lcov path", proc.stderr)

    def test_missing_lcov_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "lib" / "a.py"
            self._file(source, "x", 1_000_000)
            proc = self._run(root / "target" / "coverage" / "python" / "lcov.info", source)
            self.assertEqual(proc.returncode, 1, proc.stderr)

    def test_empty_lcov_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lcov = root / "target" / "coverage" / "python" / "lcov.info"
            source = root / "lib" / "a.py"
            self._file(lcov, "", 2_000_000)
            self._file(source, "x", 1_000_000)
            proc = self._run(lcov, source)
            self.assertEqual(proc.returncode, 1, proc.stderr)
            self.assertEqual(lcov.stat().st_size, 0)

    def test_older_lcov_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lcov = root / "target" / "coverage" / "python" / "lcov.info"
            source = root / "lib" / "a.py"
            self._file(lcov, "x", 1_000_000)
            self._file(source, "x", 2_000_000)
            proc = self._run(lcov, source)
            self.assertEqual(proc.returncode, 1, proc.stderr)
            self.assertLess(lcov.stat().st_mtime, source.stat().st_mtime)

    def test_missing_source_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lcov = root / "target" / "coverage" / "python" / "lcov.info"
            self._file(lcov, "x", 2_000_000)
            proc = self._run(lcov, root / "lib" / "missing.py")
            self.assertEqual(proc.returncode, 1, proc.stderr)

    def test_fresh_lcov_passes_when_not_older(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lcov = root / "target" / "coverage" / "python" / "lcov.info"
            older = root / "lib" / "old.py"
            same = root / "lib" / "same.py"
            self._file(lcov, "x", 2_000_000)
            self._file(older, "x", 1_000_000)
            self._file(same, "x", 2_000_000)
            proc = self._run(lcov, older, same)
            self.assertEqual(proc.returncode, 0, proc.stderr)
            self.assertGreaterEqual(lcov.stat().st_mtime, older.stat().st_mtime)
            self.assertEqual(lcov.stat().st_mtime, same.stat().st_mtime)

    def test_second_source_older_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lcov = root / "target" / "coverage" / "python" / "lcov.info"
            fresh = root / "lib" / "fresh.py"
            stale = root / "lib" / "stale.py"
            self._file(lcov, "x", 2_000_000)
            self._file(fresh, "x", 1_000_000)
            self._file(stale, "x", 3_000_000)
            proc = self._run(lcov, fresh, stale)
            self.assertEqual(proc.returncode, 1, proc.stderr)

    def test_unknown_command_names_lcov_fresh(self) -> None:
        proc = self._same_code(["python3", str(SCOPE_PY), "no-such"])
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("lcov-fresh", proc.stderr)


if __name__ == "__main__":
    unittest.main()
