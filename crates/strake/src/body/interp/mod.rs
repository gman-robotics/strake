//! Reference interpreter for finished `Program`s.

pub(crate) mod ir;
mod op;

pub(crate) use ir::{Builder, FinishError};
pub use ir::{Inst, Program};

use crate::body::diag::{Code, ExportOutcome};
use crate::body::host::Host;
use op::{Op, TrapImm};

use ir::{Imm, VReg};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    I64(i64),
    Bool(bool),
    #[allow(dead_code)]
    Unit,
}

pub(crate) struct Machine<'a> {
    program: &'a Program,
    #[allow(dead_code)]
    host: &'a dyn Host,
    fuel: i64,
    frames: Vec<Frame>,
    root_return: Option<i64>,
}

struct Frame {
    fn_idx: usize,
    block: usize,
    ip: usize,
    /// Predecessor block for the most recent edge into `block`.
    edge_pred: usize,
    regs: Vec<Option<Value>>,
    /// When set, `Ret` writes into this vreg in the caller frame instead of halting.
    pending_call_dst: Option<VReg>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExecFault {
    Trap(Code),
    Codeless(String),
}

impl<'a> Machine<'a> {
    pub(crate) fn new(program: &'a Program, fuel: i64, host: &'a dyn Host) -> Self {
        Self {
            program,
            host,
            fuel,
            frames: Vec::new(),
            root_return: None,
        }
    }

    fn charge(&mut self, cost: i64) -> Result<(), ExecFault> {
        if self.fuel < cost {
            return Err(ExecFault::Trap(Code::EFuel));
        }
        self.fuel -= cost;
        Ok(())
    }

    pub(crate) fn run_root_function(&mut self, fn_idx: usize) -> Result<i64, ExecFault> {
        self.root_return = None;
        self.enter_function(fn_idx, &[], None, true)?;
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
    ) -> Result<(), ExecFault> {
        if self.frames.len() >= 1024 {
            return Err(ExecFault::Trap(Code::EStack));
        }
        if charge_entry {
            self.charge(1)?;
        }
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
    fn exec_inst(&mut self, inst: &Inst) -> Result<bool, ExecFault> {
        if inst.op != Op::Call {
            self.charge(1)?;
        }
        match inst.op {
            Op::ConstI64 => {
                let v = match inst.imm {
                    Imm::I64(n) => Value::I64(n),
                    _ => return Err(ExecFault::Codeless("bad const".into())),
                };
                self.write_reg(inst.dst, v)?;
            }
            Op::Add => {
                let (a, b) = self.two_i64(inst)?;
                self.write_reg(inst.dst, Value::I64(a.wrapping_add(b)))?;
            }
            Op::Mul => {
                let (a, b) = self.two_i64(inst)?;
                self.write_reg(inst.dst, Value::I64(a.wrapping_mul(b)))?;
            }
            Op::Eq => {
                let (a, b) = self.two_i64(inst)?;
                self.write_reg(inst.dst, Value::Bool(a == b))?;
            }
            Op::Call => {
                if self.frames.len() >= 1024 {
                    return Err(ExecFault::Trap(Code::EStack));
                }
                self.charge(1)?;
                let callee = match inst.imm {
                    Imm::Fn(i) => i,
                    _ => return Err(ExecFault::Codeless("bad call".into())),
                };
                let args: Vec<Value> = inst
                    .args
                    .iter()
                    .map(|r| self.read_reg(*r))
                    .collect::<Result<Vec<_>, _>>()?;
                let dst = inst.dst;
                self.enter_function(callee, &args, dst, false)?;
            }
            Op::Ret => {
                let v = self.read_reg(inst.args[0])?;
                let ret_i64 = value_as_i64(&v)?;
                let call_dst = self.frames.last().unwrap().pending_call_dst;
                self.frames.pop();
                if let Some(dst) = call_dst {
                    if let Some(caller) = self.frames.last_mut() {
                        ensure_reg(&mut caller.regs, dst as usize);
                        caller.regs[dst as usize] = Some(v);
                    }
                } else if self.frames.is_empty() {
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
                let code = match kind {
                    TrapImm::Fuel => Code::EFuel,
                    TrapImm::Stack => Code::EStack,
                    TrapImm::Div0 => Code::EDiv0,
                    TrapImm::Hole => Code::EHole,
                    TrapImm::Lower => Code::ELower,
                    TrapImm::Import => Code::EImport,
                    TrapImm::Oob => Code::EOob,
                    TrapImm::User => Code::EType,
                };
                return Err(ExecFault::Trap(code));
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

fn ensure_reg(regs: &mut Vec<Option<Value>>, idx: usize) {
    if regs.len() <= idx {
        regs.resize(idx + 1, None);
    }
}

fn value_as_i64(v: &Value) -> Result<i64, ExecFault> {
    match v {
        Value::I64(n) => Ok(*n),
        _ => Err(ExecFault::Codeless("expected i64".into())),
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

pub(crate) fn run_test_exports(program: &Program, host: &dyn Host) -> Vec<ExportOutcome> {
    #[cfg(test)]
    {
        crate::body::EXEC_ENTERED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    program
        .exports
        .iter()
        .filter(|e| e.is_test)
        .map(|e| {
            let mut m = Machine::new(program, 100_000, host);
            match m.run_root_function(e.fn_idx) {
                Ok(ret) => ExportOutcome {
                    trapped: false,
                    ret,
                },
                Err(ExecFault::Trap(_)) => ExportOutcome {
                    trapped: true,
                    ret: 0,
                },
                Err(ExecFault::Codeless(_)) => ExportOutcome {
                    trapped: true,
                    ret: 0,
                },
            }
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
    let mut m = Machine::new(program, 100_000, host);
    m.root_return = None;
    m.enter_function(fn_idx, &vargs, None, true)?;
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
}
