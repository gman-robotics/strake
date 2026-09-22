# Strake v0 spec (draft)

Status: **proposed locks** from the 2026-09-21 design thread unless marked already stamped.
Audience: agents author; humans read spec and traces.
Not JSC / Bun bytecode.

This document resolves the open questions about LLM-native use and folds them into the language.

---

## Already stamped

- Name Strake; text `.strake`; binary `strake-1`; CLI `strake check | run | test | build`; crate `strake`
- License MIT OR Apache-2.0
- Linear heap; explicit capability imports; WASI is a host adapter later
- Semantics SoT: spec + reference interpreter
- First portable backend: Wasm; first native: Cranelift
- Tests and structured JSON diagnostics are first-class
- Public repo: https://github.com/gman-robotics/strake

---

## Q1. Canonical artifact: text or tree?

**Resolution:** Two layers, one meaning.

- Authoring / debug surface: line-oriented `.strake` text.
- Ship / hash / agent-edit object: `strake-1` module (binary or canonical AST).
- `strake fmt` is **mandatory** before hash. Two modules that mean the same thing are byte-identical after format.
- Agents *may* edit via structured patch against the AST. They must not rely on raw whitespace.

Whitespace is never significant except as a token separator.

---

## Q2. Prefix-closed grammar for constrained decoding?

**Resolution:** Yes. Ship `grammar/strake.gbnf` (and an equivalent CFG) with the spec.

- Grammar is LL(1)-shaped and prefix-stable: a legal prefix has at least one legal completion.
- Line-oriented statements. No indentation-as-syntax.
- One spelling per construct (`fn`, `add`, `ret`, `call`, `test`, `assert.eq`). Aliases are formatter errors.
- Official token budget is measured on `cl100k_base` and recorded in `bench/token.lock` when tooling exists. Language changes that grow the golden suite token count need an explicit note.

---

## Q3. What is a hole?

**Resolution:** Drafts may contain typed holes. Ship modules may not.

```
%3 = hole i64
```

- `strake check --draft` accepts holes.
- `strake check` (ship) rejects holes.
- Constrained decoding / FIM fills holes without rewriting the rest of the module.

---

## Q4. Official repair object?

**Resolution:** Diagnostics are JSON. Human prose is optional commentary, not the interface.

```json
{
  "code": "E0127",
  "at": { "file": "add2.strake", "line": 6, "col": 3 },
  "msg": "unbound %3",
  "want": "i64",
  "got": null,
  "patch": [{ "op": "insert", "at": { "line": 6 }, "text": "%3 = add i64 %0 %1\n" }]
}
```

- Error codes are stable; they do not renumber.
- `strake fix` applies `patch` deterministically when present.
- Tests that fail use the same schema (`want` / `got`).

---

## Q5–Q6. Token cost and which tokenizer?

**Resolution:** `cl100k_base` is the v0 budget tokenizer. A language change that increases golden-suite tokens is a spec change, not a style change.

Prefer whole-token keywords (`add`, `ret`, `call`) over novel sigils. `%N` SSA names stay because they are short and unambiguous; measure before replacing them.

---

## Q7. Is `llm` / MCP a keyword?

**Resolution:** No. Model and tool use are **capability imports**, same as I/O.

```
import ai.complete : (prompt: bytes) -> bytes
```

The ISA does not grow an `llm` statement. Hosts that want Claude/MCP bind that import.

---

## Q8. Can the model refuse?

**Resolution:** Yes. A legal ship module may be only:

```
fn main() -> i32
  ret 77
```

Convention: `77` means author-refused (document in the error catalog). Grammar-constrained decoding must not make refusal unsayable. Do not force every prompt into a working algorithm.

---

## Q9. What may traces expose?

**Resolution:** Traces are capability-scoped.

- Default test trace: export results, trap code, fuel used, pages used.
- No raw host filesystem, credentials, or unredacted import payloads in the object that returns to the next model turn.
- Heap dumps are opt-in and never the default repair context.

---

## Q10. Machine-readable language pack?

**Resolution:** The repo root ships:

- `SPEC.md` (this file)
- `grammar/strake.gbnf` (when written)
- `errors/catalog.json` (stable codes)
- `llms.txt` pointing at those three plus the opcode table

One fetch should be enough to author legal Strake.

---

## Tests sit on boundaries

`test` blocks may only `call` exports and `assert` results or traps. They must not mention `%` temps or heap offsets. (Uncle Bob: tests on APIs, not internals.)

```
test add2.basic
  %r = call add2(3, 4)
  assert.eq %r 14
```

---

## Fuel and memory

Ship modules declare limits. Defaults if omitted: `fuel 100000`, `pages 16` (64KiB pages).

```
strake 1
fuel 10000
pages 4
```

Over-fuel or OOB store is a trap with a stable code, not UB.

---

## Determinism in tests

- No `env.clock` / `env.random` in `test` unless the host script provides a fixed value.
- Differential backends (interpreter vs Wasm) must match on the golden suite.

---

## Banned in v0

- `eval` of Strake strings
- Indentation-significant syntax
- Second human-only dialect
- Unicode opcodes
- Gherkin in the ISA
- Implicit coercions
- Host GC objects as values

---

## Feature veto

A v0 feature must help an agent **specify**, **check**, or **localize a failure** (Hoare: design, documentation, debugging). Otherwise it waits.

---

## Next implementation (not this commit)

1. Opcode table (~40 ops) in this spec
2. `grammar/strake.gbnf`
3. `errors/catalog.json`
4. Reference interpreter against golden `.strake` files — gated eng after this spec is accepted
