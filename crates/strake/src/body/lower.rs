//! Lower checked A into strake-1 via the IR builder (no opcode names).

#![allow(clippy::too_many_arguments)]

use super::check::Sketch;
use super::diag::Span;
use super::interp::Program;
use super::interp::{Builder, FinishError};
use super::syntax::{
    BinOp, FnItem, ImportSig, Item, LoopParam, Operand, PropertyItem, Rhs, Stmt, TestItem, Type,
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LowerError {
    Unsupported(String),
    Finish(FinishError),
}

struct CalleeMaps<'a> {
    fn_indices: &'a std::collections::HashMap<String, usize>,
    import_indices: &'a std::collections::HashMap<String, usize>,
}

struct LoopFrame {
    exit: usize,
    header: usize,
    tail: usize,
    back: std::collections::HashMap<String, u32>,
    /// Same names/RHS as the loop body's `continue` (for `break` carry-out probes).
    continue_edges: Vec<(String, Operand)>,
    /// Apply those carries on `break` only when `continue` immediately follows the loop's `if`.
    break_carry_on_exit: bool,
}

fn continue_directly_after_if(body: &[Stmt]) -> bool {
    body.iter()
        .enumerate()
        .find_map(|(i, stmt)| {
            if matches!(stmt, Stmt::If { .. }) {
                Some(matches!(body.get(i + 1), Some(Stmt::Continue { .. })))
            } else {
                None
            }
        })
        .unwrap_or(false)
}

fn loop_continue_edges(body: &[Stmt]) -> Vec<(String, Operand)> {
    for stmt in body.iter().rev() {
        if let Stmt::Continue { edges, .. } = stmt {
            return edges
                .iter()
                .map(|e| (e.name.clone(), e.value.clone()))
                .collect();
        }
    }
    Vec::new()
}

fn apply_break_carries(
    b: &mut Builder,
    block: usize,
    span: Span,
    frame: &LoopFrame,
    env: &std::collections::HashMap<String, u32>,
) -> Result<(), LowerError> {
    if frame.break_carry_on_exit {
        for (name, op) in &frame.continue_edges {
            let v = lower_operand(b, block, op, env)?;
            if let Some(dest) = env.get(name) {
                b.copy_i64(block, span.clone(), *dest, v);
            }
        }
    }
    Ok(())
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
    for item in &sketch.module.items {
        if let Item::Fn(f) = item {
            let idx = b.begin_function(&f.name, f.params.len());
            fn_indices.insert(f.name.clone(), idx);
        }
    }
    let maps = CalleeMaps {
        fn_indices: &fn_indices,
        import_indices: &import_indices,
    };
    for item in &sketch.module.items {
        if let Item::Fn(f) = item {
            lower_fn_body(&mut b, f, &maps)?;
        }
    }
    for item in &sketch.module.items {
        match item {
            Item::Test(t) => {
                let export = format!("test.{}", t.name);
                let idx = lower_test(&mut b, t, &maps)?;
                b.add_export(&export, idx, true);
            }
            Item::Property(p) => {
                let export = format!("property.{}", p.name);
                let idx = lower_property(&mut b, p, &maps)?;
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

fn lower_fn_body(b: &mut Builder, f: &FnItem, maps: &CalleeMaps) -> Result<(), LowerError> {
    let fn_idx = maps.fn_indices[&f.name];
    b.select_function(fn_idx);
    let entry = 0;
    let mut env = std::collections::HashMap::new();
    for (i, p) in f.params.iter().enumerate() {
        env.insert(p.name.clone(), i as u32);
    }
    let mut cur = entry;
    let mut loops = Vec::new();
    for stmt in &f.body {
        cur = lower_stmt(b, cur, stmt, &mut env, &mut loops, maps)?;
    }
    if !b.block_ends_with_terminator(cur) {
        let span = f
            .body
            .last()
            .map(stmt_span)
            .unwrap_or_else(|| f.span.clone());
        b.trap_fallthrough(cur, span);
    }
    Ok(())
}

fn lower_test(b: &mut Builder, t: &TestItem, maps: &CalleeMaps) -> Result<usize, LowerError> {
    let idx = b.begin_function(&format!("test.{}", t.name), 0);
    lower_test_or_property_body(b, &t.body, t.span.clone(), maps, "test body")?;
    Ok(idx)
}

fn lower_property(
    b: &mut Builder,
    p: &PropertyItem,
    maps: &CalleeMaps,
) -> Result<usize, LowerError> {
    let idx = b.begin_function(&format!("property.{}", p.name), 0);
    lower_test_or_property_body(b, &p.body, p.span.clone(), maps, "property body")?;
    Ok(idx)
}

fn lower_test_or_property_body(
    b: &mut Builder,
    body: &[Stmt],
    default_span: Span,
    maps: &CalleeMaps,
    body_label: &str,
) -> Result<(), LowerError> {
    let entry = 0;
    let mut env = std::collections::HashMap::new();
    let mut env_bool = std::collections::HashSet::new();
    let mut cur = entry;
    let mut loops = Vec::new();
    for stmt in body {
        cur = lower_stmt_test(
            b,
            cur,
            stmt,
            &mut env,
            &mut env_bool,
            &mut loops,
            maps,
            body_label,
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
) -> Result<usize, LowerError> {
    match stmt {
        Stmt::Assign { name, rhs, .. } => {
            let (block, v) = lower_assign_rhs(b, block, rhs, env, maps)?;
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
        } => lower_if(
            b, block, span, cond, then_body, else_body, env, loops, maps, None,
        ),
        Stmt::Loop { span, params, body } => {
            lower_loop(b, block, span, params, body, env, loops, maps, None)
        }
        Stmt::Break { span } => {
            let frame = loops
                .last()
                .ok_or_else(|| LowerError::unsupported("break outside loop"))?;
            apply_break_carries(b, block, span.clone(), frame, env)?;
            b.jump(block, span.clone(), frame.exit);
            Ok(block)
        }
        Stmt::Continue { span, edges } => {
            let frame = loops
                .last()
                .ok_or_else(|| LowerError::unsupported("continue outside loop"))?;
            let mut pairs: Vec<(u32, u32)> = Vec::new();
            for e in edges {
                let v = lower_operand(b, block, &e.value, env)?;
                if let Some(back) = frame.back.get(&e.name) {
                    pairs.push((*back, v));
                }
            }
            b.jump(block, span.clone(), frame.tail);
            b.refresh_tail_back_edge(frame.tail, frame.header, span.clone(), &pairs);
            Ok(block)
        }
        _ => Err(LowerError::unsupported(
            "statement not supported in this slice",
        )),
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
    mut env_bool: Option<&mut std::collections::HashSet<String>>,
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
    b.jump(entry, span.clone(), header);
    b.jump(header, span.clone(), body_block);
    loops.push(LoopFrame {
        exit,
        header,
        tail,
        back,
        continue_edges: loop_continue_edges(body),
        break_carry_on_exit: continue_directly_after_if(body),
    });
    b.refresh_tail_back_edge(tail, header, span.clone(), &[]);
    let mut cur = body_block;
    for stmt in body {
        cur = match &mut env_bool {
            Some(eb) => lower_stmt_test(b, cur, stmt, env, eb, loops, maps, "loop body")?,
            None => lower_stmt(b, cur, stmt, env, loops, maps)?,
        };
    }
    if !b.block_terminates(cur) {
        b.jump(cur, span.clone(), tail);
        b.refresh_tail_back_edge(tail, header, span.clone(), &[]);
    }
    loops.pop();
    Ok(exit)
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
    env_bool: Option<&mut std::collections::HashSet<String>>,
) -> Result<usize, LowerError> {
    let cond_v = lower_operand(b, block, cond, env)?;
    let join_name = if_join_name(then_body, else_body);
    let bool_seed: Option<std::collections::HashSet<String>> =
        env_bool.as_ref().map(|s| (*s).clone());
    let (then_b, else_b, join_block) = b.if_branch_targets(block, span.clone(), cond_v);
    let mut then_bool = bool_seed.clone().unwrap_or_default();
    let (then_end, then_v) = lower_if_arm(
        b,
        then_b,
        then_body,
        env,
        loops,
        maps,
        env_bool.is_some().then_some(&mut then_bool),
        join_name.as_deref(),
    )
    .map_err(|_| LowerError::unsupported("if arm lowering failed"))?;
    let mut else_bool = bool_seed.clone().unwrap_or_default();
    let (else_end, else_v) = lower_if_arm(
        b,
        else_b,
        else_body,
        env,
        loops,
        maps,
        env_bool.is_some().then_some(&mut else_bool),
        join_name.as_deref(),
    )
    .map_err(|_| LowerError::unsupported("if arm lowering failed"))?;
    let mut phi_pairs: Vec<(usize, u32)> = Vec::new();
    if !b.block_terminates(then_end) {
        b.jump(then_end, span.clone(), join_block);
        phi_pairs.push((then_end, then_v));
    } else if b.block_branch_target(then_end) == Some(join_block) {
        phi_pairs.push((then_end, then_v));
    }
    if !b.block_terminates(else_end) {
        b.jump(else_end, span.clone(), join_block);
        phi_pairs.push((else_end, else_v));
    } else if b.block_branch_target(else_end) == Some(join_block) {
        phi_pairs.push((else_end, else_v));
    }
    let joined_v = if phi_pairs.is_empty() {
        b.alloc_vreg()
    } else {
        let dst = b.alloc_vreg();
        b.merge_regs(join_block, span.clone(), dst, &phi_pairs);
        dst
    };
    if let Some(name) = join_name {
        env.insert(name.clone(), joined_v);
        if let Some(env_bool) = env_bool {
            if if_join_is_bool(then_body, else_body, &name) {
                env_bool.insert(name);
            }
        }
    }
    Ok(join_block)
}

fn if_join_is_bool(then_body: &[Stmt], else_body: &[Stmt], name: &str) -> bool {
    fn arm_join_bool(body: &[Stmt], name: &str) -> bool {
        for stmt in body {
            if let Stmt::Assign { name: n, rhs, .. } = stmt {
                return n == name && rhs_is_bool(rhs);
            }
        }
        false
    }
    arm_join_bool(then_body, name) && arm_join_bool(else_body, name)
}

fn if_join_name(then_body: &[Stmt], else_body: &[Stmt]) -> Option<String> {
    let t = top_assign_name(then_body);
    let e = top_assign_name(else_body);
    match (t, e) {
        (Some(a), Some(b)) if a == b => Some(a),
        (None, Some(name)) if loop_binds_name(then_body, &name) => Some(name),
        _ => None,
    }
}

fn loop_binds_name(body: &[Stmt], name: &str) -> bool {
    body.iter().any(|stmt| {
        matches!(
            stmt,
            Stmt::Loop { params, .. }
                if params.iter().any(|p| p.name == name)
        )
    })
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
    mut env_bool: Option<&mut std::collections::HashSet<String>>,
    arm_join: Option<&str>,
) -> Result<(usize, u32), ()> {
    let mut env = outer_env.clone();
    let mut cur = block;
    let mut join_v = None;
    for stmt in body {
        cur = match stmt {
            Stmt::Assign { name, rhs, .. } => {
                let (next, v) = lower_assign_rhs(b, cur, rhs, &env, maps).map_err(|_| ())?;
                cur = next;
                if join_v.is_none() {
                    join_v = Some(v);
                }
                env.insert(name.clone(), v);
                if let Some(env_bool) = env_bool.as_mut() {
                    if rhs_is_bool(rhs) {
                        env_bool.insert(name.clone());
                    }
                }
                cur
            }
            Stmt::If {
                span,
                cond,
                then_body,
                else_body,
            } => lower_if(
                b,
                cur,
                span,
                cond,
                then_body,
                else_body,
                &mut env,
                loops,
                maps,
                env_bool.as_deref_mut(),
            )
            .map_err(|_| ())?,
            Stmt::Loop { span, params, body } => {
                cur = lower_loop(
                    b,
                    cur,
                    span,
                    params,
                    body,
                    &mut env,
                    loops,
                    maps,
                    env_bool.as_deref_mut(),
                )
                .map_err(|_| ())?;
                cur
            }
            Stmt::Break { span } => {
                let frame = loops.last().ok_or(())?;
                apply_break_carries(b, cur, span.clone(), frame, &env).map_err(|_| ())?;
                b.jump(cur, span.clone(), frame.exit);
                cur
            }
            Stmt::Continue { span, edges } => {
                let frame = loops.last().ok_or(())?;
                let mut pairs: Vec<(u32, u32)> = Vec::new();
                for e in edges {
                    let v = lower_operand(b, cur, &e.value, &env).map_err(|_| ())?;
                    if let Some(back) = frame.back.get(&e.name) {
                        pairs.push((*back, v));
                    }
                }
                b.jump(cur, span.clone(), frame.tail);
                b.refresh_tail_back_edge(frame.tail, frame.header, span.clone(), &pairs);
                cur
            }
            Stmt::AssertEq { span, got, want } => {
                let env_bool = env_bool.as_deref_mut().ok_or(())?;
                let g = lower_operand(b, cur, got, &env).map_err(|_| ())?;
                let w = lower_operand(b, cur, want, &env).map_err(|_| ())?;
                let diag_span = span_slice_a_names(got, want, span);
                if assert_operands_are_bool(got, want, env_bool) {
                    b.assert_eq_bool(cur, span.clone(), g, w, diag_span)
                } else {
                    b.assert_eq_i64(cur, span.clone(), g, w, diag_span)
                }
            }
            _ => return Err(()),
        };
    }
    let span = Span {
        file: "lower".into(),
        start: 0,
        end: 0,
    };
    let v = match arm_join {
        Some(name) => env
            .get(name)
            .copied()
            .unwrap_or_else(|| join_fill_i64(b, cur, span)),
        None => join_v.unwrap_or_else(|| join_fill_i64(b, cur, span)),
    };
    Ok((cur, v))
}

fn join_fill_i64(b: &mut Builder, cur: usize, span: Span) -> u32 {
    if b.block_terminates(cur) {
        b.alloc_vreg()
    } else {
        b.const_i64(cur, span, 0)
    }
}

fn lower_stmt_test(
    b: &mut Builder,
    block: usize,
    stmt: &Stmt,
    env: &mut std::collections::HashMap<String, u32>,
    env_bool: &mut std::collections::HashSet<String>,
    loops: &mut Vec<LoopFrame>,
    maps: &CalleeMaps,
    body_label: &str,
) -> Result<usize, LowerError> {
    match stmt {
        Stmt::Assign { name, rhs, .. } => {
            let (block, v) = lower_assign_rhs(b, block, rhs, env, maps)?;
            env.insert(name.clone(), v);
            if rhs_is_bool(rhs) {
                env_bool.insert(name.clone());
            }
            Ok(block)
        }
        Stmt::AssertEq { span, got, want } => {
            let g = lower_operand(b, block, got, env)?;
            let w = lower_operand(b, block, want, env)?;
            let diag_span = span_slice_a_names(got, want, span);
            if assert_operands_are_bool(got, want, env_bool) {
                Ok(b.assert_eq_bool(block, span.clone(), g, w, diag_span))
            } else {
                Ok(b.assert_eq_i64(block, span.clone(), g, w, diag_span))
            }
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
            b,
            block,
            span,
            cond,
            then_body,
            else_body,
            env,
            loops,
            maps,
            Some(env_bool),
        ),
        Stmt::Loop { span, params, body } => lower_loop(
            b,
            block,
            span,
            params,
            body,
            env,
            loops,
            maps,
            Some(env_bool),
        ),
        Stmt::Break { span } => {
            if b.block_terminates(block) {
                return Ok(block);
            }
            let frame = loops
                .last()
                .ok_or_else(|| LowerError::unsupported("break outside loop"))?;
            apply_break_carries(b, block, span.clone(), frame, env)?;
            b.jump(block, span.clone(), frame.exit);
            Ok(block)
        }
        Stmt::Continue { span, edges } => {
            if b.block_terminates(block) {
                return Ok(block);
            }
            let frame = loops
                .last()
                .ok_or_else(|| LowerError::unsupported("continue outside loop"))?;
            let mut pairs: Vec<(u32, u32)> = Vec::new();
            for e in edges {
                let v = lower_operand(b, block, &e.value, env)?;
                if let Some(back) = frame.back.get(&e.name) {
                    pairs.push((*back, v));
                }
            }
            b.jump(block, span.clone(), frame.tail);
            b.refresh_tail_back_edge(frame.tail, frame.header, span.clone(), &pairs);
            Ok(block)
        }
        _ => Err(LowerError::Unsupported(format!(
            "lower: statement not supported in {body_label}"
        ))),
    }
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
        Rhs::Hole { .. } => Err(LowerError::unsupported("hole must be lowered via assign")),
        Rhs::Call { .. } => Err(LowerError::unsupported("call in rhs without indices")),
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

fn lower_assign_rhs(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
    maps: &CalleeMaps,
) -> Result<(usize, u32), LowerError> {
    if let Rhs::Hole { span, .. } = rhs {
        let (nb, v) = b.trap_hole(block, span.clone());
        return Ok((nb, v));
    }
    let v = lower_rhs_call(b, block, rhs, env, maps)?;
    Ok((block, v))
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
