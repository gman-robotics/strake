//! Append-only value heap for lists, bytes, and records.
//!
//! `Heap` and `Machine::sums` are both append-only: cells are pushed and never
//! updated or removed. Every heap edge (list element, cons tail, record field)
//! is a handle strictly smaller than the cell that stores it. A sum payload may
//! hold a heap handle; that handle was issued before the sum was allocated, and
//! the sum store is never rewritten. Constructors therefore cannot build a
//! cycle. `E_CYCLE` has no constructor.

use super::{ExecFault, Val, Value};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HeapVal {
    List(Vec<Val>),
    Bytes(Vec<u8>),
    Rec(Vec<Val>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Heap {
    cells: Vec<HeapVal>,
}

enum ListStep {
    Empty,
    Cons { head: Val, tail: u32 },
}

impl Heap {
    pub(super) fn new() -> Self {
        Self { cells: Vec::new() }
    }

    /// Push `v`. Rejects a handle edge that is not strictly smaller than the
    /// new cell's index (a forward edge). That fault is codeless: `E_CYCLE`
    /// has no constructor.
    pub(super) fn alloc(&mut self, v: HeapVal) -> Result<u32, ExecFault> {
        let idx = u32::try_from(self.cells.len())
            .map_err(|_| ExecFault::Codeless("heap overflow".into()))?;
        if edges(&v).iter().any(|e| *e >= idx) {
            return Err(ExecFault::Codeless("forward heap edge".into()));
        }
        self.cells.push(v);
        Ok(idx)
    }

    pub(super) fn list_empty(&mut self) -> Result<u32, ExecFault> {
        self.alloc(HeapVal::List(Vec::new()))
    }

    pub(super) fn list_cons(&mut self, head: Val, tail: u32) -> Result<u32, ExecFault> {
        self.require_list(tail)?;
        self.alloc(HeapVal::List(vec![head, Value::Handle(tail)]))
    }

    pub(super) fn list_len(&self, mut h: u32) -> Result<i64, ExecFault> {
        let mut n: i64 = 0;
        loop {
            match self.list_step(h)? {
                ListStep::Empty => return Ok(n),
                ListStep::Cons { tail, .. } => {
                    n = n
                        .checked_add(1)
                        .ok_or_else(|| ExecFault::Codeless("list len".into()))?;
                    h = tail;
                }
            }
        }
    }

    /// `Ok(None)` is out of range, including a negative index. The caller
    /// turns that into a result `Err`, never `E_OOB`.
    pub(super) fn list_get(&self, mut h: u32, idx: i64) -> Result<Option<Val>, ExecFault> {
        if idx < 0 {
            return Ok(None);
        }
        let mut left = idx;
        loop {
            match self.list_step(h)? {
                ListStep::Empty => return Ok(None),
                ListStep::Cons { head, tail } => {
                    if left == 0 {
                        return Ok(Some(head));
                    }
                    left -= 1;
                    h = tail;
                }
            }
        }
    }

    pub(super) fn bytes_new(&mut self, bytes: Vec<u8>) -> Result<u32, ExecFault> {
        self.alloc(HeapVal::Bytes(bytes))
    }

    pub(super) fn bytes_len(&self, h: u32) -> Result<i64, ExecFault> {
        match self.cell(h)? {
            HeapVal::Bytes(bs) => {
                i64::try_from(bs.len()).map_err(|_| ExecFault::Codeless("bytes len".into()))
            }
            _ => Err(ExecFault::Codeless("bytes expected".into())),
        }
    }

    /// In range, the byte is zero-extended into `0..=255`. Out of range is
    /// `Ok(None)` so the caller can return result `Err`.
    pub(super) fn bytes_get(&self, h: u32, idx: i64) -> Result<Option<i64>, ExecFault> {
        let HeapVal::Bytes(bs) = self.cell(h)? else {
            return Err(ExecFault::Codeless("bytes expected".into()));
        };
        let Some(i) = index_in(idx, bs.len()) else {
            return Ok(None);
        };
        Ok(Some(i64::from(bs[i])))
    }

    pub(super) fn rec_new(&mut self, fields: Vec<Val>) -> Result<u32, ExecFault> {
        self.alloc(HeapVal::Rec(fields))
    }

    pub(super) fn rec_get(&self, h: u32, field: usize) -> Result<Val, ExecFault> {
        match self.cell(h)? {
            HeapVal::Rec(fs) => fs
                .get(field)
                .cloned()
                .ok_or_else(|| ExecFault::Codeless("rec field".into())),
            _ => Err(ExecFault::Codeless("record expected".into())),
        }
    }

    /// Bytewise content equality. Non-byte handles are a codeless fault.
    pub(super) fn bytes_eq(&self, a: u32, b: u32) -> Result<bool, ExecFault> {
        let left = self.cell(a)?;
        let right = self.cell(b)?;
        match (left, right) {
            (HeapVal::Bytes(x), HeapVal::Bytes(y)) => Ok(x == y),
            _ => Err(ExecFault::Codeless("eq bytes".into())),
        }
    }

    fn require_list(&self, h: u32) -> Result<(), ExecFault> {
        match self.cell(h)? {
            HeapVal::List(_) => Ok(()),
            _ => Err(ExecFault::Codeless("list tail".into())),
        }
    }

    fn cell(&self, h: u32) -> Result<&HeapVal, ExecFault> {
        let i = usize::try_from(h).map_err(|_| ExecFault::Codeless("bad heap handle".into()))?;
        self.cells
            .get(i)
            .ok_or_else(|| ExecFault::Codeless("bad heap handle".into()))
    }

    fn list_step(&self, h: u32) -> Result<ListStep, ExecFault> {
        let HeapVal::List(xs) = self.cell(h)? else {
            return Err(ExecFault::Codeless("list expected".into()));
        };
        if xs.is_empty() {
            return Ok(ListStep::Empty);
        }
        if xs.len() != 2 {
            return Err(ExecFault::Codeless("list shape".into()));
        }
        let head = xs[0].clone();
        let Value::Handle(tail) = xs[1] else {
            return Err(ExecFault::Codeless("list tail edge".into()));
        };
        if tail >= h {
            return Err(ExecFault::Codeless("list tail edge".into()));
        }
        Ok(ListStep::Cons { head, tail })
    }
}

fn edges(v: &HeapVal) -> Vec<u32> {
    let mut out = Vec::new();
    match v {
        HeapVal::List(xs) | HeapVal::Rec(xs) => {
            for x in xs {
                if let Value::Handle(h) = x {
                    out.push(*h);
                }
            }
        }
        HeapVal::Bytes(_) => {}
    }
    out
}

fn index_in(idx: i64, len: usize) -> Option<usize> {
    let i = usize::try_from(idx).ok()?;
    if i >= len {
        None
    } else {
        Some(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::compile_module;
    use crate::body::diag::{Code, Span};
    use crate::body::host::EmptyHost;
    use crate::body::interp::{Builder, ExecFault, Machine, Value};
    use crate::body::syntax;

    fn span() -> Span {
        Span {
            file: "t".into(),
            start: 0,
            end: 1,
        }
    }

    #[test]
    fn list_get_out_of_range_is_tag_one() {
        let sp = span();
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let entry = 0;
        let empty = b.list_empty(entry, sp.clone());
        let zero = b.const_i64(entry, sp.clone(), 0);
        let neg = b.const_i64(entry, sp.clone(), -1);
        let g0 = b.list_get(entry, sp.clone(), empty, zero);
        let _g1 = b.list_get(entry, sp.clone(), empty, neg);
        let four = b.const_i64(entry, sp.clone(), 4);
        let one = b.list_cons(entry, sp.clone(), four, empty);
        let past = b.const_i64(entry, sp.clone(), 1);
        let _g2 = b.list_get(entry, sp.clone(), one, past);
        let tag = b.sum_tag(entry, sp.clone(), g0);
        b.ret(entry, sp.clone(), tag);
        let program = b.finish().expect("finish");
        let mut m = Machine::new(&program, program.fuel, &EmptyHost);
        let n = m.run_root_function(fidx).expect("run");
        assert_eq!(n, 1);
        assert_eq!(m.sums.len(), 3);
        for cell in &m.sums {
            assert_eq!(cell.tag, 1);
            assert_eq!(cell.payload, Value::Unit);
        }
    }

    #[test]
    fn bytes_get_in_range_is_tag_zero_byte_closed_range() {
        let sp = span();
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let entry = 0;
        let bytes = b.bytes_new(entry, sp.clone(), &[0xff, 0x00]);
        let i0 = b.const_i64(entry, sp.clone(), 0);
        let i1 = b.const_i64(entry, sp.clone(), 1);
        let g0 = b.bytes_get(entry, sp.clone(), bytes, i0);
        let _g1 = b.bytes_get(entry, sp.clone(), bytes, i1);
        let payload = b.sum_payload(entry, sp.clone(), g0);
        b.ret(entry, sp.clone(), payload);
        let program = b.finish().expect("finish");
        let mut m = Machine::new(&program, program.fuel, &EmptyHost);
        let n = m.run_root_function(fidx).expect("run");
        assert_eq!(n, 255);
        assert_eq!(m.sums.len(), 2);
        assert_eq!(m.sums[0].tag, 0);
        match m.sums[0].payload {
            Value::I64(v) => {
                assert_eq!(v, 255);
                assert!((0..=255).contains(&v));
            }
            _ => panic!("payload"),
        }
        assert_eq!(m.sums[1].tag, 0);
        match m.sums[1].payload {
            Value::I64(v) => {
                assert_eq!(v, 0);
                assert!((0..=255).contains(&v));
            }
            _ => panic!("payload"),
        }
    }

    #[test]
    fn rec_new_then_rec_get_returns_each_field() {
        let sp = span();
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let entry = 0;
        let a = b.const_i64(entry, sp.clone(), 3);
        let c = b.const_i64(entry, sp.clone(), 5);
        let d = b.const_i64(entry, sp.clone(), 7);
        let rec = b.rec_new(entry, sp.clone(), &[a, c, d]);
        let f0 = b.rec_get(entry, sp.clone(), rec, 0);
        let f1 = b.rec_get(entry, sp.clone(), rec, 1);
        let f2 = b.rec_get(entry, sp.clone(), rec, 2);
        let k1 = b.const_i64(entry, sp.clone(), 10_000);
        let k2 = b.const_i64(entry, sp.clone(), 100);
        let p0 = b.mul_i64(entry, sp.clone(), f0, k1);
        let p1 = b.mul_i64(entry, sp.clone(), f1, k2);
        let s0 = b.add_i64(entry, sp.clone(), p0, p1);
        let s1 = b.add_i64(entry, sp.clone(), s0, f2);
        b.ret(entry, sp.clone(), s1);
        let program = b.finish().expect("finish");
        let mut m = Machine::new(&program, program.fuel, &EmptyHost);
        let n = m.run_root_function(fidx).expect("run");
        assert_eq!(n, 30_507);
        assert_eq!(
            m.heap.cells[0],
            HeapVal::Rec(vec![Value::I64(3), Value::I64(5), Value::I64(7)])
        );
    }

    #[test]
    fn alloc_rejects_self_or_forward_edge() {
        let mut h = Heap::new();
        let e = h.list_empty().expect("empty");
        assert_eq!(e, 0);
        // A cell may not point at its own index or past it.
        assert!(h.alloc(HeapVal::Rec(vec![Value::Handle(1)])).is_err());
        assert!(h
            .alloc(HeapVal::List(vec![Value::I64(1), Value::Handle(5)]))
            .is_err());
        assert_eq!(h.cells.len(), 1, "a rejected alloc must not append");
        assert_eq!(
            h.alloc(HeapVal::Rec(vec![Value::Handle(0)]))
                .expect("back edge"),
            1
        );
    }

    #[test]
    fn heap_edges_are_strictly_smaller_including_sum_payload() {
        let sp = span();
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let entry = 0;
        let h0 = b.list_empty(entry, sp.clone());
        let seven = b.const_i64(entry, sp.clone(), 7);
        let h1 = b.list_cons(entry, sp.clone(), seven, h0);
        let h2 = b.list_cons(entry, sp.clone(), h1, h0);
        let s0 = b.sum_new(entry, sp.clone(), 0, h2);
        let _h3 = b.rec_new(entry, sp.clone(), &[h2, h1]);
        let _h4 = b.list_cons(entry, sp.clone(), s0, h2);
        b.ret_i64(entry, sp.clone(), 0);
        let program = b.finish().expect("finish");
        let mut m = Machine::new(&program, program.fuel, &EmptyHost);
        let n = m.run_root_function(fidx).expect("run");
        assert_eq!(n, 0);
        assert_eq!(m.heap.cells.len(), 5);
        assert!(matches!(m.heap.cells[0], HeapVal::List(ref xs) if xs.is_empty()));
        assert_eq!(
            m.heap.cells[1],
            HeapVal::List(vec![Value::I64(7), Value::Handle(0)])
        );
        assert_eq!(
            m.heap.cells[2],
            HeapVal::List(vec![Value::Handle(1), Value::Handle(0)])
        );
        assert_eq!(
            m.heap.cells[3],
            HeapVal::Rec(vec![Value::Handle(2), Value::Handle(1)])
        );
        assert_eq!(
            m.heap.cells[4],
            HeapVal::List(vec![Value::Sum(0), Value::Handle(2)])
        );
        for (i, cell) in m.heap.cells.iter().enumerate() {
            let own = u32::try_from(i).unwrap();
            for e in edges(cell) {
                assert!(e < own, "heap edge {e} is not smaller than {own}");
            }
        }
        assert_eq!(m.sums.len(), 1);
        assert_eq!(m.sums[0].tag, 0);
        assert_eq!(m.sums[0].payload, Value::Handle(2));
        assert!(matches!(m.heap.cells[2], HeapVal::List(_)));
    }

    #[test]
    fn list_empty_costs_one_fuel() {
        let sp = span();
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let entry = 0;
        let _e = b.list_empty(entry, sp.clone());
        b.ret_i64(entry, sp.clone(), 0);
        let program = b.finish().expect("finish");
        let mut ok = Machine::new(&program, 4, &EmptyHost);
        assert!(ok.run_root_function(fidx).is_ok());
        let mut short = Machine::new(&program, 3, &EmptyHost);
        assert!(matches!(
            short.run_root_function(fidx),
            Err(ExecFault::Trap {
                code: Code::EFuel,
                ..
            })
        ));
    }

    fn compile_run(src: &str, name: &str) -> i64 {
        let module = syntax::parse(src, "t.strake").expect("parse");
        let program = compile_module(&module, false).expect("compile");
        crate::body::interp::call_function_i64(&program, name, &[], &EmptyHost).expect("exec")
    }

    #[test]
    fn list_ops_lower_and_run() {
        let src = r#"strake 1

fn pack() -> i64
  e = list.empty i64
  t = list.cons 4 e
  xs = list.cons 9 t
  n = list.len xs
  g0 = list.get xs 0
  g1 = list.get xs 1
  g2 = list.get xs 2
  match g0
    ok v ->
      a = v
    err _ ->
      a = -1
  end
  match g1
    ok v ->
      b = v
    err _ ->
      b = -1
  end
  match g2
    ok v ->
      c = v
    err _ ->
      c = -1
  end
  s1 = add i64 a b
  s2 = add i64 s1 c
  s3 = add i64 s2 n
  ret s3
end

test t
  ret 0
end
"#;
        assert_eq!(compile_run(src, "pack"), 14);
    }

    #[test]
    fn eq_bytes_is_bytewise() {
        let src = r#"strake 1

fn one(a: bytes, b: bytes) -> i64
  c = eq bytes a b
  if c
    out = 1
  else
    out = 0
  end
  ret out
end

fn score() -> i64
  a = bytes.new x"ff00"
  b = bytes.new x"ff00"
  d = bytes.new x"00ff"
  x = call one(a, b)
  y = call one(a, d)
  z = add i64 x y
  ret z
end

test t
  ret 0
end
"#;
        assert_eq!(compile_run(src, "score"), 1);
    }

    #[test]
    fn record_literal_projects_declaration_order() {
        let src = r#"strake 1

type Point { x: i64, y: i64 }

fn ax(p: Point) -> i64
  v = p.x
  ret v
end

fn ay(p: Point) -> i64
  v = p.y
  ret v
end

fn pack() -> i64
  p = { y: 5, x: 4 }
  x = call ax(p)
  y = call ay(p)
  t = mul i64 x 100
  s = add i64 t y
  ret s
end

test t
  ret 0
end
"#;
        assert_eq!(compile_run(src, "pack"), 405);
    }

    #[test]
    fn callee_returns_list_and_caller_reads_len() {
        let src = r#"strake 1

fn id(xs: list i64) -> list i64
  ret xs
end

fn pack() -> i64
  e = list.empty i64
  t = list.cons 4 e
  xs = list.cons 9 t
  ys = call id(xs)
  n = list.len ys
  ret n
end

test t
  ret 0
end
"#;
        assert_eq!(compile_run(src, "pack"), 2);
    }

    #[test]
    fn callee_returns_record_and_caller_projects_field() {
        let src = r#"strake 1

type Point { x: i64, y: i64 }

fn id(p: Point) -> Point
  ret p
end

fn pack() -> i64
  p = { y: 5, x: 4 }
  q = call id(p)
  v = q.x
  ret v
end

test t
  ret 0
end
"#;
        assert_eq!(compile_run(src, "pack"), 4);
    }

    #[test]
    fn callee_returns_sum_and_caller_reads_payload() {
        let src = r#"strake 1

fn id(r: result i64 unit) -> result i64 unit
  ret r
end

fn pack() -> i64
  r = ok i64 5
  s = call id(r)
  match s
    ok v ->
      ret v
    err _ ->
      ret 0
  end
end

test t
  ret 0
end
"#;
        assert_eq!(compile_run(src, "pack"), 5);
    }

    #[test]
    fn record_copy_projects_field_at_distinct_index() {
        let src = r#"strake 1

type A { n: i64 }
type B { label: bytes, n: i64 }

fn pack() -> i64
  b = { label: x"00", n: 9 }
  c = b
  v = c.n
  ret v
end

test t
  ret 0
end
"#;
        assert_eq!(compile_run(src, "pack"), 9);
    }

    #[test]
    fn loop_param_projects_field_at_distinct_index() {
        let src = r#"strake 1

type A { n: i64 }
type B { label: bytes, n: i64 }

fn pack() -> i64
  b = { label: x"00", n: 8 }
  loop p = b
    v = p.n
    ret v
  end
end

test t
  ret 0
end
"#;
        assert_eq!(compile_run(src, "pack"), 8);
    }

    #[test]
    fn match_binder_projects_record_field() {
        let src = r#"strake 1

type A { n: i64 }
type B { label: bytes, n: i64 }

fn pick(r: result A unit) -> i64
  match r
    ok p ->
      v = p.n
      ret v
    err _ ->
      ret 0
  end
end

fn pack() -> i64
  a = { n: 6 }
  r = ok A a
  n = call pick(r)
  ret n
end

test t
  ret 0
end
"#;
        assert_eq!(compile_run(src, "pack"), 6);
    }

    #[test]
    fn if_join_projects_record_when_arms_agree() {
        let src = r#"strake 1

type A { n: i64 }
type B { label: bytes, n: i64 }

fn pick(c: bool) -> i64
  if c
    r = { n: 3 }
  else
    r = { n: 4 }
  end
  v = r.n
  ret v
end

fn pack() -> i64
  n = call pick(true)
  ret n
end

test t
  ret 0
end
"#;
        assert_eq!(compile_run(src, "pack"), 3);
    }

    #[test]
    fn match_join_projects_record_when_arms_agree() {
        let src = r#"strake 1

type A { n: i64 }
type B { label: bytes, n: i64 }

fn pick(c: bool) -> i64
  r = ok bool c
  match r
    ok _ ->
      out = { n: 3 }
    err _ ->
      out = { n: 4 }
  end
  v = out.n
  ret v
end

fn pack() -> i64
  n = call pick(true)
  ret n
end

test t
  ret 0
end
"#;
        assert_eq!(compile_run(src, "pack"), 3);
    }

    #[test]
    fn record_literal_runs_field_type_survivor() {
        let src = r#"strake 1

type A { x: i64 }
type B { x: bool }

fn pack() -> i64
  p = { x: true }
  c = p.x
  if c
    out = 1
  else
    out = 0
  end
  ret out
end

test t
  ret 0
end
"#;
        assert_eq!(compile_run(src, "pack"), 1);
    }

    #[test]
    fn rec_get_index_above_i32_max_is_not_negative() {
        let sp = span();
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let entry = 0;
        let one = b.const_i64(entry, sp.clone(), 1);
        let rec = b.rec_new(entry, sp.clone(), &[one]);
        let field = (i32::MAX as u32) + 1;
        let got = b.rec_get(entry, sp.clone(), rec, field);
        b.ret(entry, sp, got);
        let program = b.finish().expect("finish");
        let inst = program.functions[fidx].blocks[0]
            .insts
            .iter()
            .find(|i| i.op == crate::body::interp::Op::RecGet)
            .expect("rec.get");
        assert_eq!(inst.imm, crate::body::interp::ir::Imm::U32(field));
        let mut m = Machine::new(&program, program.fuel, &EmptyHost);
        let err = m
            .run_root_function(fidx)
            .expect_err("field past the record");
        match err {
            ExecFault::Codeless(msg) => assert_eq!(msg, "rec field"),
            other => panic!("unexpected {other:?}"),
        }
    }
}
