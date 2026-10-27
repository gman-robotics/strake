//! Reference interpreter for finished `Program`s.

pub(crate) mod ir;
mod op;

pub(crate) use ir::{Builder, FinishError};
pub use ir::{Inst, Program};

use crate::body::diag::{
    render_scalar_bool, render_scalar_i64, trap_diag, Code, Diag, ExportOutcome, Kind, Span,
};
use crate::body::host::Host;
use op::{Op, TrapImm};

use ir::{Imm, RetImm, VReg};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    I64(i64),
    I32(i32),
    Bool(bool),
    F64(f64),
    #[allow(dead_code)]
    Unit,
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
    Trap { code: Code, span: Span },
    Codeless(String),
}

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
        }
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
            if self.exec_inst(&inst)? {
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
                    let (a, b) = self.two_i64(inst)?;
                    a == b
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
                    Imm::Import(import_idx) => {
                        let stub = self
                            .program
                            .imports
                            .get(import_idx)
                            .ok_or_else(|| ExecFault::Codeless("bad import idx".into()))?;
                        let host_fn = self.host.resolve_import(&stub.path);
                        if host_fn.is_none() {
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
                        let cost = 1 + stub.fuel_cost;
                        self.charge(cost, span.clone())?;
                        self.call_entries += 1;
                        let host_fn = host_fn.unwrap();
                        let result = host_fn(self, &args)
                            .map_err(|_| ExecFault::Codeless("host import failed".into()))?;
                        self.write_reg(dst, result)?;
                    }
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
                let ret_i64 = value_as_i64(&v)?;
                let is_normal_ret = matches!(&ret_imm, RetImm::Normal);
                match ret_imm {
                    RetImm::TestFail {
                        want,
                        got,
                        diag_span,
                    } => {
                        let want_v = self.read_reg(want)?;
                        let got_v = self.read_reg(got)?;
                        let (want_s, got_s) = render_test_fail_scalars(&want_v, &got_v)?;
                        self.result_diags.push(Diag {
                            code: Code::ETest,
                            kind: Kind::Result,
                            span: diag_span,
                            want: Some(want_s),
                            got: Some(got_s),
                            patch: None,
                        });
                    }
                    RetImm::Normal => {}
                }
                let call_dst = self.frames.last().unwrap().pending_call_dst;
                self.frames.pop();
                if let Some(dst) = call_dst {
                    if let Some(caller) = self.frames.last_mut() {
                        ensure_reg(&mut caller.regs, dst as usize);
                        caller.regs[dst as usize] = Some(v);
                    }
                } else if self.frames.is_empty() {
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
            _ => {
                return Err(ExecFault::Codeless(format!(
                    "unimplemented op {:?}",
                    inst.op
                )))
            }
        }
        Ok(false)
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

fn i64_div(a: i64, b: i64, span: Span) -> Result<i64, ExecFault> {
    if b == 0 || (a == i64::MIN && b == -1) {
        return Err(ExecFault::Trap {
            code: Code::EDiv0,
            span,
        });
    }
    Ok(a / b)
}

fn i32_div(a: i32, b: i32, span: Span) -> Result<i32, ExecFault> {
    if b == 0 || (a == i32::MIN && b == -1) {
        return Err(ExecFault::Trap {
            code: Code::EDiv0,
            span,
        });
    }
    Ok(a / b)
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
        _ => Err(ExecFault::Codeless("unsupported scalar in E_TEST".into())),
    }
}

fn value_as_i64(v: &Value) -> Result<i64, ExecFault> {
    match v {
        Value::I64(n) => Ok(*n),
        Value::I32(n) => Ok(*n as i64),
        _ => Err(ExecFault::Codeless("expected i64".into())),
    }
}

fn value_as_i32(v: &Value) -> Result<i32, ExecFault> {
    match v {
        Value::I32(n) => Ok(*n),
        Value::I64(n) => Ok(*n as i32),
        _ => Err(ExecFault::Codeless("expected i32".into())),
    }
}

fn value_as_f64(v: &Value) -> Result<f64, ExecFault> {
    match v {
        Value::F64(n) => Ok(*n),
        _ => Err(ExecFault::Codeless("expected f64".into())),
    }
}

fn block_imm(imm: &Imm) -> Result<usize, ExecFault> {
    match imm {
        Imm::Block(b) => Ok(*b),
        _ => Err(ExecFault::Codeless("expected block imm".into())),
    }
}

fn blocks_imm(imm: &Imm) -> Result<Vec<usize>, ExecFault> {
    match imm {
        Imm::Blocks(b) => Ok(b.clone()),
        _ => Err(ExecFault::Codeless("expected blocks imm".into())),
    }
}

/// One test/property export run: status input plus any result-channel diags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TestExportRun {
    pub outcome: ExportOutcome,
    pub diags: Vec<Diag>,
}

pub(crate) fn run_test_exports(program: &Program, host: &dyn Host) -> Vec<TestExportRun> {
    #[cfg(test)]
    {
        crate::body::EXEC_ENTERED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    program
        .exports
        .iter()
        .filter(|e| e.is_test)
        .map(|e| {
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
            };
            let mut diags = m.take_result_diags();
            if let Some(td) = trap_diag {
                diags.push(td);
            }
            TestExportRun { outcome, diags }
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::compile_module;
    use crate::body::host::EmptyHost;
    use crate::body::syntax;

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
        let imp = b.add_import("host.missing");
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
}
