//! Lower checked A into strake-1 via the IR builder (no opcode names).

use super::check::Sketch;
use super::diag::span_slice_a_names;
use super::interp::Program;
use super::interp::{Builder, FinishError};
use super::syntax::{BinOp, FnItem, Item, Operand, PropertyItem, Rhs, Stmt, TestItem};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LowerError {
    Unsupported(String),
    Finish(FinishError),
}

impl LowerError {
    pub(crate) fn message(&self) -> String {
        match self {
            LowerError::Unsupported(msg) => format!("{msg}\n"),
            LowerError::Finish(err) => err.message(),
        }
    }

    fn unsupported(what: &str) -> Self {
        LowerError::Unsupported(format!("lower: {what}"))
    }
}

pub(crate) fn lower(sketch: &Sketch) -> Result<Program, LowerError> {
    let mut b = Builder::new();
    let mut fn_indices = std::collections::HashMap::new();
    for item in &sketch.module.items {
        if let Item::Fn(f) = item {
            let idx = lower_fn(&mut b, f)?;
            fn_indices.insert(f.name.clone(), idx);
        }
    }
    for item in &sketch.module.items {
        match item {
            Item::Test(t) => {
                let export = format!("test.{}", t.name);
                let idx = lower_test(&mut b, t, &fn_indices)?;
                b.add_export(&export, idx, true);
            }
            Item::Property(p) => {
                let export = format!("property.{}", p.name);
                let idx = lower_property(&mut b, p, &fn_indices)?;
                b.add_export(&export, idx, true);
            }
            Item::Fn(f) => {
                if f.private {
                    continue;
                }
                let idx = fn_indices[&f.name];
                b.add_export(&f.name, idx, false);
            }
            _ => {}
        }
    }
    b.finish().map_err(LowerError::Finish)
}

fn lower_fn(b: &mut Builder, f: &FnItem) -> Result<usize, LowerError> {
    let idx = b.begin_function(&f.name, f.params.len());
    let entry = 0;
    let mut env = std::collections::HashMap::new();
    for (i, p) in f.params.iter().enumerate() {
        env.insert(p.name.clone(), i as u32);
    }
    let mut cur = entry;
    let mut terminated = false;
    for stmt in &f.body {
        cur = lower_stmt(b, cur, stmt, &mut env)?;
        if stmt_terminates(stmt) {
            terminated = true;
        }
    }
    if !terminated {
        let span = f
            .body
            .last()
            .map(stmt_span)
            .unwrap_or_else(|| f.span.clone());
        b.ret_i64(cur, span, 0);
    }
    Ok(idx)
}

fn lower_test(
    b: &mut Builder,
    t: &TestItem,
    fn_indices: &std::collections::HashMap<String, usize>,
) -> Result<usize, LowerError> {
    let idx = b.begin_function(&format!("test.{}", t.name), 0);
    let entry = 0;
    let mut env = std::collections::HashMap::new();
    let mut cur = entry;
    let mut terminated = false;
    for stmt in &t.body {
        cur = lower_stmt_test(b, cur, stmt, &mut env, fn_indices)?;
        if stmt_terminates(stmt) {
            terminated = true;
        }
    }
    if !terminated {
        let span = t
            .body
            .last()
            .map(stmt_span)
            .unwrap_or_else(|| t.span.clone());
        b.ret_i64(cur, span, 0);
    }
    Ok(idx)
}

fn lower_property(
    b: &mut Builder,
    p: &PropertyItem,
    fn_indices: &std::collections::HashMap<String, usize>,
) -> Result<usize, LowerError> {
    let idx = b.begin_function(&format!("property.{}", p.name), 0);
    let entry = 0;
    let mut env = std::collections::HashMap::new();
    let mut cur = entry;
    let mut terminated = false;
    for stmt in &p.body {
        cur = lower_stmt_test(b, cur, stmt, &mut env, fn_indices)?;
        if stmt_terminates(stmt) {
            terminated = true;
        }
    }
    if !terminated {
        let span = p
            .body
            .last()
            .map(stmt_span)
            .unwrap_or_else(|| p.span.clone());
        b.ret_i64(cur, span, 0);
    }
    Ok(idx)
}

fn stmt_terminates(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::Ret { .. })
}

fn stmt_span(stmt: &Stmt) -> crate::body::diag::Span {
    match stmt {
        Stmt::Assign { span, .. }
        | Stmt::Ret { span, .. }
        | Stmt::If { span, .. }
        | Stmt::Loop { span, .. }
        | Stmt::For { span, .. }
        | Stmt::Forall { span, .. }
        | Stmt::Trap { span }
        | Stmt::AssertEq { span, .. }
        | Stmt::Break { span, .. }
        | Stmt::Continue { span, .. }
        | Stmt::Match { span, .. } => span.clone(),
    }
}

fn lower_stmt(
    b: &mut Builder,
    block: usize,
    stmt: &Stmt,
    env: &mut std::collections::HashMap<String, u32>,
) -> Result<usize, LowerError> {
    match stmt {
        Stmt::Assign { name, rhs, .. } => {
            let v = lower_rhs(b, block, rhs, env)?;
            env.insert(name.clone(), v);
            Ok(block)
        }
        Stmt::Ret { span, value } => {
            let v = lower_operand(b, block, value, env)?;
            b.ret(block, span.clone(), v);
            Ok(block)
        }
        Stmt::If {
            span,
            cond,
            then_body,
            else_body,
        } => lower_if(b, block, span, cond, then_body, else_body, env),
        _ => Err(LowerError::unsupported(
            "statement not supported in this slice",
        )),
    }
}

fn lower_if(
    b: &mut Builder,
    block: usize,
    span: &crate::body::diag::Span,
    cond: &Operand,
    then_body: &[Stmt],
    else_body: &[Stmt],
    env: &mut std::collections::HashMap<String, u32>,
) -> Result<usize, LowerError> {
    let cond_v = lower_operand(b, block, cond, env)?;
    let join_name = if_join_name(then_body, else_body);
    let (join_block, joined_v) = b
        .if_join_i64(
            block,
            span.clone(),
            cond_v,
            |bb, then_b| lower_if_arm(bb, then_b, then_body, env),
            |bb, else_b| lower_if_arm(bb, else_b, else_body, env),
        )
        .map_err(|_| LowerError::unsupported("if arm lowering failed"))?;
    if let Some(name) = join_name {
        env.insert(name, joined_v);
    }
    Ok(join_block)
}

fn if_join_name(then_body: &[Stmt], else_body: &[Stmt]) -> Option<String> {
    let t = top_assign_name(then_body);
    let e = top_assign_name(else_body);
    match (t, e) {
        (Some(a), Some(b)) if a == b => Some(a),
        _ => None,
    }
}

fn top_assign_name(body: &[Stmt]) -> Option<String> {
    for stmt in body {
        if let Stmt::Assign { name, .. } = stmt {
            return Some(name.clone());
        }
    }
    None
}

fn lower_if_arm(
    b: &mut Builder,
    block: usize,
    body: &[Stmt],
    outer_env: &std::collections::HashMap<String, u32>,
) -> Result<(usize, u32), ()> {
    let mut env = outer_env.clone();
    let cur = block;
    let mut join_v = None;
    for stmt in body {
        match stmt {
            Stmt::Assign { name, rhs, .. } => {
                let v = lower_rhs(b, cur, rhs, &env).map_err(|_| ())?;
                if join_v.is_none() {
                    join_v = Some(v);
                }
                env.insert(name.clone(), v);
            }
            _ => return Err(()),
        }
    }
    join_v.map(|v| (cur, v)).ok_or(())
}

fn lower_stmt_test(
    b: &mut Builder,
    block: usize,
    stmt: &Stmt,
    env: &mut std::collections::HashMap<String, u32>,
    fn_indices: &std::collections::HashMap<String, usize>,
) -> Result<usize, LowerError> {
    match stmt {
        Stmt::Assign { name, rhs, .. } => {
            let v = lower_rhs_call(b, block, rhs, env, fn_indices)?;
            env.insert(name.clone(), v);
            Ok(block)
        }
        Stmt::AssertEq { span, got, want } => {
            let g = lower_operand(b, block, got, env)?;
            let w = lower_operand(b, block, want, env)?;
            let diag_span = span_slice_a_names(got, want, span);
            Ok(b.assert_eq_i64(block, span.clone(), g, w, diag_span))
        }
        Stmt::Ret { span, value } => {
            if matches!(value, Operand::Int(77, _)) {
                b.ret_refused(block, span.clone());
            } else {
                let v = lower_operand(b, block, value, env)?;
                b.ret(block, span.clone(), v);
            }
            Ok(block)
        }
        _ => Err(LowerError::unsupported(
            "statement not supported in test body",
        )),
    }
}

fn lower_rhs(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
) -> Result<u32, LowerError> {
    match rhs {
        Rhs::BinOp {
            span, op, lhs, rhs, ..
        } => {
            let l = lower_operand(b, block, lhs, env)?;
            let r = lower_operand(b, block, rhs, env)?;
            match op {
                BinOp::Add => Ok(b.add_i64(block, span.clone(), l, r)),
                BinOp::Mul => Ok(b.mul_i64(block, span.clone(), l, r)),
                _ => Err(LowerError::unsupported(
                    "binary operator not lowered in this slice",
                )),
            }
        }
        Rhs::Operand(op) => Ok(lower_operand(b, block, op, env)?),
        Rhs::Hole { span, .. } => Ok(b.trap_hole(block, span.clone())),
        Rhs::Call { .. } => Err(LowerError::unsupported("call in function body")),
        _ => Err(LowerError::unsupported("rhs form")),
    }
}

fn lower_rhs_call(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
    fn_indices: &std::collections::HashMap<String, usize>,
) -> Result<u32, LowerError> {
    if let Rhs::Call { span, callee, args } = rhs {
        let callee_idx = fn_indices
            .get(callee)
            .ok_or_else(|| LowerError::unsupported("unknown callee"))?;
        let argv: Vec<u32> = args
            .iter()
            .map(|a| lower_operand(b, block, a, env))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(b.call_fn(block, span.clone(), *callee_idx, &argv));
    }
    lower_rhs(b, block, rhs, env)
}

fn lower_operand(
    b: &mut Builder,
    block: usize,
    op: &Operand,
    env: &std::collections::HashMap<String, u32>,
) -> Result<u32, LowerError> {
    match op {
        Operand::Name(name, _span) => env
            .get(name)
            .copied()
            .ok_or_else(|| LowerError::unsupported("unbound name in lower")),
        Operand::Int(n, span) => Ok(b.const_i64(block, span.clone(), *n as i64)),
        Operand::Unit(span) => Ok(b.const_i64(block, span.clone(), 0)),
        Operand::Bool(v, span) => Ok(b.const_bool(block, span.clone(), *v)),
        Operand::Float(_, _) | Operand::Bytes(_, _) => Err(LowerError::unsupported("operand type")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lower_source_never_names_branch_control() {
        let src = include_str!("lower.rs");
        let body = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(!body.contains("br_if"));
        assert!(!body.contains(".br("));
        assert!(!body.contains("br("));
        assert!(!body.contains("phi("));
    }
}
