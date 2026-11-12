//! Reference interpreter for finished `Program`s.

mod heap;
pub(crate) mod ir;
mod op;

pub(crate) use ir::{Builder, FinishError};
pub use ir::{Inst, Program};

use heap::Heap;

use crate::body::diag::{
    render_scalar_bool, render_scalar_i64, trap_diag, Code, Diag, ExportOutcome, Kind, Span,
};
use crate::body::host::Host;
pub(crate) use op::{Op, ScalarTy, TrapImm};

use ir::{Imm, RetImm, VReg};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    I64(i64),
    I32(i32),
    Bool(bool),
    F64(f64),
    Unit,
    Sum(u32),
    /// Index into the append-only value heap.
    Handle(u32),
}

pub(crate) type Val = Value;

#[derive(Clone, Debug, PartialEq)]
struct SumCell {
    tag: u32,
    payload: Value,
}

pub(crate) struct Machine<'a> {
    program: &'a Program,
    host: &'a dyn Host,
    fuel: i64,
    frames: Vec<Frame>,
    root_return: Option<i64>,
    result_diags: Vec<Diag>,
    test_channel: bool,
    /// Successful call entries (host entry is call 1).
    pub(crate) call_entries: u64,
    /// Append-only, like `sums`. Heap edges are strictly smaller indices.
    /// A sum payload may hold a `Value::Handle` issued before that sum.
    /// Neither store is rewritten, so no cycle can exist. `E_CYCLE` has no constructor.
    heap: Heap,
    sums: Vec<SumCell>,
    /// Zero-filled region bytes; `Some` only with `import region mem : region`.
    region: Option<Vec<u8>>,
}

struct Frame {
    fn_idx: usize,
    block: usize,
    ip: usize,
    edge_pred: usize,
    regs: Vec<Option<Value>>,
    pending_call_dst: Option<VReg>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExecFault {
    Trap {
        code: Code,
        span: Span,
    },
    Codeless(String),
    /// Phase 1 §2.17 stop (bool `load` of a byte other than 0 or 1): exit 3,
    /// plain text, no catalog code, `status` not called.
    Halt(String),
}

/// Plain-text message for the §2.17 bool-load stop.
pub(crate) const BOOL_LOAD_HALT: &str = "bool load of a byte other than 0 or 1";

/// Region page size in bytes (SPEC "Traps and region").
pub(crate) const PAGE_BYTES: i128 = 65536;

impl<'a> Machine<'a> {
    pub(crate) fn new(program: &'a Program, fuel: i64, host: &'a dyn Host) -> Self {
        Self::new_with_channel(program, fuel, host, false)
    }

    pub(crate) fn new_test_channel(program: &'a Program, fuel: i64, host: &'a dyn Host) -> Self {
        Self::new_with_channel(program, fuel, host, true)
    }

    fn new_with_channel(
        program: &'a Program,
        fuel: i64,
        host: &'a dyn Host,
        test_channel: bool,
    ) -> Self {
        Self {
            program,
            host,
            fuel,
            frames: Vec::new(),
            root_return: None,
            result_diags: Vec::new(),
            test_channel,
            call_entries: 0,
            heap: Heap::new(),
            sums: Vec::new(),
            region: program
                .region_pages
                .map(|pages| vec![0u8; region_len(pages)]),
        }
    }

    fn alloc_sum(&mut self, tag: u32, payload: Value) -> Value {
        let idx = self.sums.len() as u32;
        self.sums.push(SumCell { tag, payload });
        Value::Sum(idx)
    }

    #[cfg(test)]
    pub(crate) fn test_alloc_sum_i64(&mut self, tag: u32, payload: i64) -> Value {
        self.alloc_sum(tag, Value::I64(payload))
    }

    fn sum_tag_of(&self, handle: u32) -> Result<u32, ExecFault> {
        self.sums
            .get(handle as usize)
            .map(|c| c.tag)
            .ok_or_else(|| ExecFault::Codeless("bad sum handle".into()))
    }

    fn sum_payload_of(&self, handle: u32) -> Result<Value, ExecFault> {
        self.sums
            .get(handle as usize)
            .map(|c| c.payload.clone())
            .ok_or_else(|| ExecFault::Codeless("bad sum handle".into()))
    }

    pub(crate) fn take_result_diags(&mut self) -> Vec<Diag> {
        let d = self.result_diags.clone();
        self.result_diags.clear();
        d
    }

    fn charge(&mut self, cost: i64, span: Span) -> Result<(), ExecFault> {
        if self.fuel < cost {
            return Err(ExecFault::Trap {
                code: Code::EFuel,
                span,
            });
        }
        self.fuel -= cost;
        Ok(())
    }

    pub(crate) fn run_root_function(&mut self, fn_idx: usize) -> Result<i64, ExecFault> {
        self.root_return = None;
        self.enter_function(
            fn_idx,
            &[],
            None,
            true,
            self.program.entry_charge_span.clone(),
        )?;
        self.run_loop()?;
        self.root_return
            .ok_or_else(|| ExecFault::Codeless("function returned no value".into()))
    }

    fn enter_function(
        &mut self,
        fn_idx: usize,
        args: &[Value],
        call_dst: Option<VReg>,
        charge_entry: bool,
        span: Span,
    ) -> Result<(), ExecFault> {
        if self.frames.len() >= 1024 {
            return Err(ExecFault::Trap {
                code: Code::EStack,
                span,
            });
        }
        if charge_entry {
            self.charge(1, span)?;
        }
        self.call_entries += 1;
        let func = &self.program.functions[fn_idx];
        let mut regs = vec![None; func.param_count.max(1)];
        for (i, arg) in args.iter().enumerate() {
            if i < func.param_count {
                ensure_reg(&mut regs, i);
                regs[i] = Some(arg.clone());
            }
        }
        self.frames.push(Frame {
            fn_idx,
            block: 0,
            ip: 0,
            edge_pred: 0,
            regs,
            pending_call_dst: call_dst,
        });
        Ok(())
    }

    fn run_loop(&mut self) -> Result<(), ExecFault> {
        while !self.frames.is_empty() {
            let frame_top = self.frames.len() - 1;
            let fn_idx = self.frames[frame_top].fn_idx;
            let block = self.frames[frame_top].block;
            let ip = self.frames[frame_top].ip;
            let func = &self.program.functions[fn_idx];
            if ip >= func.blocks[block].insts.len() {
                return Err(ExecFault::Codeless("fall off end of block".into()));
            }
            let inst = func.blocks[block].insts[ip].clone();
            self.frames[frame_top].ip += 1;
            if !self.exec_card_ext(&inst)? && !self.exec_region(&inst)? && self.exec_inst(&inst)? {
                break;
            }
        }
        Ok(())
    }

    /// Returns `true` when the root function has finished.
    #[cfg(test)]
    pub(crate) fn test_exec_inst(&mut self, inst: &Inst) -> Result<bool, ExecFault> {
        self.exec_inst(inst)
    }

    #[cfg(test)]
    pub(crate) fn test_exec_ge(&mut self, inst: &Inst) -> Result<(), ExecFault> {
        self.exec_card_ext(inst).map(|_| ())
    }

    /// One step as `run_loop` dispatches it, without the root-return check.
    #[cfg(test)]
    pub(crate) fn test_step(&mut self, inst: &Inst) -> Result<bool, ExecFault> {
        Ok(!self.exec_card_ext(inst)? && !self.exec_region(inst)? && self.exec_inst(inst)?)
    }

    fn exec_float_arith(&mut self, inst: &Inst) -> Result<bool, ExecFault> {
        if !f32_arith_inst(inst) {
            return Ok(false);
        }
        self.charge(1, inst.span.clone())?;
        let (a, b) = self.two_f64(inst)?;
        let z = f32_binop(inst.op, a as f32, b as f32);
        self.write_reg(inst.dst, Value::F64(f32_bits_to_f64(z.to_bits())))?;
        Ok(true)
    }

    fn exec_inst(&mut self, inst: &Inst) -> Result<bool, ExecFault> {
        let span = inst.span.clone();
        let skip_fuel = matches!(
            inst.op,
            Op::Trap if matches!(inst.imm, Imm::Trap(TrapImm::Fallthrough))
        );
        if inst.op != Op::Call && !skip_fuel {
            self.charge(1, span.clone())?;
        }
        match inst.op {
            Op::ConstI64 => {
                let v = match inst.imm {
                    Imm::I64(n) => Value::I64(n),
                    _ => return Err(ExecFault::Codeless("bad const".into())),
                };
                self.write_reg(inst.dst, v)?;
            }
            Op::ConstF64 => {
                let bits = match inst.imm {
                    Imm::F64(b) => b,
                    _ => return Err(ExecFault::Codeless("bad const".into())),
                };
                self.write_reg(inst.dst, Value::F64(f64::from_bits(bits)))?;
            }
            Op::ConstBool => {
                let v = match inst.imm {
                    Imm::Bool(b) => Value::Bool(b),
                    _ => return Err(ExecFault::Codeless("bad const".into())),
                };
                self.write_reg(inst.dst, v)?;
            }
            Op::Add => {
                let (a, b) = self.two_i64(inst)?;
                self.write_reg(inst.dst, Value::I64(a.wrapping_add(b)))?;
            }
            Op::Sub => {
                let (a, b) = self.two_i64(inst)?;
                self.write_reg(inst.dst, Value::I64(a.wrapping_sub(b)))?;
            }
            Op::Mul => {
                let (a, b) = self.two_i64(inst)?;
                self.write_reg(inst.dst, Value::I64(a.wrapping_mul(b)))?;
            }
            Op::Div => {
                if matches!(inst.imm, Imm::F64(_)) {
                    let (a, b) = self.two_f64(inst)?;
                    self.write_reg(inst.dst, Value::F64(a / b))?;
                } else if matches!(inst.imm, Imm::I32Width) {
                    let (a, b) = self.two_i32(inst)?;
                    self.write_reg(inst.dst, Value::I32(i32_div(a, b, span)?))?;
                } else {
                    let (a, b) = self.two_i64(inst)?;
                    self.write_reg(inst.dst, Value::I64(i64_div(a, b, span)?))?;
                }
            }
            Op::Eq => {
                let bool_cmp = matches!(inst.imm, Imm::Bool(true));
                let eq = if bool_cmp {
                    let a = self.read_reg(inst.args[0])?;
                    let b = self.read_reg(inst.args[1])?;
                    match (a, b) {
                        (Value::Bool(x), Value::Bool(y)) => x == y,
                        _ => return Err(ExecFault::Codeless("eq bool type".into())),
                    }
                } else {
                    let a = self.read_reg(inst.args[0])?;
                    let b = self.read_reg(inst.args[1])?;
                    match (&a, &b) {
                        (Value::Handle(x), Value::Handle(y)) => self.heap.bytes_eq(*x, *y)?,
                        _ => {
                            let ai = value_as_i64(&a)?;
                            let bi = value_as_i64(&b)?;
                            ai == bi
                        }
                    }
                };
                self.write_reg(inst.dst, Value::Bool(eq))?;
            }
            Op::Call => {
                let args: Vec<Value> = inst
                    .args
                    .iter()
                    .map(|r| self.read_reg(*r))
                    .collect::<Result<Vec<_>, _>>()?;
                let dst = inst.dst;
                match inst.imm {
                    Imm::Import(import_idx) => self.call_import(import_idx, &args, dst, span)?,
                    Imm::Fn(callee) => {
                        if self.frames.len() >= 1024 {
                            return Err(ExecFault::Trap {
                                code: Code::EStack,
                                span,
                            });
                        }
                        self.charge(1, span.clone())?;
                        self.enter_function(callee, &args, dst, false, span)?;
                    }
                    _ => return Err(ExecFault::Codeless("bad call".into())),
                }
            }
            Op::Ret => {
                let ret_imm = match &inst.imm {
                    Imm::Ret(r) => r.clone(),
                    _ => RetImm::Normal,
                };
                let v = self.read_reg(inst.args[0])?;
                let is_normal_ret = matches!(&ret_imm, RetImm::Normal);
                if let RetImm::TestFail {
                    want,
                    got,
                    diag_span,
                } = &ret_imm
                {
                    let want_v = self.read_reg(*want)?;
                    let got_v = self.read_reg(*got)?;
                    let (want_s, got_s) = render_test_fail_scalars(&want_v, &got_v)?;
                    self.result_diags.push(Diag {
                        code: Code::ETest,
                        kind: Kind::Result,
                        span: diag_span.clone(),
                        want: Some(want_s),
                        got: Some(got_s),
                        patch: None,
                    });
                }
                let call_dst = self.frames.last().unwrap().pending_call_dst;
                self.frames.pop();
                if let Some(dst) = call_dst {
                    if let Some(caller) = self.frames.last_mut() {
                        ensure_reg(&mut caller.regs, dst as usize);
                        caller.regs[dst as usize] = Some(v);
                    }
                } else if self.frames.is_empty() {
                    let ret_i64 = value_as_i64(&v)?;
                    if self.test_channel && ret_i64 == 77 && is_normal_ret {
                        self.result_diags.push(Diag {
                            code: Code::ERefused,
                            kind: Kind::Result,
                            span: inst.span.clone(),
                            want: None,
                            got: None,
                            patch: None,
                        });
                    }
                    self.root_return = Some(ret_i64);
                    return Ok(true);
                }
            }
            Op::Br => {
                let t = block_imm(&inst.imm)?;
                self.jump(t, 0)?;
            }
            Op::BrIf => {
                let cond = self.read_reg(inst.args[0])?;
                let targets = blocks_imm(&inst.imm)?;
                let t = match cond {
                    Value::Bool(true) => targets[0],
                    Value::Bool(false) => targets[1],
                    _ => return Err(ExecFault::Codeless("br_if needs bool".into())),
                };
                self.jump(t, 0)?;
            }
            Op::BrTable => {
                let tag_v = self.read_reg(inst.args[0])?;
                let tag = value_as_i64(&tag_v)?;
                let targets = blocks_imm(&inst.imm)?;
                let idx = tag as usize;
                if idx >= targets.len() {
                    return Err(ExecFault::Codeless("br_table out of range".into()));
                }
                self.jump(targets[idx], 0)?;
            }
            Op::SumNew => {
                let tag = match inst.imm {
                    Imm::I32(t) => t as u32,
                    _ => return Err(ExecFault::Codeless("bad sum.new tag".into())),
                };
                let payload = self.read_reg(inst.args[0])?;
                let handle = self.alloc_sum(tag, payload);
                self.write_reg(inst.dst, handle)?;
            }
            Op::SumTag => {
                let scrut = self.read_reg(inst.args[0])?;
                let handle = match scrut {
                    Value::Sum(h) => h,
                    _ => return Err(ExecFault::Codeless("sum.tag needs sum".into())),
                };
                let tag = self.sum_tag_of(handle)?;
                self.write_reg(inst.dst, Value::I64(tag as i64))?;
            }
            Op::SumPayload => {
                let scrut = self.read_reg(inst.args[0])?;
                let handle = match scrut {
                    Value::Sum(h) => h,
                    _ => return Err(ExecFault::Codeless("sum.payload needs sum".into())),
                };
                let payload = self.sum_payload_of(handle)?;
                self.write_reg(inst.dst, payload)?;
            }
            Op::Phi => {
                let preds = blocks_imm(&inst.imm)?;
                let pred = self.frames.last().unwrap().edge_pred;
                let idx = preds.iter().position(|&p| p == pred).ok_or_else(|| {
                    ExecFault::Codeless(format!("phi missing arm for block {pred}"))
                })?;
                let v = self.read_reg(inst.args[idx])?;
                self.write_reg(inst.dst, v)?;
            }
            Op::Trap => {
                let Imm::Trap(kind) = inst.imm else {
                    return Err(ExecFault::Codeless("bad trap imm".into()));
                };
                if matches!(kind, TrapImm::Fallthrough) {
                    return Err(ExecFault::Codeless("fall off end of block".into()));
                }
                let code = match kind {
                    TrapImm::Fuel => Code::EFuel,
                    TrapImm::Stack => Code::EStack,
                    TrapImm::Div0 => Code::EDiv0,
                    TrapImm::Hole => Code::EHole,
                    TrapImm::Lower => Code::ELower,
                    TrapImm::Import => Code::EImport,
                    TrapImm::Oob => Code::EOob,
                    TrapImm::User => Code::EType,
                    TrapImm::Fallthrough => Code::EType,
                };
                return Err(ExecFault::Trap { code, span });
            }
            Op::ListEmpty => {
                let h = self.heap.list_empty()?;
                self.write_reg(inst.dst, Value::Handle(h))?;
            }
            Op::ListCons => {
                let head = self.read_reg(inst.args[0])?;
                let tail = self.reg_handle(inst.args[1])?;
                let h = self.heap.list_cons(head, tail)?;
                self.write_reg(inst.dst, Value::Handle(h))?;
            }
            Op::ListLen => {
                let h = self.reg_handle(inst.args[0])?;
                let n = self.heap.list_len(h)?;
                self.write_reg(inst.dst, Value::I64(n))?;
            }
            Op::ListGet => {
                let h = self.reg_handle(inst.args[0])?;
                let idx = value_as_i64(&self.read_reg(inst.args[1])?)?;
                let sum = match self.heap.list_get(h, idx)? {
                    Some(v) => self.alloc_sum(0, v),
                    None => self.alloc_sum(1, Value::Unit),
                };
                self.write_reg(inst.dst, sum)?;
            }
            Op::BytesNew => {
                let bytes = match &inst.imm {
                    Imm::Bytes(b) => b.clone(),
                    _ => return Err(ExecFault::Codeless("bad bytes.new".into())),
                };
                let h = self.heap.bytes_new(bytes)?;
                self.write_reg(inst.dst, Value::Handle(h))?;
            }
            Op::BytesLen => {
                let h = self.reg_handle(inst.args[0])?;
                let n = self.heap.bytes_len(h)?;
                self.write_reg(inst.dst, Value::I64(n))?;
            }
            Op::BytesGet => {
                let h = self.reg_handle(inst.args[0])?;
                let idx = value_as_i64(&self.read_reg(inst.args[1])?)?;
                let sum = match self.heap.bytes_get(h, idx)? {
                    Some(n) => self.alloc_sum(0, Value::I64(n)),
                    None => self.alloc_sum(1, Value::Unit),
                };
                self.write_reg(inst.dst, sum)?;
            }
            Op::RecNew => {
                let mut fields = Vec::new();
                for arg in &inst.args {
                    fields.push(self.read_reg(*arg)?);
                }
                let h = self.heap.rec_new(fields)?;
                self.write_reg(inst.dst, Value::Handle(h))?;
            }
            Op::RecGet => {
                let h = self.reg_handle(inst.args[0])?;
                let field = match inst.imm {
                    Imm::U32(i) => i as usize,
                    _ => return Err(ExecFault::Codeless("bad rec.get".into())),
                };
                let v = self.heap.rec_get(h, field)?;
                self.write_reg(inst.dst, v)?;
            }
            _ => {
                return Err(ExecFault::Codeless(format!(
                    "unimplemented op {:?}",
                    inst.op
                )))
            }
        }
        Ok(false)
    }

    /// Card ops kept out of `exec_inst` (T3, GMA-479): `ne lt le gt ge` and
    /// `const.i32`/`const.f32`/`const.unit`. Fuel is charged before the op.
    /// Returns `false` when `inst` is not one of them.
    /// Import call entry (SPEC "Fuel"): an unbound import or a negative host
    /// `fuel_cost` is `E_IMPORT` with no push and no charge; then depth, then
    /// `1 + fuel_cost`.
    fn call_import(
        &mut self,
        import_idx: usize,
        args: &[Value],
        dst: Option<VReg>,
        span: Span,
    ) -> Result<(), ExecFault> {
        let stub = self
            .program
            .imports
            .get(import_idx)
            .ok_or_else(|| ExecFault::Codeless("bad import idx".into()))?;
        let host_fn = self.host.resolve_import(&stub.path);
        let fuel_cost = self.host.fuel_cost(&stub.path);
        if host_fn.is_none() || fuel_cost < 0 {
            return Err(ExecFault::Trap {
                code: Code::EImport,
                span,
            });
        }
        if self.frames.len() >= 1024 {
            return Err(ExecFault::Trap {
                code: Code::EStack,
                span,
            });
        }
        let cost = fuel_cost.saturating_add(1);
        self.charge(cost, span.clone())?;
        self.call_entries += 1;
        let host_fn = host_fn.unwrap();
        let result =
            host_fn(self, args).map_err(|_| ExecFault::Codeless("host import failed".into()))?;
        self.write_reg(dst, result)?;
        Ok(())
    }

    fn exec_card_ext(&mut self, inst: &Inst) -> Result<bool, ExecFault> {
        if self.exec_float_arith(inst)? {
            return Ok(true);
        }
        self.exec_cmp_or_const(inst)
    }

    fn exec_cmp_or_const(&mut self, inst: &Inst) -> Result<bool, ExecFault> {
        let is_const = matches!(inst.op, Op::ConstI32 | Op::ConstF32 | Op::ConstUnit);
        let is_cmp = matches!(inst.op, Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge)
            || self.is_float_eq(inst)?;
        if !is_const && !is_cmp {
            return Ok(false);
        }
        self.charge(1, inst.span.clone())?;
        let v = if is_const {
            const_narrow(inst)?
        } else {
            Value::Bool(self.cmp_holds(inst)?)
        };
        self.write_reg(inst.dst, v)?;
        Ok(true)
    }

    /// `load` / `store` (T5). Fuel first, then the i128 range check:
    /// `addr < 0` or `addr + width > pages * 65536` is `E_OOB`. Little-endian,
    /// unaligned is legal. Returns `false` when `inst` is not a region op.
    fn region_access(&self, inst: &Inst) -> Result<(ScalarTy, std::ops::Range<usize>), ExecFault> {
        let ty = scalar_imm(&inst.imm)?;
        let addr = value_as_i64(&self.read_reg(inst.args[0])?)?;
        Ok((ty, self.region_range(addr, ty, &inst.span)?))
    }

    fn exec_region(&mut self, inst: &Inst) -> Result<bool, ExecFault> {
        if !matches!(inst.op, Op::Load | Op::Store) {
            return Ok(false);
        }
        self.charge(1, inst.span.clone())?;
        let (ty, range) = self.region_access(inst)?;
        let out = if inst.op == Op::Load {
            decode_scalar(ty, &self.region_bytes()?[range])?
        } else {
            self.region_store(ty, range, inst.args[1])?
        };
        self.write_reg(inst.dst, out)?;
        Ok(true)
    }

    fn region_store(
        &mut self,
        ty: ScalarTy,
        range: std::ops::Range<usize>,
        value: VReg,
    ) -> Result<Value, ExecFault> {
        let bytes = encode_scalar(ty, &self.read_reg(value)?)?;
        self.region_bytes_mut()?[range].copy_from_slice(&bytes);
        Ok(Value::Unit)
    }

    fn region_range(
        &self,
        addr: i64,
        ty: ScalarTy,
        span: &Span,
    ) -> Result<std::ops::Range<usize>, ExecFault> {
        let len = self.region_bytes()?.len() as i128;
        let start = i128::from(addr);
        let end = start + ty.width();
        if start < 0 || end > len {
            return Err(ExecFault::Trap {
                code: Code::EOob,
                span: span.clone(),
            });
        }
        Ok(start as usize..end as usize)
    }

    fn region_bytes(&self) -> Result<&[u8], ExecFault> {
        self.region
            .as_deref()
            .ok_or_else(|| ExecFault::Codeless("no region".into()))
    }

    fn region_bytes_mut(&mut self) -> Result<&mut [u8], ExecFault> {
        self.region
            .as_deref_mut()
            .ok_or_else(|| ExecFault::Codeless("no region".into()))
    }

    fn cmp_holds(&self, inst: &Inst) -> Result<bool, ExecFault> {
        let a = self.read_reg(inst.args[0])?;
        let b = self.read_reg(inst.args[1])?;
        if matches!(inst.op, Op::Ne | Op::Eq) {
            if let Some(ne) = self.ne_by_identity(&a, &b)? {
                return Ok(ne == (inst.op == Op::Ne));
            }
        }
        Ok(ord_holds(inst.op, value_order(&a, &b)?))
    }

    /// `ne` cases that are not "not ordered-equal": SPEC "Float compare" makes
    /// f64 `ne` bitwise (NaN payload, +0 vs -0); bools and bytes compare by value.
    /// `eq` on two floats is bitwise (SPEC "Prelude"), the inverse of `ne`, so it
    /// runs on the compare card path instead of `exec_inst`'s integer `eq`.
    fn is_float_eq(&self, inst: &Inst) -> Result<bool, ExecFault> {
        if inst.op != Op::Eq || inst.args.len() != 2 {
            return Ok(false);
        }
        // An unreadable vreg is not a float eq here; `exec_inst` charges fuel
        // before it reads, so `E_FUEL` still comes first (SPEC "Fuel").
        let a = self.read_reg(inst.args[0]).ok();
        let b = self.read_reg(inst.args[1]).ok();
        Ok(matches!((a, b), (Some(Value::F64(_)), Some(Value::F64(_)))))
    }

    fn ne_by_identity(&self, a: &Value, b: &Value) -> Result<Option<bool>, ExecFault> {
        Ok(match (a, b) {
            (Value::F64(x), Value::F64(y)) => Some(x.to_bits() != y.to_bits()),
            (Value::Bool(x), Value::Bool(y)) => Some(x != y),
            (Value::Handle(x), Value::Handle(y)) => Some(!self.heap.bytes_eq(*x, *y)?),
            _ => None,
        })
    }

    fn jump(&mut self, block: usize, ip: usize) -> Result<(), ExecFault> {
        let frame = self.frames.last_mut().unwrap();
        let from = frame.block;
        frame.edge_pred = from;
        frame.block = block;
        frame.ip = ip;
        Ok(())
    }

    fn two_i64(&self, inst: &Inst) -> Result<(i64, i64), ExecFault> {
        let a = value_as_i64(&self.read_reg(inst.args[0])?)?;
        let b = value_as_i64(&self.read_reg(inst.args[1])?)?;
        Ok((a, b))
    }

    fn two_i32(&self, inst: &Inst) -> Result<(i32, i32), ExecFault> {
        let a = value_as_i32(&self.read_reg(inst.args[0])?)?;
        let b = value_as_i32(&self.read_reg(inst.args[1])?)?;
        Ok((a, b))
    }

    fn two_f64(&self, inst: &Inst) -> Result<(f64, f64), ExecFault> {
        let a = value_as_f64(&self.read_reg(inst.args[0])?)?;
        let b = value_as_f64(&self.read_reg(inst.args[1])?)?;
        Ok((a, b))
    }

    fn reg_handle(&self, r: VReg) -> Result<u32, ExecFault> {
        match self.read_reg(r)? {
            Value::Handle(h) => Ok(h),
            _ => Err(ExecFault::Codeless("expected heap handle".into())),
        }
    }

    fn read_reg(&self, r: VReg) -> Result<Value, ExecFault> {
        let frame = self
            .frames
            .last()
            .ok_or_else(|| ExecFault::Codeless("no frame".into()))?;
        let idx = r as usize;
        if idx < frame.regs.len() {
            frame.regs[idx]
                .clone()
                .ok_or_else(|| ExecFault::Codeless(format!("unset vreg %{r}")))
        } else {
            Err(ExecFault::Codeless(format!("undefined vreg %{r}")))
        }
    }

    fn write_reg(&mut self, dst: Option<VReg>, v: Value) -> Result<(), ExecFault> {
        let Some(r) = dst else {
            return Ok(());
        };
        let frame = self.frames.last_mut().unwrap();
        ensure_reg(&mut frame.regs, r as usize);
        frame.regs[r as usize] = Some(v);
        Ok(())
    }
}

macro_rules! checked_int_div {
    ($ty:ty, $a:expr, $b:expr, $span:expr) => {{
        let a = $a;
        let b = $b;
        if b == 0 || (a == <$ty>::MIN && b == -1) {
            Err(ExecFault::Trap {
                code: Code::EDiv0,
                span: $span,
            })
        } else {
            Ok(a / b)
        }
    }};
}

fn i64_div(a: i64, b: i64, span: Span) -> Result<i64, ExecFault> {
    checked_int_div!(i64, a, b, span)
}

fn i32_div(a: i32, b: i32, span: Span) -> Result<i32, ExecFault> {
    checked_int_div!(i32, a, b, span)
}

fn ensure_reg(regs: &mut Vec<Option<Value>>, idx: usize) {
    if regs.len() <= idx {
        regs.resize(idx + 1, None);
    }
}

fn render_test_fail_scalars(want: &Value, got: &Value) -> Result<(String, String), ExecFault> {
    Ok((render_scalar_for_test(want)?, render_scalar_for_test(got)?))
}

fn render_scalar_for_test(v: &Value) -> Result<String, ExecFault> {
    match v {
        Value::I64(n) => Ok(render_scalar_i64(*n)),
        Value::Bool(b) => Ok(render_scalar_bool(*b)),
        Value::I32(n) => Ok(render_scalar_i64(*n as i64)),
        // Shortest round-trip text (`1.5`, `NaN`, `inf`); no SPEC rendering rule yet.
        Value::F64(x) => Ok(format!("{x:?}")),
        _ => Err(ExecFault::Codeless("unsupported scalar in E_TEST".into())),
    }
}

fn exec_codeless(msg: &str) -> ExecFault {
    ExecFault::Codeless(msg.into())
}

macro_rules! value_or_err {
    ($v:expr, $msg:expr, $($pat:pat => $out:expr),+ $(,)?) => {
        match $v {
            $($pat => Ok($out),)+
            _ => Err(exec_codeless($msg)),
        }
    };
}

macro_rules! imm_or_err {
    ($imm:expr, $msg:expr, $($pat:pat => $out:expr),+ $(,)?) => {
        match $imm {
            $($pat => Ok($out),)+
            _ => Err(exec_codeless($msg)),
        }
    };
}

fn f32_arith_inst(inst: &Inst) -> bool {
    matches!(inst.imm, Imm::F32(0)) && matches!(inst.op, Op::Add | Op::Sub | Op::Mul | Op::Div)
}

fn f32_binop(op: Op, x: f32, y: f32) -> f32 {
    match op {
        Op::Sub => x - y,
        Op::Mul => x * y,
        Op::Div => x / y,
        _ => x + y,
    }
}

fn value_as_i64(v: &Value) -> Result<i64, ExecFault> {
    match v {
        Value::I64(n) => Ok(*n),
        Value::I32(n) => Ok(*n as i64),
        Value::Sum(_) => Err(exec_codeless("expected i64")),
        _ => Err(exec_codeless("expected i64")),
    }
}

fn value_as_i32(v: &Value) -> Result<i32, ExecFault> {
    value_or_err!(v, "expected i32",
        Value::I32(n) => *n,
        Value::I64(n) => *n as i32,
    )
}

/// Little-endian bytes for `store`. `f32` narrows the F64 value cell.
fn encode_scalar(ty: ScalarTy, v: &Value) -> Result<Vec<u8>, ExecFault> {
    Ok(match ty {
        ScalarTy::I32 => value_as_i32(v)?.to_le_bytes().to_vec(),
        ScalarTy::I64 => value_as_i64(v)?.to_le_bytes().to_vec(),
        ScalarTy::F32 => f64_to_f32_bits(value_as_f64(v)?).to_le_bytes().to_vec(),
        ScalarTy::F64 => value_as_f64(v)?.to_le_bytes().to_vec(),
        ScalarTy::Bool => vec![u8::from(value_as_bool(v)?)],
    })
}

fn scalar_imm(imm: &Imm) -> Result<ScalarTy, ExecFault> {
    imm_or_err!(imm, "bad region imm", Imm::Scalar(ty) => *ty)
}

fn value_as_bool(v: &Value) -> Result<bool, ExecFault> {
    value_or_err!(v, "store bool needs bool", Value::Bool(b) => *b)
}

/// Value for `load`. A bool byte other than 0 or 1 is the §2.17 stop.
/// Narrow to f32 bits. A NaN keeps its sign and the top 23 payload bits, so a
/// NaN that came from an f32 survives the store bit for bit (float `eq` is bitwise).
fn f64_to_f32_bits(x: f64) -> u32 {
    if !x.is_nan() {
        return (x as f32).to_bits();
    }
    let b = x.to_bits();
    let sign = ((b >> 63) as u32) << 31;
    let payload = ((b >> 29) as u32) & 0x007F_FFFF;
    sign | 0x7F80_0000 | payload.max(1)
}

/// Widen f32 bits to f64. A NaN keeps its sign and payload bits, the inverse
/// of `f64_to_f32_bits`.
fn f32_bits_to_f64(bits: u32) -> f64 {
    let x = f32::from_bits(bits);
    if !x.is_nan() {
        return f64::from(x);
    }
    let sign = u64::from(bits >> 31) << 63;
    let payload = u64::from(bits & 0x007F_FFFF) << 29;
    f64::from_bits(sign | 0x7FF0_0000_0000_0000 | payload)
}

fn decode_scalar(ty: ScalarTy, b: &[u8]) -> Result<Value, ExecFault> {
    let four = |b: &[u8]| [b[0], b[1], b[2], b[3]];
    let eight = |b: &[u8]| [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
    Ok(match ty {
        ScalarTy::I32 => Value::I32(i32::from_le_bytes(four(b))),
        ScalarTy::I64 => Value::I64(i64::from_le_bytes(eight(b))),
        ScalarTy::F32 => Value::F64(f32_bits_to_f64(u32::from_le_bytes(four(b)))),
        ScalarTy::F64 => Value::F64(f64::from_le_bytes(eight(b))),
        ScalarTy::Bool => match b[0] {
            0 => Value::Bool(false),
            1 => Value::Bool(true),
            _ => return Err(ExecFault::Halt(BOOL_LOAD_HALT.into())),
        },
    })
}

fn value_as_f64(v: &Value) -> Result<f64, ExecFault> {
    value_or_err!(v, "expected f64", Value::F64(n) => *n)
}

fn block_imm(imm: &Imm) -> Result<usize, ExecFault> {
    imm_or_err!(imm, "expected block imm", Imm::Block(b) => *b)
}

fn blocks_imm(imm: &Imm) -> Result<Vec<usize>, ExecFault> {
    imm_or_err!(imm, "expected blocks imm", Imm::Blocks(b) => b.clone())
}

/// One test/property export run: status input plus any result-channel diags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TestExportRun {
    pub outcome: ExportOutcome,
    pub diags: Vec<Diag>,
    /// §2.17 stop message; the CLI exits 3 with it before calling `status`.
    pub halt: Option<String>,
}

/// Order for `lt le gt ge` (and non-identity `ne`): IEEE for f64, signed for
/// integers. Bools have no order in SPEC.
fn value_order(a: &Value, b: &Value) -> Result<Option<std::cmp::Ordering>, ExecFault> {
    match (a, b) {
        (Value::F64(x), Value::F64(y)) => Ok(x.partial_cmp(y)),
        (Value::Bool(_), _) | (_, Value::Bool(_)) => {
            Err(ExecFault::Codeless("ordered compare on bool".into()))
        }
        _ => Ok(Some(value_as_i64(a)?.cmp(&value_as_i64(b)?))),
    }
}

/// `None` is an unordered float pair (NaN): every ordered compare is false.
fn ord_holds(op: Op, ord: Option<std::cmp::Ordering>) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};
    match op {
        Op::Ne => ord != Some(Equal),
        Op::Lt => ord == Some(Less),
        Op::Le => matches!(ord, Some(Less | Equal)),
        Op::Gt => ord == Some(Greater),
        Op::Ge => matches!(ord, Some(Greater | Equal)),
        _ => false,
    }
}

/// `const.f32` widens to the `F64` value cell; every f32 is exact in f64.
fn const_narrow(inst: &Inst) -> Result<Value, ExecFault> {
    match (inst.op, &inst.imm) {
        (Op::ConstI32, Imm::I32(n)) => Ok(Value::I32(*n)),
        (Op::ConstF32, Imm::F32(bits)) => Ok(Value::F64(f32_bits_to_f64(*bits))),
        (Op::ConstUnit, Imm::None) => Ok(Value::Unit),
        _ => Err(ExecFault::Codeless("bad const".into())),
    }
}

pub(crate) fn run_test_exports(program: &Program, host: &dyn Host) -> Vec<TestExportRun> {
    #[cfg(test)]
    {
        crate::note_exec_on_this_thread();
    }
    if let Some(fault) = instantiate_fault(program, host) {
        return vec![fault];
    }
    let runs = program.exports.iter().filter(|e| e.is_test).map(|e| {
        let mut m = Machine::new_test_channel(program, program.fuel, host);
        let (outcome, trap_diag) = match m.run_root_function(e.fn_idx) {
            Ok(ret) => (
                ExportOutcome {
                    trapped: false,
                    ret,
                },
                None,
            ),
            Err(ExecFault::Trap { code, span }) => (
                ExportOutcome {
                    trapped: true,
                    ret: 0,
                },
                Some(trap_diag(code, span)),
            ),
            Err(ExecFault::Codeless(_)) => (
                ExportOutcome {
                    trapped: true,
                    ret: 0,
                },
                None,
            ),
            Err(ExecFault::Halt(msg)) => {
                return TestExportRun {
                    outcome: ExportOutcome {
                        trapped: true,
                        ret: 0,
                    },
                    diags: Vec::new(),
                    halt: Some(msg),
                };
            }
        };
        let mut diags = m.take_result_diags();
        if let Some(td) = trap_diag {
            diags.push(td);
        }
        TestExportRun {
            outcome,
            diags,
            halt: None,
        }
    });
    // SPEC §2.17: a halt stops the process; later test exports never run.
    let mut out = Vec::new();
    for run in runs {
        let halted = run.halt.is_some();
        out.push(run);
        if halted {
            break;
        }
    }
    out
}

/// Region byte length for `pages`. A negative or unrepresentable size is a
/// host allocation failure, so abort instead of wrapping to a short buffer.
fn region_len(pages: i64) -> usize {
    i128::from(pages)
        .checked_mul(PAGE_BYTES)
        .filter(|n| *n >= 0)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or_else(|| panic!("region of {pages} pages cannot be allocated"))
}

/// Instantiate (SPEC "Traps and region"): every `import` fn must have a host
/// binding. A missing one is an `E_IMPORT` trap before any export runs, so
/// the host records zero calls and no fuel is charged.
fn instantiate_fault(program: &Program, host: &dyn Host) -> Option<TestExportRun> {
    let miss = program
        .imports
        .iter()
        .find(|stub| host.resolve_import(&stub.path).is_none() || host.fuel_cost(&stub.path) < 0)?;
    Some(TestExportRun {
        outcome: ExportOutcome {
            trapped: true,
            ret: 0,
        },
        diags: vec![trap_diag(Code::EImport, miss.span.clone())],
        halt: None,
    })
}

#[allow(dead_code)]
pub(crate) fn call_function_i64(
    program: &Program,
    name: &str,
    args: &[i64],
    host: &dyn Host,
) -> Result<i64, ExecFault> {
    let fn_idx = program
        .functions
        .iter()
        .position(|f| f.name == name)
        .ok_or_else(|| ExecFault::Codeless(format!("no function {name}")))?;
    let vargs: Vec<Value> = args.iter().map(|&n| Value::I64(n)).collect();
    let mut m = Machine::new(program, program.fuel, host);
    m.root_return = None;
    let span = Span {
        file: String::new(),
        start: 0,
        end: 0,
    };
    m.enter_function(fn_idx, &vargs, None, true, span)?;
    m.run_loop()?;
    m.root_return
        .ok_or_else(|| ExecFault::Codeless("function returned no value".into()))
}

/// One-element `for` whose body jumps to the exit block `for_` passes in.
/// The result is the value returned from that block.
#[cfg(test)]
fn for_body_leaves_through_exit(
    entry: usize,
    span_at: u32,
    fuel: i64,
    result: i64,
) -> Result<i64, ExecFault> {
    use crate::body::host::EmptyHost;

    let span = Span {
        file: String::new(),
        start: span_at,
        end: span_at,
    };
    let mut b = Builder::new();
    let fidx = b.begin_function("f", entry);
    let empty = b.list_empty(entry, span.clone());
    let seven = b.const_i64(entry, span.clone(), 7);
    let xs = b.list_cons(entry, span.clone(), seven, empty);
    let exit = b
        .for_(entry, span.clone(), xs, |b, ok_arm, _elt, exit| {
            if exit != entry {
                b.jump(ok_arm, span.clone(), exit);
            } else {
                b.trap_user(ok_arm, span.clone());
            }
            Ok::<usize, ()>(ok_arm)
        })
        .expect("for_");
    b.ret_i64(exit, span, result);
    let program = b.finish().expect("finish");
    let mut machine = Machine::new(&program, fuel, &EmptyHost);
    machine.run_root_function(fidx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::compile_module;
    use crate::body::host::EmptyHost;
    use crate::body::syntax;

    #[test]
    fn user_sum_red_variant_executes() {
        let src = r#"strake 1

type Color { Blue unit | Red i64 }

fn pick(c: Color) -> i64
  match c
    Blue _ ->
      out = 0
    Red x ->
      out = x
  end
  ret out
end

test t
  ret 0
end
"#;
        let module = syntax::parse(src, "t.strake").expect("parse");
        let program = compile_module(&module, false).expect("compile");
        let fn_idx = program
            .functions
            .iter()
            .position(|f| f.name == "pick")
            .expect("pick");
        let span = Span {
            file: "t".into(),
            start: 0,
            end: 1,
        };
        let mut m = Machine::new(&program, program.fuel, &EmptyHost);
        let red = m.test_alloc_sum_i64(1, 11);
        m.enter_function(fn_idx, &[red], None, true, span)
            .expect("enter");
        m.run_loop().expect("run");
        assert_eq!(m.root_return, Some(11));
    }

    #[test]
    fn add2_basic_call_returns_14() {
        let src = include_str!("../../../../../golden/add2_basic.strake");
        let module = syntax::parse(src, "golden/add2_basic.strake").expect("parse");
        let program = compile_module(&module, false).expect("compile");
        let host = EmptyHost;
        let n = call_function_i64(&program, "add2", &[3, 4], &host).expect("exec");
        assert_eq!(n, 14);
    }

    #[test]
    fn host_entry_is_call_one_frame_one() {
        let src = "strake 1\n\ntest t\n  ret 0\nend\n";
        let module = syntax::parse(src, "t.strake").expect("parse");
        let program = compile_module(&module, false).expect("compile");
        let mut m = Machine::new_test_channel(&program, program.fuel, &EmptyHost);
        m.run_root_function(0).expect("run");
        assert_eq!(m.call_entries, 1);
        assert_eq!(m.frames.len(), 0);
    }

    #[test]
    fn phi_costs_one_fuel() {
        let span = Span {
            file: "t".into(),
            start: 0,
            end: 1,
        };
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let entry = 0;
        let cond = b.const_bool(entry, span.clone(), true);
        let (end_block, v) = b
            .if_join_i64(
                entry,
                span.clone(),
                cond,
                |bb, then_b| Ok((then_b, bb.const_i64(then_b, span.clone(), 1))),
                |bb, else_b| Ok((else_b, bb.const_i64(else_b, span.clone(), 2))),
            )
            .expect("if");
        b.ret(end_block, span.clone(), v);
        let program = b.finish().expect("finish");
        let mut ok = Machine::new(&program, 7, &EmptyHost);
        assert!(ok.run_root_function(fidx).is_ok());
        let mut short = Machine::new(&program, 6, &EmptyHost);
        assert!(matches!(
            short.run_root_function(fidx),
            Err(ExecFault::Trap {
                code: Code::EFuel,
                ..
            })
        ));
    }

    #[test]
    fn unresolved_import_beats_stack_and_does_not_charge() {
        let span = Span {
            file: "t".into(),
            start: 0,
            end: 1,
        };
        let mut b = Builder::new();
        let imp = b.add_import("host.missing", span.clone());
        let fidx = b.begin_function("f", 0);
        let entry = 0;
        let _v = b.call_import(entry, span.clone(), imp, &[]);
        b.ret_i64(entry, span.clone(), 0);
        let program = b.finish().expect("finish");
        let mut m = Machine::new(&program, 50, &EmptyHost);
        for _ in 0..1024 {
            m.frames.push(Frame {
                fn_idx: fidx,
                block: 0,
                ip: 0,
                edge_pred: 0,
                regs: vec![None],
                pending_call_dst: None,
            });
        }
        let inst = program.functions[fidx].blocks[0].insts[0].clone();
        let err = m.test_exec_inst(&inst);
        assert!(matches!(
            err,
            Err(ExecFault::Trap {
                code: Code::EImport,
                ..
            })
        ));
        assert_eq!(m.fuel, 50);
    }

    #[test]
    fn fuel_before_div0_when_fuel_zero() {
        let span = Span {
            file: "t".into(),
            start: 0,
            end: 1,
        };
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let entry = 0;
        let one = b.const_i64(entry, span.clone(), 1);
        let zero = b.const_i64(entry, span.clone(), 0);
        let _d = b.div_i64(entry, span.clone(), one, zero);
        b.ret_i64(entry, span.clone(), 0);
        let program = b.finish().expect("finish");
        let mut m = Machine::new(&program, 3, &EmptyHost);
        let err = m.run_root_function(fidx);
        assert!(matches!(
            err,
            Err(ExecFault::Trap {
                code: Code::EFuel,
                ..
            })
        ));
    }

    #[test]
    fn i32_min_div_neg_one_and_zero_are_ediv0() {
        let span = Span {
            file: "t".into(),
            start: 0,
            end: 1,
        };
        assert!(matches!(
            i32_div(i32::MIN, -1, span.clone()),
            Err(ExecFault::Trap {
                code: Code::EDiv0,
                ..
            })
        ));
        assert!(matches!(
            i32_div(1, 0, span.clone()),
            Err(ExecFault::Trap {
                code: Code::EDiv0,
                ..
            })
        ));
    }

    #[test]
    fn float_div_by_zero_does_not_trap() {
        let span = Span {
            file: "t".into(),
            start: 0,
            end: 1,
        };
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let entry = 0;
        let a = b.const_f64(entry, span.clone(), 1.0f64.to_bits());
        let z = b.const_f64(entry, span.clone(), 0.0f64.to_bits());
        let _d = b.div_f64(entry, span.clone(), a, z);
        b.ret_i64(entry, span.clone(), 0);
        let program = b.finish().expect("finish");
        let mut m = Machine::new(&program, 100, &EmptyHost);
        assert!(m.run_root_function(fidx).is_ok());
    }

    #[test]
    fn exec_const_and_mul_via_test_exec_inst() {
        let span = Span {
            file: "t".into(),
            start: 0,
            end: 1,
        };
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let entry = 0;
        let _c = b.const_i64(entry, span.clone(), 5);
        b.ret_i64(entry, span.clone(), 0);
        let program = b.finish().expect("finish");
        let mut m = Machine::new(&program, 10, &EmptyHost);
        m.frames.push(Frame {
            fn_idx: fidx,
            block: 0,
            ip: 0,
            edge_pred: 0,
            regs: vec![None],
            pending_call_dst: None,
        });
        let inst = program.functions[fidx].blocks[0].insts[0].clone();
        assert!(matches!(inst.op, Op::ConstI64));
        assert!(!m.test_exec_inst(&inst).expect("const"));
        let mul_inst = Inst {
            op: Op::Mul,
            dst: Some(2),
            args: vec![0, 1],
            imm: Imm::None,
            span: span.clone(),
        };
        m.frames.last_mut().unwrap().regs = vec![Some(Value::I64(2)), Some(Value::I64(3)), None];
        assert!(!m.test_exec_inst(&mul_inst).expect("mul"));
        assert_eq!(m.frames.last().unwrap().regs[2], Some(Value::I64(6)));
    }

    #[test]
    fn ge_i64_via_test_exec_inst() {
        let span = Span {
            file: "t".into(),
            start: 0,
            end: 1,
        };
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let entry = 0;
        let two = b.const_i64(entry, span.clone(), 2);
        let three = b.const_i64(entry, span.clone(), 3);
        let ge = b.ge_i64(entry, span.clone(), three, two);
        b.ret_i64(entry, span.clone(), 0);
        let program = b.finish().expect("finish");
        let mut m = Machine::new(&program, 100, &EmptyHost);
        m.frames.push(Frame {
            fn_idx: fidx,
            block: 0,
            ip: 2,
            edge_pred: 0,
            regs: vec![Some(Value::I64(2)), Some(Value::I64(3)), None],
            pending_call_dst: None,
        });
        let inst = program.functions[fidx].blocks[0].insts[2].clone();
        assert_eq!(inst.op, Op::Ge);
        m.test_exec_ge(&inst).expect("ge");
        let out = m.frames.last().unwrap().regs[ge as usize].clone().unwrap();
        assert_eq!(out, Value::Bool(true));
    }

    fn t3_span() -> Span {
        Span {
            file: "t".into(),
            start: 0,
            end: 1,
        }
    }

    fn t3_machine_with_regs(program: &Program, regs: Vec<Option<Value>>) -> Machine<'_> {
        let mut m = Machine::new(program, 100, &EmptyHost);
        m.frames.push(Frame {
            fn_idx: 0,
            block: 0,
            ip: 0,
            edge_pred: 0,
            regs,
            pending_call_dst: None,
        });
        m
    }

    fn t3_inst(op: Op, imm: Imm) -> Inst {
        Inst {
            op,
            dst: Some(2),
            args: vec![0, 1],
            imm,
            span: t3_span(),
        }
    }

    fn t3_program() -> Program {
        let mut b = Builder::new();
        b.begin_function("f", 0);
        b.ret_i64(0, t3_span(), 0);
        b.finish().expect("finish")
    }

    fn t3_imm_for(op: Op) -> Imm {
        match op {
            Op::ConstI32 => Imm::I32(-4),
            Op::ConstI64 => Imm::I64(4),
            Op::ConstF32 => Imm::F32(1.5f32.to_bits()),
            Op::ConstF64 => Imm::F64(1.5f64.to_bits()),
            Op::ConstBool => Imm::Bool(true),
            Op::Br => Imm::Block(0),
            Op::Trap => Imm::Trap(TrapImm::User),
            _ => Imm::None,
        }
    }

    #[test]
    fn each_card_op_has_an_exec_arm() {
        use Op::*;
        let program = t3_program();
        let card = [
            Br, BrIf, BrTable, Call, Ret, Phi, Trap, Add, Sub, Mul, Div, Eq, Ne, Lt, Le, Gt, Ge,
            ConstI32, ConstI64, ConstF32, ConstF64, ConstBool, ConstUnit, SumNew, SumTag,
            SumPayload, ListEmpty, ListCons, ListLen, ListGet, BytesNew, BytesLen, BytesGet,
            RecNew, RecGet,
        ];
        for op in card {
            let regs = vec![Some(Value::I64(2)), Some(Value::I64(3)), None];
            let mut m = t3_machine_with_regs(&program, regs);
            let res = m.test_step(&t3_inst(op, t3_imm_for(op)));
            if let Err(ExecFault::Codeless(msg)) = &res {
                assert!(!msg.starts_with("unimplemented op"), "{op:?}: {msg}");
            }
        }
    }

    #[test]
    fn t3_compare_arms_follow_operand_order() {
        let program = t3_program();
        let cases = [
            (Op::Ne, 2, 3, true),
            (Op::Ne, 3, 3, false),
            (Op::Lt, 2, 3, true),
            (Op::Lt, 3, 3, false),
            (Op::Le, 3, 3, true),
            (Op::Le, 4, 3, false),
            (Op::Gt, 4, 3, true),
            (Op::Gt, 3, 3, false),
            (Op::Ge, 3, 3, true),
            (Op::Ge, 2, 3, false),
        ];
        for (op, a, b, want) in cases {
            let regs = vec![Some(Value::I64(a)), Some(Value::I64(b)), None];
            let mut m = t3_machine_with_regs(&program, regs);
            let before = m.fuel;
            assert!(!m.test_step(&t3_inst(op, Imm::None)).expect("cmp"));
            assert_eq!(m.fuel, before - 1, "{op:?} charges 1 fuel");
            let got = m.frames.last().unwrap().regs[2].clone();
            assert_eq!(got, Some(Value::Bool(want)), "{op:?} {a} {b}");
        }
    }

    #[test]
    fn t3_compare_bool_float_and_handle_operands() {
        let program = t3_program();
        let run = |op: Op, a: Value, b: Value| {
            let mut m = t3_machine_with_regs(&program, vec![Some(a), Some(b), None]);
            m.test_step(&t3_inst(op, Imm::None)).expect("cmp");
            m.frames.last().unwrap().regs[2].clone().unwrap()
        };
        assert_eq!(
            run(Op::Ne, Value::Bool(true), Value::Bool(false)),
            Value::Bool(true)
        );
        assert_eq!(
            run(Op::Lt, Value::F64(1.0), Value::F64(2.0)),
            Value::Bool(true)
        );
        let nan = Value::F64(f64::NAN);
        assert_eq!(run(Op::Ne, nan.clone(), nan.clone()), Value::Bool(false));
        let other_nan = Value::F64(f64::from_bits(f64::NAN.to_bits() ^ 1));
        assert_eq!(run(Op::Ne, nan.clone(), other_nan), Value::Bool(true));
        assert_eq!(
            run(Op::Ne, Value::F64(0.0), Value::F64(-0.0)),
            Value::Bool(true)
        );
        assert_eq!(
            run(Op::Le, Value::F64(0.0), Value::F64(-0.0)),
            Value::Bool(true)
        );
        assert_eq!(
            run(Op::Le, nan.clone(), Value::F64(1.0)),
            Value::Bool(false)
        );
        assert_eq!(run(Op::Ge, nan, Value::F64(1.0)), Value::Bool(false));
        let bools = vec![Some(Value::Bool(false)), Some(Value::Bool(true)), None];
        let mut m = t3_machine_with_regs(&program, bools);
        assert!(matches!(
            m.test_step(&t3_inst(Op::Lt, Imm::None)),
            Err(ExecFault::Codeless(_))
        ));
        let mut m = t3_machine_with_regs(&program, vec![None, None, None]);
        let x = m.heap.bytes_new(vec![1, 2]).expect("alloc");
        let y = m.heap.bytes_new(vec![1, 3]).expect("alloc");
        m.frames.last_mut().unwrap().regs =
            vec![Some(Value::Handle(x)), Some(Value::Handle(y)), None];
        m.test_step(&t3_inst(Op::Ne, Imm::None)).expect("ne bytes");
        assert_eq!(m.frames.last().unwrap().regs[2], Some(Value::Bool(true)));
    }

    #[test]
    fn t3_narrow_consts_execute_and_charge_fuel() {
        let program = t3_program();
        let cases = [
            (Op::ConstI32, Imm::I32(-7), Value::I32(-7)),
            (Op::ConstF32, Imm::F32(0.5f32.to_bits()), Value::F64(0.5)),
            (Op::ConstUnit, Imm::None, Value::Unit),
        ];
        for (op, imm, want) in cases {
            let mut m = t3_machine_with_regs(&program, vec![None, None, None]);
            let before = m.fuel;
            assert!(!m.test_step(&t3_inst(op, imm)).expect("const"));
            assert_eq!(m.fuel, before - 1);
            assert_eq!(m.frames.last().unwrap().regs[2], Some(want));
        }
        let mut m = t3_machine_with_regs(&program, vec![None, None, None]);
        let bad = m.test_step(&t3_inst(Op::ConstI32, Imm::I64(1)));
        assert!(matches!(bad, Err(ExecFault::Codeless(_))));
    }

    #[test]
    fn f32_arith_rounds_and_charges_one_fuel() {
        let program = t3_program();
        let third = f64::from(1.0f32 / 3.0f32);
        let cases = [
            (Op::Add, 0.25, 0.25, 0.5),
            (Op::Sub, 0.25, 0.25, 0.0),
            (Op::Mul, 0.25, 0.25, 0.0625),
            (Op::Div, 0.25, 0.25, 1.0),
            (Op::Add, 16_777_216.0, 1.0, 16_777_216.0),
            (Op::Div, 1.0, 3.0, third),
        ];
        for (op, a, b, want) in cases {
            let mut m = t3_machine_with_regs(
                &program,
                vec![Some(Value::F64(a)), Some(Value::F64(b)), None],
            );
            let before = m.fuel;
            assert!(!m.test_step(&t3_inst(op, Imm::F32(0))).expect("f32"));
            assert_eq!(m.fuel, before - 1, "{op:?} charges 1");
            assert_eq!(
                m.frames.last().unwrap().regs[2],
                Some(Value::F64(want)),
                "{op:?}"
            );
        }
        let mut m = t3_machine_with_regs(
            &program,
            vec![Some(Value::F64(1.0)), Some(Value::F64(0.0)), None],
        );
        assert!(m.test_step(&t3_inst(Op::Div, Imm::F32(0))).is_ok());
        match &m.frames.last().unwrap().regs[2] {
            Some(Value::F64(z)) => assert!(z.is_infinite() && z.is_sign_positive()),
            other => panic!("div0 {other:?}"),
        }
        let mut zero = t3_machine_with_regs(&program, vec![None, None, None]);
        let before = zero.fuel;
        assert!(!zero
            .test_step(&t3_inst(Op::ConstF32, Imm::F32(0)))
            .expect("const +0"));
        assert_eq!(zero.fuel, before - 1);
        assert_eq!(zero.frames.last().unwrap().regs[2], Some(Value::F64(0.0)));
    }

    #[test]
    fn for_body_jump_to_exit_returns_that_value() {
        let got = super::for_body_leaves_through_exit(0, 0, 1000, 1).expect("run");
        assert_eq!(got, 1);
    }

    #[test]
    fn t3_builder_for_runs_and_its_err_arm_traps_lower() {
        let span = t3_span();
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let empty = b.list_empty(0, span.clone());
        let seven = b.const_i64(0, span.clone(), 7);
        let xs = b.list_cons(0, span.clone(), seven, empty);
        let mut seen = None;
        let exit = b
            .for_(0, span.clone(), xs, |_, ok_arm, elt, _exit| {
                seen = Some(elt);
                Ok::<usize, ()>(ok_arm)
            })
            .expect("for_");
        assert!(seen.is_some());
        b.ret_i64(exit, span.clone(), 0);
        let program = b.finish().expect("finish");
        let mut m = Machine::new(&program, 1000, &EmptyHost);
        assert!(m.run_root_function(fidx).is_ok());
        let lower = program.functions[fidx]
            .blocks
            .iter()
            .flat_map(|blk| blk.insts.iter())
            .find(|i| matches!(i.imm, Imm::Trap(TrapImm::Lower)))
            .expect("trap Lower arm")
            .clone();
        let mut m = t3_machine_with_regs(&program, Vec::new());
        assert!(matches!(
            m.test_step(&lower),
            Err(ExecFault::Trap {
                code: Code::ELower,
                ..
            })
        ));
    }

    /// xs = [4, 3]; the body traps when it sees `target`.
    fn t3_for_traps_on(target: i64) -> Result<i64, ExecFault> {
        let span = t3_span();
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let empty = b.list_empty(0, span.clone());
        let three = b.const_i64(0, span.clone(), 3);
        let four = b.const_i64(0, span.clone(), 4);
        let t1 = b.list_cons(0, span.clone(), three, empty);
        let xs = b.list_cons(0, span.clone(), four, t1);
        let exit = b
            .for_(0, span.clone(), xs, |b, ok_arm, elt, _exit| {
                let hit = b.new_block();
                let next = b.new_block();
                let want = b.const_i64(ok_arm, span.clone(), target);
                let is_four = b.eq_i64(ok_arm, span.clone(), elt, want);
                b.cond_branch(ok_arm, span.clone(), is_four, hit, next);
                b.trap_user(hit, span.clone());
                Ok::<usize, ()>(next)
            })
            .expect("for_");
        b.ret_i64(exit, span.clone(), 0);
        let program = b.finish().expect("finish");
        let mut m = Machine::new(&program, 1000, &EmptyHost);
        m.run_root_function(fidx)
    }

    #[test]
    fn t3_builder_for_visits_every_index_from_zero() {
        for target in [4, 3] {
            let res = t3_for_traps_on(target);
            let trapped = matches!(
                res,
                Err(ExecFault::Trap {
                    code: Code::EType,
                    ..
                })
            );
            assert!(trapped, "element {target} not visited");
        }
        assert!(t3_for_traps_on(5).is_ok());
    }

    /// `forall n in lo..hi` whose body traps when `n == target`.
    fn t4_forall_traps_on(lo: i64, hi: i64, target: i64) -> Result<i64, ExecFault> {
        let span = t3_span();
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let lo_v = b.const_i64(0, span.clone(), lo);
        let hi_v = b.const_i64(0, span.clone(), hi);
        let exit = b
            .forall_range(0, span.clone(), lo_v, hi_v, |b, start, n| {
                let hit = b.new_block();
                let next = b.new_block();
                let want = b.const_i64(start, span.clone(), target);
                let is_target = b.eq_i64(start, span.clone(), n, want);
                b.cond_branch(start, span.clone(), is_target, hit, next);
                b.trap_user(hit, span.clone());
                Ok::<usize, ()>(next)
            })
            .expect("forall_range");
        b.ret_i64(exit, span.clone(), 0);
        let program = b.finish().expect("finish");
        let mut m = Machine::new(&program, 1000, &EmptyHost);
        m.run_root_function(fidx)
    }

    fn t4_trapped(res: Result<i64, ExecFault>) -> bool {
        matches!(
            res,
            Err(ExecFault::Trap {
                code: Code::EType,
                ..
            })
        )
    }

    #[test]
    fn t4_forall_range_visits_lo_through_hi_inclusive() {
        for target in [-1, 0, 1] {
            assert!(t4_trapped(t4_forall_traps_on(-1, 1, target)), "{target}");
        }
        assert!(t4_forall_traps_on(-1, 1, 2).is_ok());
        assert!(t4_forall_traps_on(-1, 1, -2).is_ok());
        assert!(
            t4_trapped(t4_forall_traps_on(7, 7, 7)),
            "lo == hi runs once"
        );
    }

    #[test]
    fn t4_forall_range_reaches_i64_max_without_wrapping() {
        let max = i64::MAX;
        assert!(t4_trapped(t4_forall_traps_on(max - 2, max, max)));
        assert!(t4_forall_traps_on(max - 2, max, max - 3).is_ok());
    }

    #[test]
    fn t5_region_len_checks_bounds() {
        assert_eq!(region_len(1), 65536);
        assert_eq!(region_len(0), 0);
        for bad in [-1, 1 << 48, i64::MAX] {
            assert!(
                std::panic::catch_unwind(|| region_len(bad)).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn t5_scalar_codec_round_trips() {
        let cases = [
            (ScalarTy::I32, Value::I32(-5)),
            (ScalarTy::I64, Value::I64(i64::MIN)),
            (ScalarTy::F32, Value::F64(0.25)),
            (ScalarTy::F64, Value::F64(-1.5)),
            (ScalarTy::Bool, Value::Bool(true)),
            (ScalarTy::Bool, Value::Bool(false)),
        ];
        for (ty, v) in cases {
            let bytes = encode_scalar(ty, &v).expect("encode");
            assert_eq!(bytes.len() as i128, ty.width(), "{ty:?}");
            assert_eq!(decode_scalar(ty, &bytes).expect("decode"), v, "{ty:?}");
        }
        assert_eq!(
            encode_scalar(ScalarTy::I32, &Value::I32(258)).unwrap(),
            vec![2, 1, 0, 0]
        );
        assert!(matches!(
            decode_scalar(ScalarTy::Bool, &[2]),
            Err(ExecFault::Halt(m)) if m == BOOL_LOAD_HALT
        ));
        assert!(encode_scalar(ScalarTy::Bool, &Value::I64(1)).is_err());
    }

    static T5_HOST_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn t5_counting_import(_: &mut Machine, _: &[Value]) -> Result<Value, ()> {
        T5_HOST_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Value::I64(7))
    }

    struct T5HalfHost;

    static NEG_COST_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn neg_cost_import(_m: &mut Machine, _a: &[Value]) -> Result<Value, ()> {
        NEG_COST_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Value::I64(0))
    }

    /// Binds every import, but `host.neg` costs -1 fuel (SPEC "Fuel": must be >= 0).
    struct NegCostHost;

    impl Host for NegCostHost {
        fn resolve_import(&self, _path: &str) -> Option<crate::body::host::ImportFn> {
            Some(neg_cost_import as crate::body::host::ImportFn)
        }

        fn fuel_cost(&self, path: &str) -> i64 {
            if path == "host.neg" {
                -1
            } else {
                0
            }
        }
    }

    fn neg_cost_program() -> (Program, usize) {
        let span = t3_span();
        let mut b = Builder::new();
        let neg = b.add_import("host.neg", span.clone());
        let fidx = b.begin_function("test.t", 0);
        let _r = b.call_import(0, span.clone(), neg, &[]);
        b.ret_i64(0, span.clone(), 0);
        b.add_export("test.t", fidx, true);
        (b.finish().expect("finish"), fidx)
    }

    /// Binds `host.ok` at fuel cost 4, or fails the call for `host.fail`.
    struct CostHost;

    fn ok_import(_m: &mut Machine, _a: &[Value]) -> Result<Value, ()> {
        Ok(Value::I64(9))
    }

    fn fail_import(_m: &mut Machine, _a: &[Value]) -> Result<Value, ()> {
        Err(())
    }

    impl Host for CostHost {
        fn resolve_import(&self, path: &str) -> Option<crate::body::host::ImportFn> {
            match path {
                "host.ok" => Some(ok_import as crate::body::host::ImportFn),
                "host.fail" => Some(fail_import as crate::body::host::ImportFn),
                _ => None,
            }
        }

        fn fuel_cost(&self, _path: &str) -> i64 {
            4
        }
    }

    fn one_import_program(path: &str) -> (Program, usize) {
        let span = t3_span();
        let mut b = Builder::new();
        let imp = b.add_import(path, span.clone());
        let fidx = b.begin_function("test.t", 0);
        let r = b.call_import(0, span.clone(), imp, &[]);
        b.ret(0, span.clone(), r);
        b.add_export("test.t", fidx, true);
        (b.finish().expect("finish"), fidx)
    }

    #[test]
    fn import_call_charges_one_plus_fuel_cost() {
        let (program, fidx) = one_import_program("host.ok");
        let mut m = Machine::new(&program, 1000, &CostHost);
        assert_eq!(m.run_root_function(fidx).unwrap(), 9);
        let mut low = Machine::new(&program, 4, &CostHost);
        assert!(matches!(
            low.run_root_function(fidx),
            Err(ExecFault::Trap {
                code: Code::EFuel,
                ..
            })
        ));
        let (failing, fidx) = one_import_program("host.fail");
        let mut m = Machine::new(&failing, 1000, &CostHost);
        assert!(matches!(
            m.run_root_function(fidx),
            Err(ExecFault::Codeless(_))
        ));
    }

    #[test]
    fn negative_fuel_cost_traps_e_import_at_instantiate_and_call() {
        NEG_COST_CALLS.store(0, std::sync::atomic::Ordering::SeqCst);
        let (program, fidx) = neg_cost_program();
        let runs = run_test_exports(&program, &NegCostHost);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].diags[0].code, Code::EImport);
        let mut m = Machine::new(&program, 1000, &NegCostHost);
        assert!(matches!(
            m.run_root_function(fidx),
            Err(ExecFault::Trap {
                code: Code::EImport,
                ..
            })
        ));
        assert_eq!(NEG_COST_CALLS.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    fn float_eq(x: f64, y: f64) -> bool {
        let span = t3_span();
        let mut b = Builder::new();
        let fidx = b.begin_function("f", 0);
        let a = b.const_f64(0, span.clone(), x.to_bits());
        let c = b.const_f64(0, span.clone(), y.to_bits());
        let e = b.eq_i64(0, span.clone(), a, c);
        b.ret(0, span.clone(), e);
        let program = b.finish().expect("finish");
        let eq = program.functions[fidx].blocks[0]
            .insts
            .iter()
            .find(|i| i.op == Op::Eq)
            .unwrap()
            .clone();
        let regs = vec![Some(Value::F64(x)), Some(Value::F64(y)), None];
        let mut m = t3_machine_with_regs(&program, regs);
        let mut eq = eq;
        eq.args = vec![0, 1];
        eq.dst = Some(2);
        m.test_step(&eq).unwrap();
        matches!(m.read_reg(2).unwrap(), Value::Bool(true))
    }

    #[test]
    fn float_eq_is_bitwise() {
        assert!(float_eq(1.5, 1.5));
        assert!(!float_eq(0.0, -0.0), "+0 and -0 differ");
        let nan_a = f64::from_bits(0x7FF8_0000_0000_0001);
        let nan_b = f64::from_bits(0x7FF8_0000_0000_0002);
        assert!(float_eq(nan_a, nan_a), "same NaN bits are eq");
        assert!(!float_eq(nan_a, nan_b), "different NaN payloads differ");
    }

    #[test]
    fn float_eq_with_unset_vreg_and_no_fuel_is_e_fuel() {
        let span = t3_span();
        let mut b = Builder::new();
        let _f = b.begin_function("f", 0);
        let a = b.const_f64(0, span.clone(), 1.5f64.to_bits());
        let e = b.eq_i64(0, span.clone(), a, a);
        b.ret(0, span.clone(), e);
        let program = b.finish().expect("finish");
        let eq = program.functions[0].blocks[0]
            .insts
            .iter()
            .find(|i| i.op == Op::Eq)
            .unwrap()
            .clone();
        let mut m = t3_machine_with_regs(&program, vec![None, None, None]);
        m.fuel = 0;
        assert!(matches!(
            m.test_step(&eq),
            Err(ExecFault::Trap {
                code: Code::EFuel,
                ..
            })
        ));
    }

    #[test]
    fn f32_nan_payload_and_sign_survive_store_and_load() {
        for bits in [0x7F80_0001u32, 0xFFC0_1234, 0x7FC0_0000, 0x7FBF_FFFF] {
            let wide = f32_bits_to_f64(bits);
            assert!(wide.is_nan());
            assert_eq!(f64_to_f32_bits(wide), bits, "{bits:#x}");
            let bytes = encode_scalar(ScalarTy::F32, &Value::F64(wide)).unwrap();
            assert_eq!(bytes, bits.to_le_bytes());
            match decode_scalar(ScalarTy::F32, &bytes).unwrap() {
                Value::F64(x) => assert_eq!(x.to_bits(), wide.to_bits()),
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(f64_to_f32_bits(0.25), 0.25f32.to_bits());
        assert_eq!(f32_bits_to_f64(0.25f32.to_bits()), 0.25);
        // A NaN whose payload sits only below f32 precision stays a NaN, not inf.
        assert!(f32::from_bits(f64_to_f32_bits(f64::from_bits(0x7FF0_0000_0000_0001))).is_nan());
    }

    impl Host for T5HalfHost {
        fn resolve_import(&self, path: &str) -> Option<crate::body::host::ImportFn> {
            (path == "host.bound").then_some(t5_counting_import as crate::body::host::ImportFn)
        }
    }

    #[test]
    fn t5_instantiate_missing_import_traps_before_any_call() {
        let span = t3_span();
        let mut b = Builder::new();
        let bound = b.add_import("host.bound", span.clone());
        let _miss = b.add_import("host.missing", span.clone());
        let fidx = b.begin_function("test.t", 0);
        let _r = b.call_import(0, span.clone(), bound, &[]);
        b.ret_i64(0, span.clone(), 0);
        b.add_export("test.t", fidx, true);
        let program = b.finish().expect("finish");
        T5_HOST_CALLS.store(0, std::sync::atomic::Ordering::SeqCst);
        let runs = run_test_exports(&program, &T5HalfHost);
        assert_eq!(runs.len(), 1);
        assert!(runs[0].outcome.trapped);
        assert_eq!(runs[0].diags.len(), 1);
        assert_eq!(runs[0].diags[0].code, Code::EImport);
        assert_eq!(T5_HOST_CALLS.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn t5_bool_halt_stops_later_test_exports() {
        let span = t3_span();
        let mut b = Builder::new();
        b.set_region_pages(1);
        let bound = b.add_import("host.bound", span.clone());
        let first = b.begin_function("test.a", 0);
        let addr = b.const_i64(0, span.clone(), 0);
        let two = b.const_i64(0, span.clone(), 2);
        let _s = b.store(0, span.clone(), ScalarTy::I64, addr, two);
        let _l = b.load(0, span.clone(), ScalarTy::Bool, addr);
        b.ret_i64(0, span.clone(), 0);
        b.add_export("test.a", first, true);
        let second = b.begin_function("test.b", 0);
        let _r = b.call_import(0, span.clone(), bound, &[]);
        b.ret_i64(0, span.clone(), 0);
        b.add_export("test.b", second, true);
        let program = b.finish().expect("finish");
        T5_HOST_CALLS.store(0, std::sync::atomic::Ordering::SeqCst);
        let runs = run_test_exports(&program, &T5HalfHost);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].halt.as_deref(), Some(BOOL_LOAD_HALT));
        assert_eq!(T5_HOST_CALLS.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn value_as_and_imm_helpers() {
        assert_eq!(value_as_i32(&Value::I32(7)).unwrap(), 7);
        assert_eq!(value_as_i32(&Value::I64(9)).unwrap(), 9);
        assert!(value_as_i32(&Value::Bool(true)).is_err());
        assert_eq!(value_as_f64(&Value::F64(1.5)).unwrap(), 1.5);
        assert!(value_as_f64(&Value::I64(1)).is_err());
        assert_eq!(block_imm(&Imm::Block(3)).unwrap(), 3);
        assert!(block_imm(&Imm::Blocks(vec![1])).is_err());
        assert_eq!(blocks_imm(&Imm::Blocks(vec![1, 2])).unwrap(), vec![1, 2]);
        assert!(blocks_imm(&Imm::Block(0)).is_err());
    }
}
