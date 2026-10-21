//! Lower checked A into strake-1 via the IR builder (no opcode names).

use super::check::Sketch;
use super::diag::Span;
use super::interp::Program;
use super::interp::{Builder, FinishError};
use super::syntax::{BinOp, FnItem, Item, Operand, Rhs, Stmt, TestItem};

pub(crate) fn lower(sketch: &Sketch) -> Result<Program, FinishError> {
    let mut b = Builder::new();
    let mut fn_indices = std::collections::HashMap::new();
    for item in &sketch.module.items {
        if let Item::Fn(f) = item {
            let idx = lower_fn(&mut b, f);
            fn_indices.insert(f.name.clone(), idx);
        }
    }
    for item in &sketch.module.items {
        match item {
            Item::Test(t) => {
                let export = format!("test.{}", t.name);
                let idx = lower_test(&mut b, t, &fn_indices);
                b.add_export(&export, idx, true);
            }
            Item::Fn(f) => {
                let idx = fn_indices[&f.name];
                b.add_export(&f.name, idx, false);
            }
            _ => {}
        }
    }
    b.finish()
}

fn lower_fn(b: &mut Builder, f: &FnItem) -> usize {
    let idx = b.begin_function(&f.name, f.params.len());
    let entry = 0;
    let mut env = std::collections::HashMap::new();
    for (i, p) in f.params.iter().enumerate() {
        env.insert(p.name.clone(), i as u32);
    }
    let mut cur = entry;
    for stmt in &f.body {
        cur = lower_stmt(b, cur, stmt, &mut env, &f.name);
    }
    idx
}

fn lower_test(
    b: &mut Builder,
    t: &TestItem,
    fn_indices: &std::collections::HashMap<String, usize>,
) -> usize {
    let idx = b.begin_function(&format!("test.{}", t.name), 0);
    let entry = 0;
    let mut env = std::collections::HashMap::new();
    let mut cur = entry;
    for stmt in &t.body {
        cur = lower_stmt_test(b, cur, stmt, &mut env, fn_indices);
    }
    let unit_span = t.span.clone();
    b.ret_i64(cur, unit_span, 0);
    idx
}

fn lower_stmt(
    b: &mut Builder,
    block: usize,
    stmt: &Stmt,
    env: &mut std::collections::HashMap<String, u32>,
    _fn_name: &str,
) -> usize {
    match stmt {
        Stmt::Assign { name, rhs, .. } => {
            let v = lower_rhs(b, block, rhs, env);
            env.insert(name.clone(), v);
            block
        }
        Stmt::Ret { span, value } => {
            let v = lower_operand(b, block, value, env);
            b.ret(block, span.clone(), v);
            block
        }
        _ => block,
    }
}

fn lower_stmt_test(
    b: &mut Builder,
    block: usize,
    stmt: &Stmt,
    env: &mut std::collections::HashMap<String, u32>,
    fn_indices: &std::collections::HashMap<String, usize>,
) -> usize {
    match stmt {
        Stmt::Assign { name, rhs, .. } => {
            let v = lower_rhs_call(b, block, rhs, env, fn_indices);
            env.insert(name.clone(), v);
            block
        }
        Stmt::AssertEq { span, got, want } => {
            let g = lower_operand(b, block, got, env);
            let w = lower_operand(b, block, want, env);
            let eq = b.eq_i64(block, span.clone(), g, w);
            let fail = b.new_block();
            let pass = b.new_block();
            b.br_if(block, span.clone(), eq, pass, fail);
            b.ret_i64(fail, span.clone(), 1);
            pass
        }
        _ => block,
    }
}

fn lower_rhs(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
) -> u32 {
    match rhs {
        Rhs::BinOp {
            span, op, lhs, rhs, ..
        } => {
            let l = lower_operand(b, block, lhs, env);
            let r = lower_operand(b, block, rhs, env);
            match op {
                BinOp::Add => b.add_i64(block, span.clone(), l, r),
                BinOp::Mul => b.mul_i64(block, span.clone(), l, r),
                _ => b.add_i64(block, span.clone(), l, r),
            }
        }
        Rhs::Operand(op) => lower_operand(b, block, op, env),
        _ => b.const_i64(block, dummy_span(), 0),
    }
}

fn lower_rhs_call(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
    fn_indices: &std::collections::HashMap<String, usize>,
) -> u32 {
    if let Rhs::Call { span, callee, args } = rhs {
        let callee_idx = fn_indices[callee];
        let argv: Vec<u32> = args
            .iter()
            .map(|a| lower_operand(b, block, a, env))
            .collect();
        return b.call_fn(block, span.clone(), callee_idx, &argv);
    }
    lower_rhs(b, block, rhs, env)
}

fn lower_operand(
    b: &mut Builder,
    block: usize,
    op: &Operand,
    env: &std::collections::HashMap<String, u32>,
) -> u32 {
    match op {
        Operand::Name(name, span) => env
            .get(name)
            .copied()
            .unwrap_or_else(|| b.const_i64(block, span.clone(), 0)),
        Operand::Int(n, span) => b.const_i64(block, span.clone(), *n as i64),
        Operand::Unit(span) => b.const_i64(block, span.clone(), 0),
        Operand::Bool(v, span) => b.const_i64(block, span.clone(), if *v { 1 } else { 0 }),
        Operand::Float(_, span) => b.const_i64(block, span.clone(), 0),
        Operand::Bytes(_, span) => b.const_i64(block, span.clone(), 0),
    }
}

fn dummy_span() -> Span {
    Span {
        file: String::new(),
        start: 0,
        end: 0,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lower_source_never_names_private_opcodes() {
        let src = include_str!("lower.rs");
        let body = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(!body.contains("::Br"));
        assert!(!body.contains("::BrIf"));
        assert!(!body.contains("::Phi"));
    }
}
