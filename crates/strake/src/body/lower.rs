//! Lower checked A into strake-1 via the IR builder (no opcode names).

#![allow(clippy::too_many_arguments)]

use super::check::Sketch;
use super::diag::Span;
use super::interp::Program;
use super::interp::{Builder, FinishError};
use super::syntax::{
    BinOp, FnItem, ForallBound, ImportSig, Item, LoopParam, MatchArm, Operand, PropertyItem, Rhs,
    StaticI64, Stmt, TestItem, Type,
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LowerError {
    Unsupported(String),
    Finish(FinishError),
}

struct CalleeMaps<'a> {
    fn_indices: &'a std::collections::HashMap<String, usize>,
    import_indices: &'a std::collections::HashMap<String, usize>,
    bool_returns: &'a std::collections::HashSet<String>,
}

struct LoopFrame {
    exit: usize,
    tail: usize,
    back: std::collections::HashMap<String, u32>,
}

struct LowerCtx<'a> {
    env_bool: Option<&'a mut std::collections::HashSet<String>>,
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
    b.set_entry_charge_span(sketch.module.span.clone());
    for item in &sketch.module.items {
        if let Item::Fuel(n, span) = item {
            b.set_fuel(*n as i64);
            b.set_entry_charge_span(span.clone());
        }
    }
    let mut import_indices = std::collections::HashMap::new();
    for item in &sketch.module.items {
        if let Item::Import(im) = item {
            if matches!(im.sig, ImportSig::Fn { .. }) {
                let idx = b.add_import(&im.path);
                import_indices.insert(im.path.clone(), idx);
            }
        }
    }
    let mut fn_indices = std::collections::HashMap::new();
    let mut bool_returns = std::collections::HashSet::new();
    for item in &sketch.module.items {
        if let Item::Fn(f) = item {
            let idx = b.begin_function(&f.name, f.params.len());
            fn_indices.insert(f.name.clone(), idx);
            if type_is_bool(&f.ret) {
                bool_returns.insert(f.name.clone());
            }
        }
    }
    let maps = CalleeMaps {
        fn_indices: &fn_indices,
        import_indices: &import_indices,
        bool_returns: &bool_returns,
    };
    for item in &sketch.module.items {
        if let Item::Fn(f) = item {
            lower_fn_body(&mut b, f, sketch, &maps)?;
        }
    }
    for item in &sketch.module.items {
        match item {
            Item::Test(t) => {
                let export = format!("test.{}", t.name);
                let idx = lower_test(&mut b, sketch, t, &maps)?;
                b.add_export(&export, idx, true);
            }
            Item::Property(p) => {
                let export = format!("property.{}", p.name);
                let idx = lower_property(&mut b, sketch, p, &maps)?;
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

fn lower_fn_body(
    b: &mut Builder,
    f: &FnItem,
    sketch: &Sketch,
    maps: &CalleeMaps,
) -> Result<(), LowerError> {
    let fn_idx = maps.fn_indices[&f.name];
    b.select_function(fn_idx);
    let entry = 0;
    let mut env = std::collections::HashMap::new();
    for (i, p) in f.params.iter().enumerate() {
        env.insert(p.name.clone(), i as u32);
    }
    let mut cur = entry;
    cur = lower_module_binds(b, cur, sketch, &mut env, maps)?;
    let mut loops = Vec::new();
    let mut ctx = LowerCtx { env_bool: None };
    for stmt in &f.body {
        cur = lower_stmt(b, cur, stmt, &mut env, &mut loops, maps, &mut ctx)?;
    }
    Ok(())
}

fn lower_test(
    b: &mut Builder,
    sketch: &Sketch,
    t: &TestItem,
    maps: &CalleeMaps,
) -> Result<usize, LowerError> {
    let idx = b.begin_function(&format!("test.{}", t.name), 0);
    lower_test_or_property_body(b, sketch, &t.body, t.span.clone(), maps, "test body")?;
    Ok(idx)
}

fn lower_property(
    b: &mut Builder,
    sketch: &Sketch,
    p: &PropertyItem,
    maps: &CalleeMaps,
) -> Result<usize, LowerError> {
    let idx = b.begin_function(&format!("property.{}", p.name), 0);
    lower_test_or_property_body(b, sketch, &p.body, p.span.clone(), maps, "property body")?;
    Ok(idx)
}

fn lower_test_or_property_body(
    b: &mut Builder,
    sketch: &Sketch,
    body: &[Stmt],
    default_span: Span,
    maps: &CalleeMaps,
    body_label: &str,
) -> Result<(), LowerError> {
    let entry = 0;
    let mut env = std::collections::HashMap::new();
    let mut env_bool = std::collections::HashSet::new();
    let mut cur = entry;
    cur = lower_module_binds(b, cur, sketch, &mut env, maps)?;
    let mut loops = Vec::new();
    let mut ctx = LowerCtx {
        env_bool: Some(&mut env_bool),
    };
    for stmt in body {
        cur = lower_stmt_test(
            b, cur, stmt, &mut env, &mut loops, maps, &mut ctx, body_label,
        )?;
    }
    if !b.block_ends_with_ret(cur) {
        let span = body.last().map(stmt_span).unwrap_or(default_span);
        b.ret_i64(cur, span, 0);
    }
    Ok(())
}

/// Narrow a diagnostic span to A-level name operands (invariant 6).
fn span_slice_a_names(got: &Operand, want: &Operand, fallback: &Span) -> Span {
    let mut spans: Vec<&Span> = Vec::new();
    if let Operand::Name(_, s) = got {
        spans.push(s);
    }
    if let Operand::Name(_, s) = want {
        spans.push(s);
    }
    if spans.is_empty() {
        return fallback.clone();
    }
    let start = spans.iter().map(|s| s.start).min().unwrap();
    let end = spans.iter().map(|s| s.end).max().unwrap();
    Span {
        file: fallback.file.clone(),
        start,
        end,
    }
}

fn stmt_span(stmt: &Stmt) -> Span {
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
    loops: &mut Vec<LoopFrame>,
    maps: &CalleeMaps,
    ctx: &mut LowerCtx<'_>,
) -> Result<usize, LowerError> {
    match stmt {
        Stmt::Assign { name, rhs, .. } => {
            let v = lower_rhs_call(b, block, rhs, env, maps)?;
            env.insert(name.clone(), v);
            if let Some(env_bool) = ctx.env_bool.as_deref_mut() {
                if rhs_is_bool(rhs)
                    || matches!(
                        rhs,
                        Rhs::Call { callee, .. } if maps.bool_returns.contains(callee)
                    )
                {
                    env_bool.insert(name.clone());
                }
            }
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
        } => lower_if(
            b, block, span, cond, then_body, else_body, env, loops, maps, ctx,
        ),
        Stmt::Loop { span, params, body } => {
            lower_loop(b, block, span, params, body, env, loops, maps, ctx)
        }
        Stmt::Break { span } => {
            let exit = loops
                .last()
                .map(|f| f.exit)
                .ok_or_else(|| LowerError::unsupported("break outside loop"))?;
            b.jmp(block, span.clone(), exit);
            Ok(block)
        }
        Stmt::Continue { span, edges } => {
            let frame = loops
                .last()
                .ok_or_else(|| LowerError::unsupported("continue outside loop"))?;
            for e in edges {
                let v = lower_operand(b, block, &e.value, env)?;
                if let Some(back) = frame.back.get(&e.name) {
                    b.copy_i64(block, span.clone(), *back, v);
                }
            }
            b.jmp(block, span.clone(), frame.tail);
            Ok(block)
        }
        Stmt::For {
            span,
            binder,
            list,
            body,
        } => lower_for(b, block, span, binder, list, body, env, loops, maps, ctx),
        Stmt::Match {
            span,
            scrutinee,
            arms,
        } => lower_match(b, block, span, scrutinee, arms, env, loops, maps, ctx),
        Stmt::Forall {
            span,
            binder,
            bound,
            body,
        } => lower_forall(b, block, span, binder, bound, body, env, loops, maps, ctx),
        Stmt::Trap { span } => {
            b.trap_user(block, span.clone());
            Ok(block)
        }
        Stmt::AssertEq { span, got, want } => {
            let env_bool = ctx
                .env_bool
                .as_deref_mut()
                .ok_or_else(|| LowerError::unsupported("assert.eq outside test/property"))?;
            let g = lower_operand(b, block, got, env)?;
            let w = lower_operand(b, block, want, env)?;
            let diag_span = span_slice_a_names(got, want, span);
            if assert_operands_are_bool(got, want, env_bool) {
                Ok(b.assert_eq_bool(block, span.clone(), g, w, diag_span))
            } else {
                Ok(b.assert_eq_i64(block, span.clone(), g, w, diag_span))
            }
        }
    }
}

fn lower_module_binds(
    b: &mut Builder,
    block: usize,
    sketch: &Sketch,
    env: &mut std::collections::HashMap<String, u32>,
    maps: &CalleeMaps,
) -> Result<usize, LowerError> {
    let cur = block;
    for item in &sketch.module.items {
        match item {
            Item::Bind { name, value, span } => {
                let v = b.const_i64(cur, span.clone(), *value as i64);
                env.insert(name.clone(), v);
            }
            Item::ListBind { name, rhs, .. } => {
                let v = lower_rhs_call(b, cur, rhs, env, maps)?;
                env.insert(name.clone(), v);
            }
            _ => {}
        }
    }
    Ok(cur)
}

fn type_id(ty: &Type) -> i32 {
    match ty {
        Type::Named(n, _) => n.len() as i32,
        Type::List(inner, _) => 10 + type_id(inner),
        Type::Result(_, _, _) => 20,
    }
}

fn lower_loop(
    b: &mut Builder,
    block: usize,
    span: &Span,
    params: &[LoopParam],
    body: &[Stmt],
    env: &mut std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    maps: &CalleeMaps,
    ctx: &mut LowerCtx<'_>,
) -> Result<usize, LowerError> {
    let entry = block;
    let header = b.new_block();
    let body_block = b.new_block();
    let tail = b.new_block();
    let exit = b.new_block();
    let mut back: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for p in params {
        let init = lower_operand(b, entry, &p.init, env)?;
        let back_v = b.alloc_vreg();
        let phi = b.alloc_vreg();
        b.merge_regs(header, span.clone(), phi, &[(entry, init), (tail, back_v)]);
        env.insert(p.name.clone(), phi);
        back.insert(p.name.clone(), back_v);
    }
    b.jmp(tail, span.clone(), header);
    b.jmp(entry, span.clone(), header);
    b.jmp(header, span.clone(), body_block);
    loops.push(LoopFrame { exit, tail, back });
    let mut cur = body_block;
    for stmt in body {
        cur = lower_stmt(b, cur, stmt, env, loops, maps, ctx)?;
    }
    if !b.block_terminates(cur) {
        b.jmp(cur, span.clone(), tail);
    }
    loops.pop();
    Ok(exit)
}

fn lower_for(
    b: &mut Builder,
    block: usize,
    span: &Span,
    binder: &str,
    list: &str,
    body: &[Stmt],
    env: &mut std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    maps: &CalleeMaps,
    ctx: &mut LowerCtx<'_>,
) -> Result<usize, LowerError> {
    let entry = block;
    let list_v = env
        .get(list)
        .copied()
        .ok_or_else(|| LowerError::unsupported("unbound list in for"))?;
    let n = b.list_len(entry, span.clone(), list_v);
    let i_init = b.const_i64(entry, span.clone(), 0);
    let header = b.new_block();
    let body_block = b.new_block();
    let ok_arm = b.new_block();
    let err_arm = b.new_block();
    let after_ok = b.new_block();
    let after_match = b.new_block();
    let exit = b.new_block();
    let i_phi = b.alloc_vreg();
    let i_back = b.alloc_vreg();
    b.jmp(entry, span.clone(), header);
    b.merge_regs(
        header,
        span.clone(),
        i_phi,
        &[(entry, i_init), (after_match, i_back)],
    );
    let done = b.ge_i64(header, span.clone(), i_phi, n);
    b.cond_branch(header, span.clone(), done, exit, body_block);
    let slot = b.list_get(body_block, span.clone(), list_v, i_phi);
    let tag = b.sum_tag(body_block, span.clone(), slot);
    b.dispatch_table(body_block, span.clone(), tag, &[ok_arm, err_arm]);
    let payload = b.sum_payload(ok_arm, span.clone(), slot);
    env.insert(binder.to_string(), payload);
    loops.push(LoopFrame {
        exit,
        tail: after_match,
        back: std::collections::HashMap::new(),
    });
    let mut cur = ok_arm;
    for stmt in body {
        cur = lower_stmt(b, cur, stmt, env, loops, maps, ctx)?;
    }
    loops.pop();
    if !b.block_terminates(cur) {
        b.jmp(cur, span.clone(), after_ok);
        b.jmp(after_ok, span.clone(), after_match);
    }
    b.trap_lower(err_arm, span.clone());
    let one = b.const_i64(after_match, span.clone(), 1);
    let next_i = b.add_i64(after_match, span.clone(), i_phi, one);
    b.copy_i64(after_match, span.clone(), i_back, next_i);
    b.jmp(after_match, span.clone(), header);
    Ok(exit)
}

fn sum_arm_block(tag: &str, ok_block: usize, err_block: usize) -> Result<usize, LowerError> {
    match tag {
        "ok" => Ok(ok_block),
        "err" => Ok(err_block),
        _ => Err(LowerError::unsupported("unknown match tag")),
    }
}

fn lower_match(
    b: &mut Builder,
    block: usize,
    span: &Span,
    scrutinee: &str,
    arms: &[MatchArm],
    env: &mut std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    maps: &CalleeMaps,
    ctx: &mut LowerCtx<'_>,
) -> Result<usize, LowerError> {
    let scr = env
        .get(scrutinee)
        .copied()
        .ok_or_else(|| LowerError::unsupported("unbound scrutinee"))?;
    let tag = b.sum_tag(block, span.clone(), scr);
    let join_name = match_join_name(arms);
    let join_block = b.new_block();
    let ok_block = b.new_block();
    let err_block = b.new_block();
    b.dispatch_table(block, span.clone(), tag, &[ok_block, err_block]);
    let mut ok_join: Option<u32> = None;
    let mut err_join: Option<u32> = None;
    for arm in arms {
        let arm_b = sum_arm_block(&arm.tag, ok_block, err_block)?;
        let mut arm_env = env.clone();
        if let Some(binder) = &arm.binder {
            if binder != "_" {
                let payload = b.sum_payload(arm_b, span.clone(), scr);
                arm_env.insert(binder.clone(), payload);
            }
        }
        let mut cur = arm_b;
        for stmt in &arm.body {
            cur = lower_stmt(b, cur, stmt, &mut arm_env, loops, maps, ctx)?;
        }
        if let Some(jn) = &join_name {
            let end_v = arm_env.get(jn).copied();
            if arm.tag == "ok" {
                ok_join = end_v;
            } else if arm.tag == "err" {
                err_join = end_v;
            }
        }
        b.jmp(cur, span.clone(), join_block);
    }
    if let Some(jn) = join_name {
        let v_ok = ok_join.unwrap_or_else(|| b.const_i64(join_block, span.clone(), 0));
        let v_err = err_join.unwrap_or_else(|| b.const_i64(join_block, span.clone(), 0));
        let joined = b.alloc_vreg();
        b.merge_regs(
            join_block,
            span.clone(),
            joined,
            &[(ok_block, v_ok), (err_block, v_err)],
        );
        env.insert(jn, joined);
    }
    Ok(join_block)
}

fn match_join_name(arms: &[MatchArm]) -> Option<String> {
    let names: Vec<Option<String>> = arms.iter().map(arm_top_assign).collect();
    let first = names.first().and_then(|n| n.clone())?;
    if names.iter().all(|n| *n == Some(first.clone())) {
        Some(first)
    } else {
        None
    }
}

fn arm_top_assign(arm: &MatchArm) -> Option<String> {
    for stmt in &arm.body {
        if let Stmt::Assign { name, .. } = stmt {
            return Some(name.clone());
        }
    }
    None
}

fn lower_forall(
    b: &mut Builder,
    block: usize,
    span: &Span,
    binder: &str,
    bound: &ForallBound,
    body: &[Stmt],
    env: &mut std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    maps: &CalleeMaps,
    ctx: &mut LowerCtx<'_>,
) -> Result<usize, LowerError> {
    match bound {
        ForallBound::Range { lo, hi, .. } => {
            let entry = block;
            let lo_v = resolve_static_i64_lower(lo, env, b, entry, span)?;
            let hi_v = resolve_static_i64_lower(hi, env, b, entry, span)?;
            let header = b.new_block();
            let body_block = b.new_block();
            let tail = b.new_block();
            let exit = b.new_block();
            let i_phi = b.alloc_vreg();
            let i_back = b.alloc_vreg();
            let one = b.const_i64(entry, span.clone(), 1);
            let hi_plus = b.add_i64(entry, span.clone(), hi_v, one);
            b.merge_regs(
                header,
                span.clone(),
                i_phi,
                &[(entry, lo_v), (tail, i_back)],
            );
            env.insert(binder.to_string(), i_phi);
            let done = b.ge_i64(header, span.clone(), i_phi, hi_plus);
            b.jmp(entry, span.clone(), header);
            b.cond_branch(header, span.clone(), done, exit, body_block);
            let mut cur = body_block;
            for stmt in body {
                cur = lower_stmt(b, cur, stmt, env, loops, maps, ctx)?;
            }
            if !b.block_terminates(cur) {
                b.jmp(cur, span.clone(), tail);
            }
            let step = b.add_i64(tail, span.clone(), i_phi, one);
            b.copy_i64(tail, span.clone(), i_back, step);
            b.jmp(tail, span.clone(), header);
            Ok(exit)
        }
        ForallBound::List { name, .. } => {
            lower_for(b, block, span, binder, name, body, env, loops, maps, ctx)
        }
    }
}

fn resolve_static_i64_lower(
    bnd: &StaticI64,
    env: &std::collections::HashMap<String, u32>,
    b: &mut Builder,
    block: usize,
    _span: &Span,
) -> Result<u32, LowerError> {
    match bnd {
        StaticI64::Lit(n, s) => Ok(b.const_i64(block, s.clone(), *n as i64)),
        StaticI64::Name(name, _s) => env
            .get(name)
            .copied()
            .ok_or_else(|| LowerError::unsupported("unbound module int bind")),
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_if(
    b: &mut Builder,
    block: usize,
    span: &Span,
    cond: &Operand,
    then_body: &[Stmt],
    else_body: &[Stmt],
    env: &mut std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    maps: &CalleeMaps,
    ctx: &mut LowerCtx<'_>,
) -> Result<usize, LowerError> {
    let cond_v = lower_operand(b, block, cond, env)?;
    let join_name = if_join_name(then_body, else_body);
    if let Some(name) = join_name {
        let then_b = b.new_block();
        let else_b = b.new_block();
        let join_b = b.new_block();
        b.cond_branch(block, span.clone(), cond_v, then_b, else_b);
        let (then_end, then_v) = lower_if_arm(b, then_b, then_body, env, loops, maps, ctx)
            .map_err(|_| LowerError::unsupported("if arm lowering failed"))?;
        b.jmp(then_end, span.clone(), join_b);
        let (else_end, else_v) = lower_if_arm(b, else_b, else_body, env, loops, maps, ctx)
            .map_err(|_| LowerError::unsupported("if arm lowering failed"))?;
        b.jmp(else_end, span.clone(), join_b);
        let joined = b.alloc_vreg();
        b.merge_regs(
            join_b,
            span.clone(),
            joined,
            &[(then_b, then_v), (else_b, else_v)],
        );
        env.insert(name, joined);
        Ok(join_b)
    } else {
        let then_b = b.new_block();
        let else_b = b.new_block();
        let join_b = b.new_block();
        b.cond_branch(block, span.clone(), cond_v, then_b, else_b);
        let then_end = lower_if_arm_stmts(b, then_b, then_body, env, loops, maps, ctx)?;
        b.jmp(then_end, span.clone(), join_b);
        let else_end = lower_if_arm_stmts(b, else_b, else_body, env, loops, maps, ctx)?;
        b.jmp(else_end, span.clone(), join_b);
        Ok(join_b)
    }
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
    loops: &mut Vec<LoopFrame>,
    maps: &CalleeMaps,
    ctx: &mut LowerCtx<'_>,
) -> Result<(usize, u32), ()> {
    let mut env = outer_env.clone();
    let mut cur = block;
    let mut join_v = None;
    for stmt in body {
        match stmt {
            Stmt::Assign { name, rhs, .. } => {
                let v = lower_rhs_call(b, cur, rhs, &env, maps).map_err(|_| ())?;
                if join_v.is_none() {
                    join_v = Some(v);
                }
                env.insert(name.clone(), v);
            }
            _ => {
                cur = lower_stmt(b, cur, stmt, &mut env, loops, maps, ctx).map_err(|_| ())?;
            }
        }
    }
    join_v.map(|v| (cur, v)).ok_or(())
}

fn lower_if_arm_stmts(
    b: &mut Builder,
    block: usize,
    body: &[Stmt],
    env: &mut std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    maps: &CalleeMaps,
    ctx: &mut LowerCtx<'_>,
) -> Result<usize, LowerError> {
    let mut cur = block;
    for stmt in body {
        cur = lower_stmt(b, cur, stmt, env, loops, maps, ctx)?;
    }
    Ok(cur)
}

fn lower_stmt_test(
    b: &mut Builder,
    block: usize,
    stmt: &Stmt,
    env: &mut std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    maps: &CalleeMaps,
    ctx: &mut LowerCtx<'_>,
    _body_label: &str,
) -> Result<usize, LowerError> {
    lower_stmt(b, block, stmt, env, loops, maps, ctx)
}

fn operand_is_bool(op: &Operand) -> bool {
    matches!(op, Operand::Bool(_, _))
}

fn rhs_is_bool(rhs: &Rhs) -> bool {
    match rhs {
        Rhs::Not { .. } => true,
        Rhs::Operand(Operand::Bool(_, _)) => true,
        Rhs::BinOp {
            ty, op: BinOp::Eq, ..
        } if type_is_bool(ty) => true,
        _ => false,
    }
}

fn assert_operands_are_bool(
    got: &Operand,
    want: &Operand,
    env_bool: &std::collections::HashSet<String>,
) -> bool {
    let got_b = operand_is_bool(got) || matches!(got, Operand::Name(n, _) if env_bool.contains(n));
    let want_b =
        operand_is_bool(want) || matches!(want, Operand::Name(n, _) if env_bool.contains(n));
    got_b && want_b
}

fn lower_rhs(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
) -> Result<u32, LowerError> {
    match rhs {
        Rhs::BinOp {
            span,
            op,
            ty,
            lhs,
            rhs,
        } => {
            let l = lower_operand(b, block, lhs, env)?;
            let r = lower_operand(b, block, rhs, env)?;
            match op {
                BinOp::Add => Ok(b.add_i64(block, span.clone(), l, r)),
                BinOp::Sub => Ok(b.sub_i64(block, span.clone(), l, r)),
                BinOp::Mul => Ok(b.mul_i64(block, span.clone(), l, r)),
                BinOp::Div => lower_div(block, span.clone(), ty, l, r, b),
                BinOp::Eq if type_is_bool(ty) => Ok(b.eq_bool(block, span.clone(), l, r)),
                BinOp::Eq => Ok(b.eq_i64(block, span.clone(), l, r)),
                _ => Err(LowerError::unsupported(
                    "binary operator not lowered in this slice",
                )),
            }
        }
        Rhs::Not { operand, span } => {
            let v = lower_operand(b, block, operand, env)?;
            let false_v = b.const_bool(block, span.clone(), false);
            Ok(b.eq_bool(block, span.clone(), v, false_v))
        }
        Rhs::Operand(op) => Ok(lower_operand(b, block, op, env)?),
        Rhs::Hole { span, .. } => Ok(b.trap_hole(block, span.clone())),
        Rhs::Call { .. } => Err(LowerError::unsupported("call in rhs without indices")),
        Rhs::ListEmpty { elem, span } => Ok(b.list_empty(block, span.clone(), type_id(elem))),
        Rhs::ListCons { head, tail, span } => {
            let h = lower_operand(b, block, head, env)?;
            let t = lower_operand(b, block, tail, env)?;
            Ok(b.list_cons(block, span.clone(), h, t))
        }
        Rhs::ListLen { list, span } => {
            let l = lower_operand(b, block, list, env)?;
            Ok(b.list_len(block, span.clone(), l))
        }
        Rhs::ListGet { list, index, span } => {
            let l = lower_operand(b, block, list, env)?;
            let i = lower_operand(b, block, index, env)?;
            Ok(b.list_get(block, span.clone(), l, i))
        }
        Rhs::Ok { value, span, .. } => {
            let v = lower_operand(b, block, value, env)?;
            Ok(b.sum_new(block, span.clone(), 0, v))
        }
        Rhs::Err { value, span, .. } => {
            let v = lower_operand(b, block, value, env)?;
            Ok(b.sum_new(block, span.clone(), 1, v))
        }
        _ => Err(LowerError::unsupported("rhs form")),
    }
}

fn type_is_bool(ty: &Type) -> bool {
    matches!(ty, Type::Named(n, _) if n == "bool")
}

fn lower_div(
    block: usize,
    span: Span,
    ty: &Type,
    l: u32,
    r: u32,
    b: &mut Builder,
) -> Result<u32, LowerError> {
    match ty {
        Type::Named(name, _) if name == "i32" => Ok(b.div_i32(block, span, l, r)),
        Type::Named(name, _) if name == "i64" => Ok(b.div_i64(block, span, l, r)),
        Type::Named(name, _) if name == "f64" => Ok(b.div_f64(block, span, l, r)),
        _ => Err(LowerError::unsupported("div type")),
    }
}

fn lower_rhs_call(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
    maps: &CalleeMaps,
) -> Result<u32, LowerError> {
    if let Rhs::Call { span, callee, args } = rhs {
        let argv: Vec<u32> = args
            .iter()
            .map(|a| lower_operand(b, block, a, env))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(callee_idx) = maps.fn_indices.get(callee) {
            return Ok(b.call_fn(block, span.clone(), *callee_idx, &argv));
        }
        if let Some(import_idx) = maps.import_indices.get(callee) {
            return Ok(b.call_import(block, span.clone(), *import_idx, &argv));
        }
        return Err(LowerError::unsupported("unknown callee"));
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
    use super::*;
    use crate::body::syntax::Operand;

    #[test]
    fn lower_source_never_names_branch_control() {
        let src = include_str!("lower.rs");
        let body = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(!body.contains("br_if"));
        assert!(!body.contains(".br("));
        assert!(!body.contains("br("));
        assert!(!body.contains("phi("));
    }

    #[test]
    fn span_slice_a_names_uses_name_operands_only() {
        let fallback = Span {
            file: "t.strake".into(),
            start: 0,
            end: 100,
        };
        let got = Operand::Name(
            "r".into(),
            Span {
                file: "t.strake".into(),
                start: 40,
                end: 41,
            },
        );
        let want = Operand::Int(
            14,
            Span {
                file: "t.strake".into(),
                start: 42,
                end: 44,
            },
        );
        let sliced = span_slice_a_names(&got, &want, &fallback);
        assert_eq!(sliced.start, 40);
        assert_eq!(sliced.end, 41);
    }
}
