//! Closed strake-1 opcode card (private to the interpreter).

#![allow(dead_code)]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Br,
    BrIf,
    BrTable,
    Call,
    Ret,
    Phi,
    Trap,
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    ConstI32,
    ConstI64,
    ConstF32,
    ConstF64,
    ConstBool,
    ConstUnit,
    SumNew,
    SumTag,
    SumPayload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrapImm {
    Hole,
    Lower,
    Import,
    Oob,
    Stack,
    Div0,
    Fuel,
    User,
    /// SSA terminator for “fall off end of block” (exec → codeless trap, exit 2).
    Fallthrough,
}
