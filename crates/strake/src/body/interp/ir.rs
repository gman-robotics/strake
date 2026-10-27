//! SSA program builder and finished `Program` (opcodes stay in `interp`).

use super::op::{Op, TrapImm};
use crate::body::diag::Span;

pub type VReg = u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportStub {
    pub path: String,
    pub fuel_cost: i64,
}

#[derive(Debug)]
pub struct Program {
    pub functions: Vec<Function>,
    pub exports: Vec<Export>,
    pub fuel: i64,
    pub imports: Vec<ImportStub>,
    /// Span charged on host entry (module `fuel` directive or module header).
    pub entry_charge_span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub param_count: usize,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub insts: Vec<Inst>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inst {
    pub op: Op,
    pub dst: Option<VReg>,
    pub args: Vec<VReg>,
    pub imm: Imm,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetImm {
    Normal,
    TestFail {
        want: VReg,
        got: VReg,
        diag_span: Span,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Imm {
    None,
    I64(i64),
    I32(i32),
    F64(u64),
    F32(u32),
    Bool(bool),
    Block(usize),
    Blocks(Vec<usize>),
    Fn(usize),
    Import(usize),
    I32Width,
    Trap(TrapImm),
    Ret(RetImm),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub name: String,
    pub fn_idx: usize,
    pub is_test: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FinishError {
    InvalidBranchTarget {
        block: usize,
        target: usize,
    },
    InvalidPhi {
        block: usize,
        dst: VReg,
    },
    SsaBlockTerminator {
        fn_name: String,
        block: usize,
        detail: String,
    },
    SsaPhiPredecessor {
        fn_name: String,
        block: usize,
        phi_dst: VReg,
        pred_block: usize,
    },
    SsaPhiMissingPredecessor {
        fn_name: String,
        block: usize,
        phi_dst: VReg,
        pred_block: usize,
    },
    SsaPhiImmediate {
        fn_name: String,
        block: usize,
        phi_dst: VReg,
    },
}

fn is_terminator(op: Op) -> bool {
    matches!(op, Op::Br | Op::BrIf | Op::BrTable | Op::Ret | Op::Trap)
}

fn terminator_successors(inst: &Inst) -> Vec<usize> {
    match inst.op {
        Op::Br => {
            if let Imm::Block(t) = inst.imm {
                vec![t]
            } else {
                vec![]
            }
        }
        Op::BrIf | Op::BrTable => {
            if let Imm::Blocks(ts) = &inst.imm {
                ts.clone()
            } else {
                vec![]
            }
        }
        Op::Ret | Op::Trap => vec![],
        _ => vec![],
    }
}

pub(crate) fn verify_function_ssa_public(func: &Function) -> Result<(), FinishError> {
    verify_function_ssa(func)
}

fn verify_function_ssa(func: &Function) -> Result<(), FinishError> {
    let n = func.blocks.len();
    let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (bi, block) in func.blocks.iter().enumerate() {
        if block.insts.is_empty() {
            return Err(FinishError::SsaBlockTerminator {
                fn_name: func.name.clone(),
                block: bi,
                detail: "block has no instructions".into(),
            });
        }
        let last_idx = block.insts.len() - 1;
        for (ii, inst) in block.insts.iter().enumerate() {
            if is_terminator(inst.op) && ii != last_idx {
                return Err(FinishError::SsaBlockTerminator {
                    fn_name: func.name.clone(),
                    block: bi,
                    detail: "terminator is not the last instruction in the block".into(),
                });
            }
        }
        let last = &block.insts[last_idx];
        if !is_terminator(last.op) {
            return Err(FinishError::SsaBlockTerminator {
                fn_name: func.name.clone(),
                block: bi,
                detail: "block does not end in a terminator".into(),
            });
        }
        for t in terminator_successors(last) {
            if t < n {
                predecessors[t].push(bi);
            }
        }
    }
    for (bi, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            if inst.op != Op::Phi {
                continue;
            }
            let dst = inst.dst.unwrap_or(0);
            let Imm::Blocks(phi_preds) = &inst.imm else {
                return Err(FinishError::SsaPhiImmediate {
                    fn_name: func.name.clone(),
                    block: bi,
                    phi_dst: dst,
                });
            };
            for pred in phi_preds {
                if !predecessors[bi].contains(pred) {
                    return Err(FinishError::SsaPhiPredecessor {
                        fn_name: func.name.clone(),
                        block: bi,
                        phi_dst: dst,
                        pred_block: *pred,
                    });
                }
            }
            for pred in &predecessors[bi] {
                if !phi_preds.contains(pred) {
                    return Err(FinishError::SsaPhiMissingPredecessor {
                        fn_name: func.name.clone(),
                        block: bi,
                        phi_dst: dst,
                        pred_block: *pred,
                    });
                }
            }
        }
    }
    Ok(())
}

pub(crate) struct Builder {
    functions: Vec<Function>,
    exports: Vec<Export>,
    imports: Vec<ImportStub>,
    fuel: i64,
    entry_charge_span: Span,
    cur_fn: Option<usize>,
    next_vreg: VReg,
    next_block: usize,
}

#[allow(dead_code)]
impl Builder {
    pub(crate) fn new() -> Self {
        Self {
            functions: Vec::new(),
            exports: Vec::new(),
            imports: Vec::new(),
            fuel: 100_000,
            entry_charge_span: Span {
                file: String::new(),
                start: 0,
                end: 0,
            },
            cur_fn: None,
            next_vreg: 0,
            next_block: 0,
        }
    }

    pub(crate) fn set_fuel(&mut self, fuel: i64) {
        self.fuel = fuel;
    }

    pub(crate) fn set_entry_charge_span(&mut self, span: Span) {
        self.entry_charge_span = span;
    }

    pub(crate) fn add_import(&mut self, path: &str) -> usize {
        let idx = self.imports.len();
        self.imports.push(ImportStub {
            path: path.to_string(),
            fuel_cost: 0,
        });
        idx
    }

    pub(crate) fn begin_function(&mut self, name: &str, params: usize) -> usize {
        let fn_idx = self.functions.len();
        self.functions.push(Function {
            name: name.to_string(),
            param_count: params,
            blocks: Vec::new(),
        });
        self.cur_fn = Some(fn_idx);
        self.next_vreg = params as VReg;
        self.next_block = 0;
        self.new_block();
        fn_idx
    }

    /// Aim subsequent `emit` calls at an already-registered function (multi-fn modules).
    pub(crate) fn select_function(&mut self, fn_idx: usize) {
        self.cur_fn = Some(fn_idx);
        let func = &self.functions[fn_idx];
        self.next_vreg = func.param_count as VReg;
    }

    pub(crate) fn new_block(&mut self) -> usize {
        let fn_idx = self.cur_fn.expect("begin_function first");
        let id = self.functions[fn_idx].blocks.len();
        self.functions[fn_idx]
            .blocks
            .push(Block { insts: Vec::new() });
        id
    }

    pub(crate) fn alloc_vreg(&mut self) -> VReg {
        let v = self.next_vreg;
        self.next_vreg += 1;
        v
    }

    fn emit(&mut self, block: usize, inst: Inst) {
        let fn_idx = self.cur_fn.expect("begin_function first");
        self.functions[fn_idx].blocks[block].insts.push(inst);
    }

    pub(crate) fn const_bool(&mut self, block: usize, span: Span, value: bool) -> VReg {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::ConstBool,
                dst: Some(dst),
                args: Vec::new(),
                imm: Imm::Bool(value),
                span,
            },
        );
        dst
    }

    pub(crate) fn const_i64(&mut self, block: usize, span: Span, value: i64) -> VReg {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::ConstI64,
                dst: Some(dst),
                args: Vec::new(),
                imm: Imm::I64(value),
                span,
            },
        );
        dst
    }

    pub(crate) fn add_i64(&mut self, block: usize, span: Span, lhs: VReg, rhs: VReg) -> VReg {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::Add,
                dst: Some(dst),
                args: vec![lhs, rhs],
                imm: Imm::None,
                span,
            },
        );
        dst
    }

    pub(crate) fn mul_i64(&mut self, block: usize, span: Span, lhs: VReg, rhs: VReg) -> VReg {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::Mul,
                dst: Some(dst),
                args: vec![lhs, rhs],
                imm: Imm::None,
                span,
            },
        );
        dst
    }

    pub(crate) fn sub_i64(&mut self, block: usize, span: Span, lhs: VReg, rhs: VReg) -> VReg {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::Sub,
                dst: Some(dst),
                args: vec![lhs, rhs],
                imm: Imm::None,
                span,
            },
        );
        dst
    }

    pub(crate) fn div_i64(&mut self, block: usize, span: Span, lhs: VReg, rhs: VReg) -> VReg {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::Div,
                dst: Some(dst),
                args: vec![lhs, rhs],
                imm: Imm::None,
                span,
            },
        );
        dst
    }

    pub(crate) fn div_i32(&mut self, block: usize, span: Span, lhs: VReg, rhs: VReg) -> VReg {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::Div,
                dst: Some(dst),
                args: vec![lhs, rhs],
                imm: Imm::I32Width,
                span,
            },
        );
        dst
    }

    pub(crate) fn div_f64(&mut self, block: usize, span: Span, lhs: VReg, rhs: VReg) -> VReg {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::Div,
                dst: Some(dst),
                args: vec![lhs, rhs],
                imm: Imm::F64(0),
                span,
            },
        );
        dst
    }

    pub(crate) fn eq_i64(&mut self, block: usize, span: Span, lhs: VReg, rhs: VReg) -> VReg {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::Eq,
                dst: Some(dst),
                args: vec![lhs, rhs],
                imm: Imm::None,
                span,
            },
        );
        dst
    }

    pub(crate) fn eq_bool(&mut self, block: usize, span: Span, lhs: VReg, rhs: VReg) -> VReg {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::Eq,
                dst: Some(dst),
                args: vec![lhs, rhs],
                imm: Imm::Bool(true),
                span,
            },
        );
        dst
    }

    pub(crate) fn const_f64(&mut self, block: usize, span: Span, bits: u64) -> VReg {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::ConstF64,
                dst: Some(dst),
                args: Vec::new(),
                imm: Imm::F64(bits),
                span,
            },
        );
        dst
    }

    pub(crate) fn call_fn(
        &mut self,
        block: usize,
        span: Span,
        callee: usize,
        args: &[VReg],
    ) -> VReg {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::Call,
                dst: Some(dst),
                args: args.to_vec(),
                imm: Imm::Fn(callee),
                span,
            },
        );
        dst
    }

    pub(crate) fn call_import(
        &mut self,
        block: usize,
        span: Span,
        import_idx: usize,
        args: &[VReg],
    ) -> VReg {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::Call,
                dst: Some(dst),
                args: args.to_vec(),
                imm: Imm::Import(import_idx),
                span,
            },
        );
        dst
    }

    pub(crate) fn ret(&mut self, block: usize, span: Span, value: VReg) {
        self.emit(
            block,
            Inst {
                op: Op::Ret,
                dst: None,
                args: vec![value],
                imm: Imm::Ret(RetImm::Normal),
                span,
            },
        );
    }

    pub(crate) fn ret_test_fail(
        &mut self,
        block: usize,
        span: Span,
        want: VReg,
        got: VReg,
        diag_span: Span,
    ) {
        let one = self.const_i64(block, span.clone(), 1);
        self.emit(
            block,
            Inst {
                op: Op::Ret,
                dst: None,
                args: vec![one],
                imm: Imm::Ret(RetImm::TestFail {
                    want,
                    got,
                    diag_span,
                }),
                span,
            },
        );
    }

    pub(crate) fn block_ends_with_ret(&self, block: usize) -> bool {
        let fn_idx = self.cur_fn.expect("begin_function first");
        self.functions[fn_idx]
            .blocks
            .get(block)
            .and_then(|b| b.insts.last())
            .is_some_and(|inst| inst.op == Op::Ret)
    }

    pub(crate) fn block_ends_with_terminator(&self, block: usize) -> bool {
        let fn_idx = self.cur_fn.expect("begin_function first");
        self.functions[fn_idx]
            .blocks
            .get(block)
            .and_then(|b| b.insts.last())
            .is_some_and(|inst| is_terminator(inst.op))
    }

    pub(crate) fn trap_fallthrough(&mut self, block: usize, span: Span) {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::Trap,
                dst: Some(dst),
                args: Vec::new(),
                imm: Imm::Trap(TrapImm::Fallthrough),
                span,
            },
        );
    }

    pub(crate) fn ret_i64(&mut self, block: usize, span: Span, value: i64) {
        let v = self.const_i64(block, span.clone(), value);
        self.ret(block, span, v);
    }

    pub(crate) fn block_terminates(&self, block: usize) -> bool {
        self.block_ends_with_terminator(block)
    }

    pub(crate) fn block_branch_target(&self, block: usize) -> Option<usize> {
        let fn_idx = self.cur_fn.expect("begin_function first");
        let block = self.functions[fn_idx].blocks.get(block)?;
        let last = block.insts.last()?;
        if last.op == Op::Br {
            if let Imm::Block(t) = last.imm {
                return Some(t);
            }
        }
        None
    }

    pub(crate) fn jmp(&mut self, block: usize, span: Span, target: usize) {
        self.br(block, span, target);
    }

    pub(crate) fn merge_regs(
        &mut self,
        block: usize,
        span: Span,
        dst: VReg,
        pairs: &[(usize, VReg)],
    ) {
        self.phi(block, span, dst, pairs);
    }

    /// Insert phi nodes at the front of `block` (used when closing a loop).
    pub(crate) fn prepend_merge_regs(
        &mut self,
        block: usize,
        span: Span,
        phis: &[(VReg, Vec<(usize, VReg)>)],
    ) {
        let fn_idx = self.cur_fn.expect("begin_function first");
        let mut prefix = Vec::new();
        for (dst, pairs) in phis {
            let mut args = Vec::new();
            let mut blocks = Vec::new();
            for (b, v) in pairs {
                blocks.push(*b);
                args.push(*v);
            }
            prefix.push(Inst {
                op: Op::Phi,
                dst: Some(*dst),
                args,
                imm: Imm::Blocks(blocks),
                span: span.clone(),
            });
        }
        let block_insts = &mut self.functions[fn_idx].blocks[block].insts;
        block_insts.splice(0..0, prefix);
    }

    pub(crate) fn copy_i64(&mut self, block: usize, span: Span, dst: VReg, src: VReg) {
        let z = self.const_i64(block, span.clone(), 0);
        self.emit(
            block,
            Inst {
                op: Op::Add,
                dst: Some(dst),
                args: vec![src, z],
                imm: Imm::None,
                span,
            },
        );
    }

    /// Rebuild tail as back-edge copies then `br header` (SSA: `back` defs live on tail).
    pub(crate) fn refresh_tail_back_edge(
        &mut self,
        tail: usize,
        header: usize,
        span: Span,
        pairs: &[(VReg, VReg)],
    ) {
        let fn_idx = self.cur_fn.expect("begin_function first");
        self.functions[fn_idx].blocks[tail].insts.clear();
        for (back, src) in pairs {
            self.copy_i64(tail, span.clone(), *back, *src);
        }
        self.jump(tail, span, header);
    }

    fn br(&mut self, block: usize, span: Span, target: usize) {
        self.emit(
            block,
            Inst {
                op: Op::Br,
                dst: None,
                args: Vec::new(),
                imm: Imm::Block(target),
                span,
            },
        );
    }

    fn phi(&mut self, block: usize, span: Span, dst: VReg, pairs: &[(usize, VReg)]) {
        let mut args = Vec::new();
        let mut blocks = Vec::new();
        for (b, v) in pairs {
            blocks.push(*b);
            args.push(*v);
        }
        self.emit(
            block,
            Inst {
                op: Op::Phi,
                dst: Some(dst),
                args,
                imm: Imm::Blocks(blocks),
                span,
            },
        );
    }

    pub(crate) fn br_if(
        &mut self,
        block: usize,
        span: Span,
        cond: VReg,
        then_target: usize,
        else_target: usize,
    ) {
        self.emit(
            block,
            Inst {
                op: Op::BrIf,
                dst: None,
                args: vec![cond],
                imm: Imm::Blocks(vec![then_target, else_target]),
                span,
            },
        );
    }

    /// `assert.eq` desugar: on mismatch `ret` with `TestFail` sidecar, else continue.
    pub(crate) fn assert_eq_i64(
        &mut self,
        block: usize,
        span: Span,
        got: VReg,
        want: VReg,
        diag_span: Span,
    ) -> usize {
        let eq = self.eq_i64(block, span.clone(), got, want);
        let fail = self.new_block();
        let pass = self.new_block();
        self.br_if(block, span.clone(), eq, pass, fail);
        self.ret_test_fail(fail, span.clone(), want, got, diag_span);
        pass
    }

    pub(crate) fn assert_eq_bool(
        &mut self,
        block: usize,
        span: Span,
        got: VReg,
        want: VReg,
        diag_span: Span,
    ) -> usize {
        let eq = self.eq_bool(block, span.clone(), got, want);
        let fail = self.new_block();
        let pass = self.new_block();
        self.br_if(block, span.clone(), eq, pass, fail);
        self.ret_test_fail(fail, span.clone(), want, got, diag_span);
        pass
    }

    pub(crate) fn if_branch_targets(
        &mut self,
        block: usize,
        span: Span,
        cond: VReg,
    ) -> (usize, usize, usize) {
        let then_b = self.new_block();
        let else_b = self.new_block();
        let join_b = self.new_block();
        self.br_if(block, span, cond, then_b, else_b);
        (then_b, else_b, join_b)
    }

    pub(crate) fn if_join_merge(
        &mut self,
        join_b: usize,
        span: Span,
        then_end: usize,
        then_v: VReg,
        else_end: usize,
        else_v: VReg,
    ) -> VReg {
        self.br(then_end, span.clone(), join_b);
        self.br(else_end, span.clone(), join_b);
        let dst = self.alloc_vreg();
        self.phi(join_b, span, dst, &[(then_end, then_v), (else_end, else_v)]);
        dst
    }

    pub(crate) fn jump(&mut self, block: usize, span: Span, target: usize) {
        if self.block_terminates(block) {
            return;
        }
        self.jmp(block, span, target);
    }

    /// Structured `if` with one i64 join value per arm (invariant 1).
    pub(crate) fn if_join_i64<F1, F2>(
        &mut self,
        block: usize,
        span: Span,
        cond: VReg,
        then_fn: F1,
        else_fn: F2,
    ) -> Result<(usize, VReg), ()>
    where
        F1: FnOnce(&mut Builder, usize) -> Result<(usize, VReg), ()>,
        F2: FnOnce(&mut Builder, usize) -> Result<(usize, VReg), ()>,
    {
        let then_b = self.new_block();
        let else_b = self.new_block();
        let join_b = self.new_block();
        self.br_if(block, span.clone(), cond, then_b, else_b);
        let (then_end, then_v) = then_fn(self, then_b)?;
        self.br(then_end, span.clone(), join_b);
        let (else_end, else_v) = else_fn(self, else_b)?;
        self.br(else_end, span.clone(), join_b);
        let dst = self.alloc_vreg();
        self.phi(
            join_b,
            span.clone(),
            dst,
            &[(then_end, then_v), (else_end, else_v)],
        );
        Ok((join_b, dst))
    }

    pub(crate) fn trap_hole(&mut self, block: usize, span: Span) -> (usize, VReg) {
        let dst = self.alloc_vreg();
        self.emit(
            block,
            Inst {
                op: Op::Trap,
                dst: Some(dst),
                args: Vec::new(),
                imm: Imm::Trap(TrapImm::Hole),
                span,
            },
        );
        (self.new_block(), dst)
    }

    pub(crate) fn add_export(&mut self, name: &str, fn_idx: usize, is_test: bool) {
        self.exports.push(Export {
            name: name.to_string(),
            fn_idx,
            is_test,
        });
    }

    pub(crate) fn finish(self) -> Result<Program, FinishError> {
        for func in &self.functions {
            let n = func.blocks.len();
            for (bi, block) in func.blocks.iter().enumerate() {
                for inst in &block.insts {
                    match inst.op {
                        Op::Br => {
                            if let Imm::Block(t) = inst.imm {
                                if t >= n {
                                    return Err(FinishError::InvalidBranchTarget {
                                        block: bi,
                                        target: t,
                                    });
                                }
                            }
                        }
                        Op::BrIf => {
                            if let Imm::Blocks(ts) = &inst.imm {
                                for t in ts {
                                    if *t >= n {
                                        return Err(FinishError::InvalidBranchTarget {
                                            block: bi,
                                            target: *t,
                                        });
                                    }
                                }
                            }
                        }
                        Op::Phi => {
                            if let Imm::Blocks(preds) = &inst.imm {
                                if preds.len() != inst.args.len() {
                                    return Err(FinishError::InvalidPhi {
                                        block: bi,
                                        dst: inst.dst.unwrap_or(0),
                                    });
                                }
                                for p in preds {
                                    if *p >= n {
                                        return Err(FinishError::InvalidBranchTarget {
                                            block: bi,
                                            target: *p,
                                        });
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        for func in &self.functions {
            verify_function_ssa(func)?;
        }
        Ok(Program {
            functions: self.functions,
            exports: self.exports,
            fuel: self.fuel,
            imports: self.imports,
            entry_charge_span: self.entry_charge_span.clone(),
        })
    }
}

impl FinishError {
    pub(crate) fn message(&self) -> String {
        match self {
            FinishError::InvalidBranchTarget { block, target } => {
                format!("lower finish: block {block} has invalid branch target {target}")
            }
            FinishError::InvalidPhi { block, dst } => {
                format!("lower finish: block {block} phi %{dst} has mismatched predecessors")
            }
            FinishError::SsaBlockTerminator {
                fn_name,
                block,
                detail,
            } => {
                format!("ssa verify: function `{fn_name}` block {block}: {detail}")
            }
            FinishError::SsaPhiPredecessor {
                fn_name,
                block,
                phi_dst,
                pred_block,
            } => {
                format!(
                    "ssa verify: function `{fn_name}` block {block} phi %{phi_dst}: predecessor block {pred_block} is not a CFG predecessor"
                )
            }
            FinishError::SsaPhiMissingPredecessor {
                fn_name,
                block,
                phi_dst,
                pred_block,
            } => {
                format!(
                    "ssa verify: function `{fn_name}` block {block} phi %{phi_dst}: missing incoming for CFG predecessor block {pred_block}"
                )
            }
            FinishError::SsaPhiImmediate {
                fn_name,
                block,
                phi_dst,
            } => {
                format!(
                    "ssa verify: function `{fn_name}` block {block} phi %{phi_dst}: immediate is not a block list"
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::diag::Span;

    fn span() -> Span {
        Span {
            file: "t.strake".into(),
            start: 0,
            end: 1,
        }
    }

    #[test]
    fn finish_rejects_invalid_branch_target() {
        let mut b = Builder::new();
        b.begin_function("f", 0);
        let entry = 0;
        b.br(entry, span(), 99);
        let err = b.finish().unwrap_err();
        assert!(matches!(
            err,
            FinishError::InvalidBranchTarget { target: 99, .. }
        ));
    }

    #[test]
    fn finish_rejects_invalid_phi_predecessors() {
        let mut b = Builder::new();
        b.begin_function("f", 0);
        let entry = 0;
        let dst = b.alloc_vreg();
        b.phi(entry, span(), dst, &[(0, 0), (5, 0)]);
        let err = b.finish().unwrap_err();
        assert!(matches!(
            err,
            FinishError::InvalidBranchTarget { target: 5, .. }
        ));
    }

    #[test]
    fn finish_rejects_phi_pred_not_cfg_edge() {
        let mut b = Builder::new();
        b.begin_function("bad_phi", 0);
        let entry = 0;
        let arm = b.new_block();
        let join = b.new_block();
        b.br(entry, span(), arm);
        b.br(arm, span(), join);
        let dst = b.alloc_vreg();
        // entry branches to arm, not join — phi must not list entry as predecessor of join.
        b.phi(join, span(), dst, &[(entry, 0)]);
        b.ret(join, span(), dst);
        let err = b.finish().unwrap_err();
        assert!(matches!(
            err,
            FinishError::SsaPhiPredecessor {
                fn_name,
                block: 2,
                pred_block: 0,
                ..
            } if fn_name == "bad_phi"
        ));
    }

    #[test]
    fn finish_rejects_phi_missing_cfg_predecessor() {
        let mut b = Builder::new();
        b.begin_function("missing_pred", 0);
        let entry = 0;
        let then_b = b.new_block();
        let else_b = b.new_block();
        let join = b.new_block();
        let cond = b.const_i64(entry, span(), 1);
        b.br_if(entry, span(), cond, then_b, else_b);
        b.br(then_b, span(), join);
        b.br(else_b, span(), join);
        let dst = b.alloc_vreg();
        b.phi(join, span(), dst, &[(then_b, 0)]);
        b.ret(join, span(), dst);
        let err = b.finish().unwrap_err();
        assert!(matches!(
            err,
            FinishError::SsaPhiMissingPredecessor {
                fn_name,
                block: 3,
                pred_block: 2,
                ..
            } if fn_name == "missing_pred"
        ));
    }

    #[test]
    fn finish_rejects_phi_non_block_immediate() {
        let s = span();
        let func = Function {
            name: "bad_imm".into(),
            param_count: 0,
            blocks: vec![
                Block {
                    insts: vec![Inst {
                        op: Op::Br,
                        dst: None,
                        args: vec![],
                        imm: Imm::Block(1),
                        span: s.clone(),
                    }],
                },
                Block {
                    insts: vec![
                        Inst {
                            op: Op::Phi,
                            dst: Some(1),
                            args: vec![0],
                            imm: Imm::None,
                            span: s.clone(),
                        },
                        Inst {
                            op: Op::Ret,
                            dst: None,
                            args: vec![1],
                            imm: Imm::Ret(RetImm::Normal),
                            span: s.clone(),
                        },
                    ],
                },
            ],
        };
        let err = verify_function_ssa(&func).unwrap_err();
        assert!(matches!(
            err,
            FinishError::SsaPhiImmediate {
                fn_name,
                block: 1,
                phi_dst: 1,
                ..
            } if fn_name == "bad_imm"
        ));
    }
}
