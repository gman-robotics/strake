"""Fail when a pull request body is missing a required section or a section is empty."""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

SECTIONS = (
    "Tier",
    "Models",
    "Linear",
    "Summary",
    "Gates",
    "Blast radius",
    "pstack",
    "Evidence",
)

_HEADING = re.compile(r"(?m)^## (.+)$")


def _matches(text: str) -> list[re.Match[str]]:
    return list(_HEADING.finditer(text))


def _title(match: re.Match[str]) -> str:
    return match.group(1).strip()


def _end(text: str, matches: list[re.Match[str]], index: int) -> int:
    if index + 1 < len(matches):
        return matches[index + 1].start()
    return len(text)


def _store(
    text: str,
    matches: list[re.Match[str]],
    index: int,
    found: dict[str, str],
) -> dict[str, str]:
    match = matches[index]
    found[_title(match)] = text[match.end() : _end(text, matches, index)].strip()
    return found


def _bodies(
    text: str,
    matches: list[re.Match[str]],
    index: int,
    found: dict[str, str],
) -> dict[str, str]:
    if index >= len(matches):
        return found
    return _bodies(text, matches, index + 1, _store(text, matches, index, found))


def _sections(text: str) -> dict[str, str]:
    return _bodies(text, _matches(text), 0, {})


def _missing(found: dict[str, str], name: str) -> str | None:
    if name not in found:
        return "missing " + name
    return None


def _blank(found: dict[str, str], name: str) -> bool:
    return found[name] == ""


def _blank_problem(found: dict[str, str], name: str) -> str | None:
    if _blank(found, name):
        return "empty " + name
    return None


def _empty(found: dict[str, str], name: str) -> str | None:
    if name not in found:
        return None
    return _blank_problem(found, name)


def _append(problems: list[str], item: str | None) -> list[str]:
    if item is not None:
        problems.append(item)
    return problems


def _visit(
    found: dict[str, str],
    index: int,
    problems: list[str],
    note,
) -> list[str]:
    if index >= len(SECTIONS):
        return problems
    return _visit(found, index + 1, _append(problems, note(found, SECTIONS[index])), note)


def check(text: str) -> list[str]:
    found = _sections(text)
    problems = _visit(found, 0, [], _missing)
    return _visit(found, 0, problems, _empty)


def _from_path(argv: list[str]) -> str | None:
    if len(argv) > 1:
        return Path(argv[1]).read_text(encoding="utf-8")
    return None


def _read_fallback() -> str:
    text = os.environ.get("PR_BODY")
    if text is not None:
        return text
    return sys.stdin.read()


def _read(argv: list[str]) -> str:
    text = _from_path(argv)
    if text is not None:
        return text
    return _read_fallback()


def _args(argv: list[str] | None) -> list[str]:
    if argv is None:
        return list(sys.argv)
    return list(argv)


def _report(problems: list[str]) -> int:
    if problems:
        sys.stderr.write("\n".join(problems) + "\n")
        return 1
    return 0


def main(argv: list[str] | None = None) -> int:
    return _report(check(_read(_args(argv))))


if __name__ == "__main__":
    sys.exit(main())
