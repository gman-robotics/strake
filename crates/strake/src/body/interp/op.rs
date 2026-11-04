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
    ListEmpty,
    ListCons,
    ListLen,
    ListGet,
    BytesNew,
    BytesLen,
    BytesGet,
    RecNew,
    RecGet,
    Load,
    Store,
}

/// Region access width and value kind (SPEC "Traps and region").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScalarTy {
    I32,
    I64,
    F32,
    F64,
    Bool,
}

impl ScalarTy {
    /// Byte width: `i32`/`f32` 4, `i64`/`f64` 8, `bool` 1.
    pub(crate) fn width(self) -> i128 {
        match self {
            ScalarTy::I32 | ScalarTy::F32 => 4,
            ScalarTy::I64 | ScalarTy::F64 => 8,
            ScalarTy::Bool => 1,
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "i32" => Some(ScalarTy::I32),
            "i64" => Some(ScalarTy::I64),
            "f32" => Some(ScalarTy::F32),
            "f64" => Some(ScalarTy::F64),
            "bool" => Some(ScalarTy::Bool),
            _ => None,
        }
    }
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
