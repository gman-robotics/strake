# Strake-A → strake-1 lowering

Lock: 2026-09-21, revised after adversarial REVISE. Agents write **Strake-A** (agent ANF
surface — still a provisional name pending a later rename pass). Backends eat
**`strake-1`** (portable SSA ISA — also provisional). The reference interpreter runs
`strake-1`. Golden tests are `.strake` files lowered then executed.

**Invariant 0 — A ≡ strake-1.** Same answers from A and from the `strake-1` it becomes.
The meaning of an A program is `interp(lower(A))`.

## Pipeline

```
.strake  →  parse A  →  check A  →  lower  →  strake-1  →  interp | wasm | cranelift
```

`strake fmt` runs on A. Hash / `use @blake3` is of the formatted A text (v0). Optional
later: hash the `strake-1` blob too.

## Names → SSA

Each A binding becomes one `%n`, numbered in domination order inside the function.
Parameters are `%0 .. %k-1`. A name is not reused (ANF) — see "L6 — one binding site per
name" below for the exact rule.

### L6 — one binding site per name

A source name has one binding site per function. A second `name =` for the same name is
check `E_SHADOW`. The formatter must not rename a rebind into legality — legality cannot
depend on `fmt`, and hash-after-fmt would otherwise change which programs check.

Binding sites are parameters, `name =`, `for` binders, match-arm binders, and loop
parameters — one site per name. Both `if` arms may contribute the same join name; that
is one binding site. A join name used after `if` and missing on one arm is `E_IF_PHI`.
A name bound outside an arm and assigned inside an arm is `E_SHADOW`.

Loop form:

```
acc0 = 0
loop acc = acc0
  next = add i64 acc 1
  continue acc = next
end
```

`acc` is the header phi. `continue` supplies back-edge operands and is not itself a
binding. Assigning `acc` in the body is `E_SHADOW`. `continue` must mention each loop
parameter exactly once (`E_TYPE` if not). Bare `loop` / `break` / `continue` with no
parameters carries no SSA values. Blocks close with `end`; indent is insignificant.

`%`-prefixed names introduced by lowering (below) are exempt from `E_SHADOW`. The `%`
prefix is reserved for the lowerer: user source may not spell a `%`-prefixed name (check
`E_PARSE`); every `%n`/`%i`/`%done`/`%slot`/`%v`/… below is a compiler temp, never a
name an agent can write.

## Blocks and phis

A has structured control only. Lowering introduces blocks.

### if

```
if cond
  t = ...
else
  t = ...
end
# t used after
```

```
  br_if cond then else
then:
  %t_then = ...
  br join
else:
  %t_else = ...
  br join
join:
  %t = phi %t_then %t_else
```

Both arms must produce the same names the join uses. Missing name in one arm is
`E_IF_PHI`.

`br_if cond then else` is **normative** `strake-1` syntax: a two-target conditional
branch. It is not Wasm `br_if` and does not lower to it directly (Wasm's `br_if` is a
single-target conditional branch out of a block); the Wasm backend re-encodes this form
when it lands.

### loop / break / continue

```
loop
  ...
  if done
    break
  end
  continue
end
```

```
header:
  %acc = phi %acc0 %acc_back
  br_if done exit body
body:
  ...
  br header
exit:
```

One loop header. `break` → `br exit`. `continue` → `br header`. Values live across
iterations are phis on the header. The compiler inserts them; the A author names the
bindings (loop parameters — see L6 above).

### for x in xs (normative desugar)

Every temporary the desugar introduces is `%`-prefixed and hygienic — never a user
binding, never reused as one. The single user binder is `x`. The match yields the
element into `x`; the user's loop body then runs (it may use `x`, and may itself bind
further names); `continue` runs last:

```
%n = list.len xs
loop %i = 0
  %done = ge i64 %i %n
  if %done
    break
  end
  %slot = list.get xs %i
  match %slot
    Ok %v ->
      x = %v
      ...body...
    Err _ ->
      trap E_LOWER
  end
  continue %i = add i64 %i 1
end
```

`match` here is a statement, not an expression: the `Ok` arm binds `x` from the payload
and then runs the user's loop body in place, ending at (but not including) `continue`;
the `Err` arm traps and never reaches `continue` for that iteration. `x` is one binding
site for the body (L6) — the desugar does not assign `x` a second time and does not
reuse it as a loop parameter. `continue %i = add i64 %i 1` matches the loop-parameter
form (see "L6" and "loop / break / continue" above): it mentions the sole loop
parameter `%i` exactly once. `E_LOWER` fires only if this desugar observes `list.get`
return `Err` while `%i < %n` — a lowering assertion, not a user-reachable case for any
`xs` whose `list.len` reports correctly. No iterator object in `strake-1`.

### match

```
match r
  Ok v  -> e1
  Err e -> e2
end
```

```
  %tag = sum.tag r
  br_table %tag b_ok b_err
b_ok:
  %v = sum.payload r
  ...
  br join
b_err:
  %e = sum.payload r
  ...
  br join
join:
  %out = phi ...
```

Match must be exhaustive on the declared sum. Non-exhaustive is `E_MATCH`.

## assert.eq desugar

`assert.eq` is not an opcode. It is legal only inside `test` and `property` bodies
(elsewhere `E_PARSE`). Desugar:

```
%eq = eq <ty> want got
if %eq
  ...
else
  diag E_TEST want got
  ret 1
end
```

On mismatch: emit diag `E_TEST` (`want`/`got`) and `ret 1` — this does not trap. The
lowerer inserts `ret 0` at the end of a `test`/`property` body that falls off the end.
Explicit `ret 77` inside a test or property lowers to result `E_REFUSED` (CLI 77); a
callee that merely returns the i64 `77` is not rewritten to a refusal — refusal is a
`ret 77` in the test/property body itself. A trap encountered while running a test stays
that trap (CLI 2) and is never rewritten to `E_TEST` or 77.

`forall n in LO..HI` inside `property` lowers to the loop-parameter form above, iterating
the static span `LO..HI` inclusive (see "Calls, tests, properties" below for the i128
check). A failing iteration returns 1 and stops — the first failing assert returns
immediately. `forall` over a list literal lowers as `for` + assert.

## Values in strake-1

Scalars are immediate SSA types.

Compound values are **handles** into a value heap owned by the interpreter / runtime —
not the linear region.

| A | strake-1 |
|---|---|
| record construct | `rec.new f1 f2 ...` |
| `x.field` | `rec.get x field_index` |
| `ok v` / `err e` | `sum.new tag payload` |
| `list.empty T` | `list.empty type_id` |
| `list.cons h t` | `list.cons h t` |
| `list.len` | `list.len` |
| `list.get` | `list.get` → `result` |
| `bytes.new hex` | `bytes.new` (`hex` immediate → `bytes`) |
| `bytes.len` | `bytes.len` |
| `bytes.get` | `bytes.get` → `result` |

This value heap is *not* host GC objects in the JS sense. It is a spec-owned acyclic
(v0) heap: records, sums, lists, bytes. Mutation of those values is forbidden. Regions
are a separate optional space for explicit `load`/`store`. **v0 leaks** — the heap is
append-only, never reclaimed, and carries no byte cap (see SPEC.md "Value heap" and
"Deferrals").

v0 lists/records/sums are acyclic. `E_CYCLE` stays reserved in the catalog and is not
emitted: v0 constructors cannot build a cycle without mutation, and there is no cycle
detector in MVP.

## Regions

Only appear if the module `import`s a region (`import region mem : region`; no bare
`region` grant). Page size is **65536** bytes. Default `pages 16` (1048576 bytes) unless
the import states another page count. Addresses are little-endian byte offsets.
Unaligned access is legal. The bounds check is done in **i128**: for an access at
`offset` with width `w` (widths: `i32`/`f32` = 4, `i64`/`f64` = 8, `bool` = 1),
`offset < 0` or `offset + w > pages * 65536` traps `E_OOB`. `load`/`store` without a
region import is check `E_REGION`. `load`/`store` of `unit`, lists, records, or sums is
`E_TYPE`. Lowering does not invent a region for lists.

## Fuel and call entry order

Each executed `strake-1` instruction costs 1 (including `const`, `phi`, `br`,
`list.cons`, `call`). An import call costs `1 + fuel_cost`, where `fuel_cost` is a host
`i64`, default 0, and must be ≥ 0. Fuel is checked *before* the instruction executes; if
remaining fuel is less than the cost, trap `E_FUEL` and do not execute it.

Call entry order, checked in this sequence:

1. Unresolved import → `E_IMPORT`; do not push a frame or charge fuel.
2. Else, if depth would exceed 1024 → `E_STACK`; do not charge fuel.
3. Else, charge fuel, then push the frame.

Frame 1024 is legal; frame 1025 traps. A host import occupies one frame until it
returns. Re-entrant host→guest calls push additional guest frames. Default
`fuel 100000`; one module-level directive may override; there is no per-function fuel.

## Calls, tests, properties

`call f(a,b)` → `call @f %a %b`.

`test name` becomes a `strake-1` export `test.name` that returns `i32` (0 pass, nonzero
fail code) plus a diag sidecar from `assert.eq` (see "assert.eq desugar" above).

`property` + `forall n: i64 in LO..HI` lowers to the loop-parameter form over the static
span, inclusive. Check-time: let `span = i128(HI) - i128(LO) + 1`. If `HI < LO`, or
`span` is ≤ 0 or `> 9223372036854775807` (`i64::MAX`), that is `E_FORALL`. For `i64`
`LO`/`HI` computed in `i128` (no overflow), `span ≤ 0` and `HI < LO` are the same
condition; both are named so the check reads directly off either form. Do not use
wrapping `sub`. `LO`/`HI` must be `i64` literals or module-level integer literal
bindings, else `E_FORALL`. There is **no** fixed iteration cap. A huge legal span runs
until module `fuel` traps.

`forall` over a list literal is `for` + assert.

## Holes

`--draft`: `hole T` → `trap E_HOLE` so execution fails loudly. Ship check rejects holes
before lower (check `E_HOLE`).

## Imports

An `import m.f : (T) -> U` becomes a `strake-1` import stub. No lowering of the callee.
Unknown callees (neither a local `fn` nor a declared `import`) are check `E_IMPORT`;
missing host binding or negative `fuel_cost` at instantiate time is trap `E_IMPORT`.

## Opcode sketch (strake-1 v0)

Control: `br br_if br_table call ret phi trap`
Int/float: `add sub mul div eq ne lt le gt ge`
Value heap: `rec.new rec.get sum.new sum.tag sum.payload list.empty list.cons list.len list.get bytes.new bytes.len bytes.get`
Region: `load store`
Const: `const.i32 const.i64 const.f32 const.f64 const.bool const.unit`

That is the closed const list — no ellipsis, no other const forms. The A spelling keeps
types on the operands (`add i64 a b`); `strake-1` mirrors that: `add` is one opcode with
types already on the operands, not an `i64.add`-style encoding. That is the closed ISA
card. A keywords not on this list must desugar before emit (`for`, `if`, `match`,
`assert.eq`, `ok`, `err`, field syntax, `not`).

## Invariants the lowerer must keep

0. **A ≡ strake-1** — same answers from A and from the `strake-1` it becomes;
   `interp(lower(A))` is the meaning of an A program.
1. Every A value used after a join is a phi (or check failed).
2. No unstructured jump that is not `if`/`loop`/`match`/`for`.
3. `list.get`/`bytes.get` OOB is `Err`, never region OOB.
4. Region OOB is `trap` (`E_OOB`); missing region import is check `E_REGION`.
5. Golden: `lower(A)` run on interp equals spec answers in the `.strake` tests.
6. Agents never need to read the emitted `%n` to debug; diags map back to A names/lines.

## What lowering is not

Not optimization. No constant fold required in v0. Cranelift/Wasm may optimize after.
Not a second language. If A cannot lower, it is not legal A.
