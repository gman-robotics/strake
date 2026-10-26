//! SSA program builder and finished `Program` (opcodes stay in `interp`).

use super::op::{Op, TrapImm};
use crate::body::diag::Span;

pub type VReg = u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub functions: Vec<Function>,
    pub exports: Vec<Export>,
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
    InvalidBranchTarget { block: usize, target: usize },
    InvalidPhi { block: usize, dst: VReg },
}

pub(crate) struct Builder {
    functions: Vec<Function>,
    exports: Vec<Export>,
    cur_fn: Option<usize>,
    next_vreg: VReg,
    next_block: usize,
}

impl Builder {
    pub(crate) fn new() -> Self {
        Self {
            functions: Vec::new(),
            exports: Vec::new(),
            cur_fn: None,
            next_vreg: 0,
            next_block: 0,
        }
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

    pub(crate) fn ret_i64(&mut self, block: usize, span: Span, value: i64) {
        let v = self.const_i64(block, span.clone(), value);
        self.ret(block, span, v);
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

    fn br_if(
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
            &[(then_b, then_v), (else_b, else_v)],
        );
        Ok((join_b, dst))
    }

    pub(crate) fn trap_hole(&mut self, block: usize, span: Span) -> VReg {
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
        dst
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
        Ok(Program {
            functions: self.functions,
            exports: self.exports,
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
}
