# Strake-A → strake-1 lowering

Lock: 2026-09-21. Agents write A. Backends eat `strake-1`.
The reference interpreter runs `strake-1`. Golden tests are `.strake` files lowered then executed.

## Pipeline

```
.strake  →  parse A  →  check A  →  lower  →  strake-1  →  interp | wasm | cranelift
```

`strake fmt` runs on A. Hash / `use @blake3` is of the formatted A text (v0). Optional later: hash the `strake-1` blob too.

## Names → SSA

Each A binding becomes one `%n`, numbered in domination order inside the function.
Parameters are `%0 .. %k-1`.
A name is not reused; `x = ...` then `x = ...` is two values (ANF). Formatter may rewrite the second to `x2` or keep shadowing as a check error — **v0 lock: no shadowing**. Recheck is an error `E_SHADOW`.

## Blocks and phis

A has structured control only. Lowering introduces blocks.

### if

```
if cond
  t = ...
else
  t = ...
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

Both arms must produce the same names the join uses. Missing name in one arm is `E_IF_PHI`.

### loop / break / continue

```
loop
  ...
  if done
    break
  continue
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

One loop header. `break` → `br exit`. `continue` → `br header`.
Values live across iterations are phis on the header. A compiler inserts them; A author names the bindings.

### for x in xs

Desugars in A, then lowers as loop:

```
i = 0
n = list.len xs
loop
  if ge i n
    break
  x = match list.get xs i
    Ok v  -> v
    Err _ -> trap          # cannot happen if i < n; trap is a lowering assert
  ...
  i = add i64 i 1
  continue
```

No iterator object in `strake-1`.

### match

```
match r
  Ok v  -> e1
  Err e -> e2
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

## Values in strake-1

Scalars are immediate SSA types.

Compound values are **handles** into a value heap owned by the interpreter / runtime — not the linear region.

| A | strake-1 |
|---|---|
| record construct | `rec.new f1 f2 ...` |
| `x.field` | `rec.get x field_index` |
| `ok v` / `err e` | `sum.new tag payload` |
| `list.empty T` | `list.empty type_id` |
| `list.cons h t` | `list.cons h t` |
| `list.len` | `list.len` |
| `list.get` | `list.get` → `result` |

This value heap is *not* host GC objects in the JS sense. It is a spec-owned acyclic (v0) heap: records, sums, lists, bytes. Mutation of those values is forbidden. Regions are a separate optional space for explicit `load`/`store`.

v0 lists/records/sums are acyclic. Cycles are `E_CYCLE` if we ever detect them at construct time (constructors cannot form cycles without mutation, so this is a future-proof check).

## Regions

Only appear if the module `import`s a region or declares `region`.
`load`/`store` outside a region are `E_REGION`.
Lowering does not invent a region for lists.

## Calls, tests, properties

`call f(a,b)` → `call @f %a %b`.

`test name` becomes a `strake-1` export `test.name` that returns `i32` (0 pass, nonzero fail code) plus a diag sidecar from `assert.eq`.

`property` + `forall n: i64 in LO..HI` unrolls in the lowerer to a loop from LO to HI inclusive (span cap: 10_000 iterations; larger is `E_FORALL`). Each iteration is `assert.eq`.

`forall` over a list literal is `for` + assert.

## Holes

`--draft`: `hole T` → `trap E_HOLE` so execution fails loudly.
Ship check rejects holes before lower.

## Imports

A `import m.f : (T) -> U` becomes a `strake-1` import stub. No lowering of the callee.

## Opcode sketch (strake-1 v0)

Control: `br br_if br_table call ret phi trap`
Int/float: `add sub mul div eq ne lt le gt ge`
Value heap: `rec.new rec.get sum.new sum.tag sum.payload list.empty list.cons list.len list.get`
Region: `load store`
Const: `const.i64 const.bool ...`

That is the closed ISA card. A keywords not on this list must desugar before emit (`for`, `if`, `match`, `assert.eq`, `ok`, `err`, field syntax).

## Invariants the lowerer must keep

1. Every A value used after a join is a phi (or check failed).
2. No unstructured jump that is not `if`/`loop`/`match`/`for`.
3. `list.get` OOB is `Err`, never region OOB.
4. Region OOB is `trap`.
5. Golden: `lower(A)` run on interp equals spec answers in the `.strake` tests.
6. Agents never need to read the emitted `%n` to debug; diags map back to A names/lines.

## What lowering is not

Not optimization. No constant fold required in v0. Cranelift/Wasm may optimize after.
Not a second language. If A cannot lower, it is not legal A.
