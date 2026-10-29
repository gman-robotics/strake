//! Lower checked A into strake-1 via the IR builder (no opcode names).

#![allow(clippy::too_many_arguments)]

use super::carry::{
    if_join_bind_names, join_site_by_span, loop_site_by_span, match_phi_names, match_site_by_span,
    LoopSite, SiteId,
};
use super::check::Sketch;
use super::diag::Span;
use super::interp::Program;
use super::interp::{Builder, FinishError};
use super::syntax::{
    BinOp, ContinueEdge, FieldDecl, FnItem, ImportSig, Item, LoopParam, MatchArm, Module, Operand,
    PropertyItem, Rhs, Stmt, TestItem, Type, TypeDefKind,
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

struct LowerPlan<'a> {
    sketch: &'a Sketch,
    visited_loops: std::collections::HashSet<SiteId>,
    visited_joins: std::collections::HashSet<SiteId>,
    visited_matches: std::collections::HashSet<SiteId>,
    /// SSA name → record type name, for field-index projection.
    rec_ty: std::collections::HashMap<String, String>,
}

struct LoopParamLowering {
    name: String,
    header_phi: u32,
    exit_phi: u32,
    header_edges: Vec<(usize, u32)>,
    exit_edges: Vec<(usize, u32)>,
}

struct LoopFrame {
    header: usize,
    exit: usize,
    params: Vec<LoopParamLowering>,
}

fn verify_loop_frame_against_plan(
    b: &Builder,
    frame: &LoopFrame,
    preheader: usize,
    site: &LoopSite,
) -> Result<(), LowerError> {
    let first = frame
        .params
        .first()
        .ok_or_else(|| LowerError::unsupported("empty loop frame"))?;
    if first.header_edges.len() != 1 + site.backedges.len() {
        return Err(LowerError::unsupported(
            "carry plan backedge count mismatch",
        ));
    }
    let header_preds: std::collections::HashSet<usize> =
        first.header_edges.iter().map(|(blk, _)| *blk).collect();
    if !header_preds.contains(&preheader) {
        return Err(LowerError::unsupported("carry plan missing preheader edge"));
    }
    let back_count = header_preds.len() - 1;
    if back_count != site.backedges.len() {
        return Err(LowerError::unsupported("carry plan header pred mismatch"));
    }
    let mut cfg_hdr = b.cfg_predecessors(frame.header);
    cfg_hdr.sort_unstable();
    let mut hdr: Vec<usize> = header_preds.iter().copied().collect();
    hdr.sort_unstable();
    if cfg_hdr != hdr {
        return Err(LowerError::unsupported("carry plan header cfg mismatch"));
    }
    if site.breaks.is_empty() {
        if frame.params.iter().any(|p| !p.exit_edges.is_empty()) {
            return Err(LowerError::unsupported("carry plan unexpected exit phis"));
        }
        return Ok(());
    }
    let exit_preds: std::collections::HashSet<usize> = frame
        .params
        .first()
        .map(|p| p.exit_edges.iter().map(|(blk, _)| *blk).collect())
        .unwrap_or_default();
    if exit_preds.len() != site.breaks.len() {
        return Err(LowerError::unsupported("carry plan break pred mismatch"));
    }
    let mut cfg_exit = b.cfg_predecessors(frame.exit);
    cfg_exit.sort_unstable();
    let mut ex: Vec<usize> = exit_preds.iter().copied().collect();
    ex.sort_unstable();
    if cfg_exit != ex {
        return Err(LowerError::unsupported("carry plan exit cfg mismatch"));
    }
    Ok(())
}

fn finish_loop_frame(b: &mut Builder, frame: &LoopFrame, span: Span) {
    let header_phis: Vec<(u32, Vec<(usize, u32)>)> = frame
        .params
        .iter()
        .map(|p| (p.header_phi, p.header_edges.clone()))
        .collect();
    if !header_phis.is_empty() {
        b.prepend_merge_regs(frame.header, span.clone(), &header_phis);
    }
    let exit_phis: Vec<(u32, Vec<(usize, u32)>)> = frame
        .params
        .iter()
        .filter(|p| !p.exit_edges.is_empty())
        .map(|p| (p.exit_phi, p.exit_edges.clone()))
        .collect();
    if !exit_phis.is_empty() {
        b.prepend_merge_regs(frame.exit, span.clone(), &exit_phis);
    }
}

fn record_continue(
    b: &mut Builder,
    block: usize,
    span: Span,
    edges: &[ContinueEdge],
    frame: &mut LoopFrame,
    env: &std::collections::HashMap<String, u32>,
) -> Result<(), LowerError> {
    let pred = block;
    let header_phis: Vec<u32> = frame.params.iter().map(|p| p.header_phi).collect();
    let mut raw: Vec<u32> = Vec::with_capacity(frame.params.len());
    for p in &frame.params {
        let edge = edges
            .iter()
            .find(|e| e.name == p.name)
            .ok_or_else(|| LowerError::unsupported("continue names unknown loop param"))?;
        raw.push(lower_operand(b, block, &edge.value, env)?);
    }
    for (pi, v) in raw.iter().enumerate() {
        let arg = if header_phis
            .iter()
            .enumerate()
            .any(|(j, h)| j != pi && *h == *v)
        {
            let t = b.alloc_vreg();
            b.copy_i64(block, span.clone(), t, *v);
            t
        } else {
            *v
        };
        frame.params[pi].header_edges.push((pred, arg));
    }
    b.jump(block, span, frame.header);
    Ok(())
}

fn record_break(
    b: &mut Builder,
    block: usize,
    span: Span,
    frame: &mut LoopFrame,
    env: &std::collections::HashMap<String, u32>,
) -> Result<(), LowerError> {
    let pred = block;
    for p in &mut frame.params {
        let v = env
            .get(&p.name)
            .copied()
            .ok_or_else(|| LowerError::unsupported("unbound loop param on break"))?;
        p.exit_edges.push((pred, v));
    }
    b.jump(block, span, frame.exit);
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
    let mut plan = LowerPlan {
        sketch,
        visited_loops: std::collections::HashSet::new(),
        visited_joins: std::collections::HashSet::new(),
        visited_matches: std::collections::HashSet::new(),
        rec_ty: std::collections::HashMap::new(),
    };
    for item in &sketch.module.items {
        if let Item::Fn(f) = item {
            lower_fn_body(&mut b, f, &mut plan, &maps)?;
        }
    }
    for item in &sketch.module.items {
        match item {
            Item::Test(t) => {
                let export = format!("test.{}", t.name);
                let idx = lower_test(&mut b, t, &mut plan, &maps)?;
                b.add_export(&export, idx, true);
            }
            Item::Property(p) => {
                let export = format!("property.{}", p.name);
                let idx = lower_property(&mut b, p, &mut plan, &maps)?;
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
    let total_loops = sketch.carry.loops.len();
    let total_joins = sketch.carry.joins.len();
    if plan.visited_loops.len() != total_loops {
        return Err(LowerError::unsupported(
            "carry plan loop site count mismatch",
        ));
    }
    if plan.visited_joins.len() != total_joins {
        return Err(LowerError::unsupported(
            "carry plan join site count mismatch",
        ));
    }
    let total_matches = sketch.carry.matches.len();
    if plan.visited_matches.len() != total_matches {
        return Err(LowerError::unsupported(
            "carry plan match site count mismatch",
        ));
    }
    b.finish().map_err(LowerError::Finish)
}

fn lower_fn_body(
    b: &mut Builder,
    f: &FnItem,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
) -> Result<(), LowerError> {
    let fn_idx = maps.fn_indices[&f.name];
    b.select_function(fn_idx);
    let entry = 0;
    let mut env = std::collections::HashMap::new();
    plan.rec_ty.clear();
    for (i, p) in f.params.iter().enumerate() {
        env.insert(p.name.clone(), i as u32);
        if let Type::Named(name, _) = &p.ty {
            if record_decls(&plan.sketch.module, name).is_some() {
                plan.rec_ty.insert(p.name.clone(), name.clone());
            }
        }
    }
    let mut cur = entry;
    let mut loops = Vec::new();
    for stmt in &f.body {
        cur = lower_stmt(b, cur, stmt, &mut env, &mut loops, plan, maps)?;
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

fn lower_test(
    b: &mut Builder,
    t: &TestItem,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
) -> Result<usize, LowerError> {
    let idx = b.begin_function(&format!("test.{}", t.name), 0);
    lower_test_or_property_body(b, &t.body, t.span.clone(), plan, maps, "test body")?;
    Ok(idx)
}

fn lower_property(
    b: &mut Builder,
    p: &PropertyItem,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
) -> Result<usize, LowerError> {
    let idx = b.begin_function(&format!("property.{}", p.name), 0);
    lower_test_or_property_body(b, &p.body, p.span.clone(), plan, maps, "property body")?;
    Ok(idx)
}

fn lower_test_or_property_body(
    b: &mut Builder,
    body: &[Stmt],
    default_span: Span,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    body_label: &str,
) -> Result<(), LowerError> {
    plan.rec_ty.clear();
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
            plan,
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
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
) -> Result<usize, LowerError> {
    match stmt {
        Stmt::Assign { name, rhs, .. } => {
            let (block, v) = lower_assign_rhs(b, block, rhs, env, plan, maps)?;
            note_record(plan, name, rhs);
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
            b, block, span, cond, then_body, else_body, env, loops, plan, maps, None,
        ),
        Stmt::Loop { span, params, body } => {
            lower_loop(b, block, span, params, body, env, loops, plan, maps, None)
        }
        Stmt::Break { span } => {
            let frame = loops
                .last_mut()
                .ok_or_else(|| LowerError::unsupported("break outside loop"))?;
            record_break(b, block, span.clone(), frame, env)?;
            Ok(block)
        }
        Stmt::Continue { span, edges } => {
            let frame = loops
                .last_mut()
                .ok_or_else(|| LowerError::unsupported("continue outside loop"))?;
            record_continue(b, block, span.clone(), edges, frame, env)?;
            Ok(block)
        }
        Stmt::Match {
            span,
            scrutinee,
            arms,
        } => lower_match(
            b, block, span, scrutinee, arms, env, loops, plan, maps, None,
        ),
        Stmt::Trap { span } => {
            b.trap_user(block, span.clone());
            Ok(block)
        }
        _ => Err(LowerError::unsupported(
            "statement not supported in this slice",
        )),
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
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    mut env_bool: Option<&mut std::collections::HashSet<String>>,
) -> Result<usize, LowerError> {
    let site = match_site_by_span(&plan.sketch.carry, span)
        .ok_or_else(|| LowerError::unsupported("carry plan missing match site"))?
        .clone();
    if !plan.visited_matches.insert(site.id) {
        return Err(LowerError::unsupported("carry plan duplicate match site"));
    }
    let scrut_v = env
        .get(scrutinee)
        .copied()
        .ok_or_else(|| LowerError::unsupported("unbound match scrutinee"))?;
    let tag_v = b.sum_tag(block, span.clone(), scrut_v);
    let arm_entries: Vec<usize> = site.tags.iter().map(|_| b.new_block()).collect();
    b.br_table(block, span.clone(), tag_v, &arm_entries);
    let join_block = b.new_block();
    let phi_names = match_phi_names(&site);
    let any_reaches = site.arms.iter().any(|a| a.reaches);
    let mut arm_end_blocks: Vec<(usize, std::collections::HashMap<String, u32>)> = Vec::new();
    for (arm, supply) in arms.iter().zip(site.arms.iter()) {
        let arm_block = arm_entries[supply.tag_index as usize];
        let mut arm_env = env.clone();
        let mut cur = arm_block;
        if let Some(binder) = &arm.binder {
            if binder != "_" {
                let payload = b.sum_payload(cur, span.clone(), scrut_v);
                arm_env.insert(binder.clone(), payload);
            }
        }
        let saved_rec = plan.rec_ty.clone();
        for stmt in &arm.body {
            cur = lower_match_arm_stmt(
                b,
                cur,
                stmt,
                &mut arm_env,
                loops,
                plan,
                maps,
                env_bool.as_deref_mut(),
            )?;
        }
        plan.rec_ty = saved_rec;
        if supply.reaches {
            if !b.block_terminates(cur) {
                b.jump(cur, span.clone(), join_block);
            }
            let mut snap = std::collections::HashMap::new();
            for name in &supply.delivered {
                if let Some(v) = arm_env.get(name) {
                    snap.insert(name.clone(), *v);
                }
            }
            arm_end_blocks.push((cur, snap));
        }
    }
    if !any_reaches {
        b.trap_fallthrough(join_block, span.clone());
        return Ok(join_block);
    }
    let cont = b.new_block();
    for name in &phi_names {
        let mut pairs: Vec<(usize, u32)> = Vec::new();
        for (end_blk, snap) in &arm_end_blocks {
            if let Some(v) = snap.get(name) {
                pairs.push((*end_blk, *v));
            }
        }
        let dst = b.alloc_vreg();
        b.merge_regs(join_block, span.clone(), dst, &pairs);
        env.insert(name.clone(), dst);
    }
    b.jump(join_block, span.clone(), cont);
    Ok(cont)
}

fn lower_match_arm_stmt(
    b: &mut Builder,
    block: usize,
    stmt: &Stmt,
    env: &mut std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    env_bool: Option<&mut std::collections::HashSet<String>>,
) -> Result<usize, LowerError> {
    match stmt {
        Stmt::Assign { name, rhs, .. } => {
            let (block, v) = lower_assign_rhs(b, block, rhs, env, plan, maps)?;
            note_record(plan, name, rhs);
            env.insert(name.clone(), v);
            Ok(block)
        }
        Stmt::If {
            span,
            cond,
            then_body,
            else_body,
        } => lower_if(
            b, block, span, cond, then_body, else_body, env, loops, plan, maps, env_bool,
        ),
        Stmt::Loop { span, params, body } => lower_loop(
            b, block, span, params, body, env, loops, plan, maps, env_bool,
        ),
        Stmt::Match {
            span,
            scrutinee,
            arms,
        } => lower_match(
            b, block, span, scrutinee, arms, env, loops, plan, maps, env_bool,
        ),
        Stmt::Ret { span, value } => {
            let v = lower_operand(b, block, value, env)?;
            b.ret(block, span.clone(), v);
            Ok(block)
        }
        Stmt::Trap { span } => {
            b.trap_user(block, span.clone());
            Ok(block)
        }
        Stmt::Break { span } => {
            let frame = loops
                .last_mut()
                .ok_or_else(|| LowerError::unsupported("break outside loop"))?;
            record_break(b, block, span.clone(), frame, env)?;
            Ok(block)
        }
        Stmt::Continue { span, edges } => {
            let frame = loops
                .last_mut()
                .ok_or_else(|| LowerError::unsupported("continue outside loop"))?;
            record_continue(b, block, span.clone(), edges, frame, env)?;
            Ok(block)
        }
        _ => Err(LowerError::unsupported("statement in match arm")),
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
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    mut env_bool: Option<&mut std::collections::HashSet<String>>,
) -> Result<usize, LowerError> {
    let site = loop_site_by_span(&plan.sketch.carry, span)
        .ok_or_else(|| LowerError::unsupported("carry plan missing loop site"))?
        .clone();
    if !plan.visited_loops.insert(site.id) {
        return Err(LowerError::unsupported("carry plan duplicate loop site"));
    }
    if site.params.len() != params.len()
        || site
            .params
            .iter()
            .zip(params.iter())
            .any(|(a, b)| a != &b.name)
    {
        return Err(LowerError::unsupported("carry plan loop params mismatch"));
    }
    let entry = block;
    let header = b.new_block();
    let body_block = b.new_block();
    let exit = b.new_block();
    let mut frame_params: Vec<LoopParamLowering> = Vec::new();
    let mut preheader_env = env.clone();
    for p in params.iter() {
        let init = lower_operand(b, entry, &p.init, &preheader_env)?;
        let header_phi = b.alloc_vreg();
        let exit_phi = b.alloc_vreg();
        frame_params.push(LoopParamLowering {
            name: p.name.clone(),
            header_phi,
            exit_phi,
            header_edges: vec![(entry, init)],
            exit_edges: Vec::new(),
        });
        preheader_env.insert(p.name.clone(), init);
        env.insert(p.name.clone(), header_phi);
    }
    b.jump(entry, span.clone(), header);
    b.jump(header, span.clone(), body_block);
    loops.push(LoopFrame {
        header,
        exit,
        params: frame_params,
    });
    let mut cur = body_block;
    for stmt in body {
        cur = match &mut env_bool {
            Some(eb) => lower_stmt_test(b, cur, stmt, env, eb, loops, plan, maps, "loop body")?,
            None => lower_stmt(b, cur, stmt, env, loops, plan, maps)?,
        };
    }
    if !b.block_terminates(cur) {
        let pred = cur;
        let frame = loops.last_mut().expect("loop frame");
        for p in &mut frame.params {
            let v = env
                .get(&p.name)
                .copied()
                .ok_or_else(|| LowerError::unsupported("unbound loop param at fallthrough"))?;
            p.header_edges.push((pred, v));
        }
        b.jump(cur, span.clone(), header);
    }
    let frame = loops.pop().expect("loop frame");
    verify_loop_frame_against_plan(b, &frame, entry, &site)?;
    let has_exit_phis = frame.params.iter().any(|p| !p.exit_edges.is_empty());
    finish_loop_frame(b, &frame, span.clone());
    for p in &frame.params {
        env.insert(p.name.clone(), p.exit_phi);
    }
    if has_exit_phis {
        let cont = b.new_block();
        b.jump(frame.exit, span.clone(), cont);
        Ok(cont)
    } else {
        Ok(frame.exit)
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
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    mut env_bool: Option<&mut std::collections::HashSet<String>>,
) -> Result<usize, LowerError> {
    let join_site = join_site_by_span(&plan.sketch.carry, span)
        .ok_or_else(|| LowerError::unsupported("carry plan missing join site"))?;
    if !plan.visited_joins.insert(join_site.id) {
        return Err(LowerError::unsupported("carry plan duplicate join site"));
    }
    let join_names = if_join_bind_names(&plan.sketch.carry, join_site, then_body, else_body);
    if join_names.len() > 1 {
        return lower_if_multi_join(
            b,
            block,
            span,
            cond,
            then_body,
            else_body,
            env,
            loops,
            plan,
            maps,
            env_bool,
            &join_names,
        );
    }
    let join_name = join_names.first().map(String::as_str);
    let cond_v = lower_operand(b, block, cond, env)?;
    let bool_seed: Option<std::collections::HashSet<String>> =
        env_bool.as_ref().map(|s| (*s).clone());
    let (then_b, else_b, join_block) = b.if_branch_targets(block, span.clone(), cond_v);
    let mut then_bool = bool_seed.clone().unwrap_or_default();
    let saved_rec = plan.rec_ty.clone();
    let then_arm = lower_if_arm(
        b,
        then_b,
        then_body,
        env,
        loops,
        plan,
        maps,
        env_bool.is_some().then_some(&mut then_bool),
        &join_names,
    );
    plan.rec_ty = saved_rec;
    let (then_end, then_vals) =
        then_arm.map_err(|_| LowerError::unsupported("if arm lowering failed"))?;
    let then_v = arm_join_value(&then_vals, join_name, b, then_end, span);
    let mut phi_pairs: Vec<(usize, u32)> = Vec::new();
    if !b.block_terminates(then_end) {
        b.jump(then_end, span.clone(), join_block);
        phi_pairs.push((then_end, then_v));
    } else if b.block_branch_target(then_end) == Some(join_block) {
        phi_pairs.push((then_end, then_v));
    }
    let mut else_bool = bool_seed.clone().unwrap_or_default();
    let saved_rec = plan.rec_ty.clone();
    let else_arm = lower_if_arm(
        b,
        else_b,
        else_body,
        env,
        loops,
        plan,
        maps,
        env_bool.is_some().then_some(&mut else_bool),
        &join_names,
    );
    plan.rec_ty = saved_rec;
    let (else_end, else_vals) =
        else_arm.map_err(|_| LowerError::unsupported("if arm lowering failed"))?;
    let else_v = arm_join_value(&else_vals, join_name, b, else_end, span);
    if !b.block_terminates(else_end) {
        b.jump(else_end, span.clone(), join_block);
        phi_pairs.push((else_end, else_v));
    } else if b.block_branch_target(else_end) == Some(join_block) {
        phi_pairs.push((else_end, else_v));
    }
    if phi_pairs.is_empty() && b.block_terminates(then_end) && b.block_terminates(else_end) {
        if !b.block_terminates(join_block) {
            b.trap_fallthrough(join_block, span.clone());
        }
        return Ok(then_end);
    }
    let joined_v = if phi_pairs.is_empty() {
        b.alloc_vreg()
    } else {
        let dst = b.alloc_vreg();
        b.merge_regs(join_block, span.clone(), dst, &phi_pairs);
        dst
    };
    let cont = b.new_block();
    b.jump(join_block, span.clone(), cont);
    if let Some(name) = join_name {
        env.insert(name.to_string(), joined_v);
        if let Some(eb) = env_bool.as_mut() {
            if if_join_is_bool(then_body, else_body, name) {
                eb.insert(name.to_string());
            }
        }
    }
    Ok(cont)
}

fn arm_join_value(
    vals: &std::collections::HashMap<String, u32>,
    join_name: Option<&str>,
    b: &mut Builder,
    arm_end: usize,
    span: &Span,
) -> u32 {
    match join_name {
        Some(name) => vals
            .get(name)
            .copied()
            .unwrap_or_else(|| join_fill_i64(b, arm_end, span.clone())),
        None => vals
            .values()
            .next()
            .copied()
            .unwrap_or_else(|| join_fill_i64(b, arm_end, span.clone())),
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_if_multi_join(
    b: &mut Builder,
    block: usize,
    span: &Span,
    cond: &Operand,
    then_body: &[Stmt],
    else_body: &[Stmt],
    env: &mut std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    mut env_bool: Option<&mut std::collections::HashSet<String>>,
    join_names: &[String],
) -> Result<usize, LowerError> {
    let cond_v = lower_operand(b, block, cond, env)?;
    let bool_seed: Option<std::collections::HashSet<String>> =
        env_bool.as_ref().map(|s| (*s).clone());
    let (then_b, else_b, join_block) = b.if_branch_targets(block, span.clone(), cond_v);
    let mut then_bool = bool_seed.clone().unwrap_or_default();
    let saved_rec = plan.rec_ty.clone();
    let then_arm = lower_if_arm(
        b,
        then_b,
        then_body,
        env,
        loops,
        plan,
        maps,
        env_bool.is_some().then_some(&mut then_bool),
        join_names,
    );
    plan.rec_ty = saved_rec;
    let (then_end, then_vals) =
        then_arm.map_err(|_| LowerError::unsupported("if arm lowering failed"))?;
    let mut else_bool = bool_seed.clone().unwrap_or_default();
    let saved_rec = plan.rec_ty.clone();
    let else_arm = lower_if_arm(
        b,
        else_b,
        else_body,
        env,
        loops,
        plan,
        maps,
        env_bool.is_some().then_some(&mut else_bool),
        join_names,
    );
    plan.rec_ty = saved_rec;
    let (else_end, else_vals) =
        else_arm.map_err(|_| LowerError::unsupported("if arm lowering failed"))?;
    if b.block_terminates(then_end) && b.block_terminates(else_end) {
        if !b.block_terminates(join_block) {
            b.trap_fallthrough(join_block, span.clone());
        }
        return Ok(then_end);
    }
    if !b.block_terminates(then_end) {
        b.jump(then_end, span.clone(), join_block);
    }
    if !b.block_terminates(else_end) {
        b.jump(else_end, span.clone(), join_block);
    }
    for name in join_names {
        let then_v = arm_join_value(&then_vals, Some(name), b, then_end, span);
        let else_v = arm_join_value(&else_vals, Some(name), b, else_end, span);
        let mut pairs = Vec::new();
        if !b.block_terminates(then_end) || b.block_branch_target(then_end) == Some(join_block) {
            pairs.push((then_end, then_v));
        }
        if !b.block_terminates(else_end) || b.block_branch_target(else_end) == Some(join_block) {
            pairs.push((else_end, else_v));
        }
        let dst = b.alloc_vreg();
        b.merge_regs(join_block, span.clone(), dst, &pairs);
        env.insert(name.clone(), dst);
    }
    if let Some(eb) = env_bool.as_mut() {
        for name in join_names {
            if if_join_is_bool(then_body, else_body, name) {
                eb.insert(name.clone());
            }
        }
    }
    let cont = b.new_block();
    b.jump(join_block, span.clone(), cont);
    Ok(cont)
}

fn if_join_is_bool(then_body: &[Stmt], else_body: &[Stmt], name: &str) -> bool {
    fn arm_join_bool(body: &[Stmt], name: &str) -> bool {
        for stmt in body {
            if let Stmt::Assign { name: n, rhs, .. } = stmt {
                if n == name {
                    return rhs_is_bool(rhs);
                }
            }
        }
        false
    }
    arm_join_bool(then_body, name) && arm_join_bool(else_body, name)
}

fn lower_if_arm(
    b: &mut Builder,
    block: usize,
    body: &[Stmt],
    outer_env: &std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    mut env_bool: Option<&mut std::collections::HashSet<String>>,
    join_names: &[String],
) -> Result<(usize, std::collections::HashMap<String, u32>), ()> {
    let mut env = outer_env.clone();
    let mut cur = block;
    let mut join_v = None;
    for stmt in body {
        cur = match stmt {
            Stmt::Assign { name, rhs, .. } => {
                let (next, v) = lower_assign_rhs(b, cur, rhs, &env, plan, maps).map_err(|_| ())?;
                cur = next;
                note_record(plan, name, rhs);
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
                plan,
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
                    plan,
                    maps,
                    env_bool.as_deref_mut(),
                )
                .map_err(|_| ())?;
                cur
            }
            Stmt::Break { span } => {
                let frame = loops.last_mut().ok_or(())?;
                record_break(b, cur, span.clone(), frame, &env).map_err(|_| ())?;
                cur
            }
            Stmt::Continue { span, edges } => {
                let frame = loops.last_mut().ok_or(())?;
                record_continue(b, cur, span.clone(), edges, frame, &env).map_err(|_| ())?;
                cur
            }
            Stmt::Match {
                span,
                scrutinee,
                arms,
            } => lower_match(
                b,
                cur,
                span,
                scrutinee,
                arms,
                &mut env,
                loops,
                plan,
                maps,
                env_bool.as_deref_mut(),
            )
            .map_err(|_| ())?,
            Stmt::Ret { span, value } => {
                let v = lower_operand(b, cur, value, &env).map_err(|_| ())?;
                b.ret(cur, span.clone(), v);
                cur
            }
            Stmt::Trap { span } => {
                b.trap_user(cur, span.clone());
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
    let mut vals = std::collections::HashMap::new();
    if join_names.is_empty() {
        let v = join_v.unwrap_or_else(|| join_fill_i64(b, cur, span.clone()));
        vals.insert(String::new(), v);
    } else {
        for name in join_names {
            vals.insert(
                name.clone(),
                env.get(name)
                    .copied()
                    .unwrap_or_else(|| join_fill_i64(b, cur, span.clone())),
            );
        }
    }
    Ok((cur, vals))
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
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    body_label: &str,
) -> Result<usize, LowerError> {
    match stmt {
        Stmt::Assign { name, rhs, .. } => {
            let (block, v) = lower_assign_rhs(b, block, rhs, env, plan, maps)?;
            note_record(plan, name, rhs);
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
            plan,
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
            plan,
            maps,
            Some(env_bool),
        ),
        Stmt::Break { span } => {
            if b.block_terminates(block) {
                return Ok(block);
            }
            let frame = loops
                .last_mut()
                .ok_or_else(|| LowerError::unsupported("break outside loop"))?;
            record_break(b, block, span.clone(), frame, env)?;
            Ok(block)
        }
        Stmt::Continue { span, edges } => {
            if b.block_terminates(block) {
                return Ok(block);
            }
            let frame = loops
                .last_mut()
                .ok_or_else(|| LowerError::unsupported("continue outside loop"))?;
            record_continue(b, block, span.clone(), edges, frame, env)?;
            Ok(block)
        }
        Stmt::Match {
            span,
            scrutinee,
            arms,
        } => lower_match(
            b,
            block,
            span,
            scrutinee,
            arms,
            env,
            loops,
            plan,
            maps,
            Some(env_bool),
        ),
        Stmt::Trap { span } => {
            b.trap_user(block, span.clone());
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
    plan: &LowerPlan<'_>,
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
        Rhs::Ok { span, value, .. } => {
            let payload = lower_operand(b, block, value, env)?;
            Ok(b.sum_new(block, span.clone(), 0, payload))
        }
        Rhs::Err { span, value, .. } => {
            let payload = lower_operand(b, block, value, env)?;
            Ok(b.sum_new(block, span.clone(), 1, payload))
        }
        Rhs::ListEmpty { span, .. } => Ok(b.list_empty(block, span.clone())),
        Rhs::ListCons { span, head, tail } => {
            let h = lower_operand(b, block, head, env)?;
            let t = lower_operand(b, block, tail, env)?;
            Ok(b.list_cons(block, span.clone(), h, t))
        }
        Rhs::ListLen { span, list } => {
            let l = lower_operand(b, block, list, env)?;
            Ok(b.list_len(block, span.clone(), l))
        }
        Rhs::ListGet { span, list, index } => {
            let l = lower_operand(b, block, list, env)?;
            let i = lower_operand(b, block, index, env)?;
            Ok(b.list_get(block, span.clone(), l, i))
        }
        Rhs::BytesNew { span, hex } => Ok(b.bytes_new(block, span.clone(), hex)),
        Rhs::BytesLen { span, bytes } => {
            let v = lower_operand(b, block, bytes, env)?;
            Ok(b.bytes_len(block, span.clone(), v))
        }
        Rhs::BytesGet { span, bytes, index } => {
            let v = lower_operand(b, block, bytes, env)?;
            let i = lower_operand(b, block, index, env)?;
            Ok(b.bytes_get(block, span.clone(), v, i))
        }
        Rhs::Record { span, fields } => {
            let rname = match_record_name(&plan.sketch.module, fields)?;
            let decls = record_decls(&plan.sketch.module, &rname)
                .ok_or_else(|| LowerError::unsupported("record type"))?;
            let mut regs = Vec::with_capacity(decls.len());
            for decl in decls {
                let op = fields
                    .iter()
                    .find(|(n, _)| n == &decl.name)
                    .map(|(_, op)| op)
                    .ok_or_else(|| LowerError::unsupported("record field"))?;
                regs.push(lower_operand(b, block, op, env)?);
            }
            Ok(b.rec_new(block, span.clone(), &regs))
        }
        Rhs::Project { span, base, field } => {
            let base_v = env
                .get(base)
                .copied()
                .ok_or_else(|| LowerError::unsupported("unbound name in lower"))?;
            let idx = project_field_index(&plan.sketch.module, &plan.rec_ty, base, field)?;
            Ok(b.rec_get(block, span.clone(), base_v, idx))
        }
        _ => Err(LowerError::unsupported("rhs form")),
    }
}

fn note_record(plan: &mut LowerPlan<'_>, name: &str, rhs: &Rhs) {
    match bound_record_type(&plan.sketch.module, rhs, &plan.rec_ty) {
        Some(tn) => {
            plan.rec_ty.insert(name.to_string(), tn);
        }
        None => {
            plan.rec_ty.remove(name);
        }
    }
}

fn bound_record_type(
    module: &Module,
    rhs: &Rhs,
    rec_ty: &std::collections::HashMap<String, String>,
) -> Option<String> {
    match rhs {
        Rhs::Record { fields, .. } => match_record_name(module, fields).ok(),
        Rhs::Project { base, field, .. } => {
            let tn = rec_ty.get(base)?;
            let decls = record_decls(module, tn)?;
            let fty = &decls.iter().find(|f| f.name == *field)?.ty;
            match fty {
                Type::Named(n, _) if record_decls(module, n).is_some() => Some(n.clone()),
                _ => None,
            }
        }
        _ => None,
    }
}

fn record_decls<'a>(module: &'a Module, name: &str) -> Option<&'a [FieldDecl]> {
    for item in &module.items {
        if let Item::TypeDef(td) = item {
            if td.name == name {
                if let TypeDefKind::Record(fields) = &td.kind {
                    return Some(fields.as_slice());
                }
            }
        }
    }
    None
}

fn match_record_name(module: &Module, fields: &[(String, Operand)]) -> Result<String, LowerError> {
    let mut names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
    names.sort_unstable();
    if names.windows(2).any(|w| w[0] == w[1]) {
        return Err(LowerError::unsupported("record type"));
    }
    let mut hits = Vec::new();
    for item in &module.items {
        let Item::TypeDef(td) = item else {
            continue;
        };
        let TypeDefKind::Record(decls) = &td.kind else {
            continue;
        };
        let mut dn: Vec<&str> = decls.iter().map(|f| f.name.as_str()).collect();
        dn.sort_unstable();
        if dn == names {
            hits.push(td.name.clone());
        }
    }
    if hits.len() == 1 {
        Ok(hits.remove(0))
    } else {
        Err(LowerError::unsupported("record type"))
    }
}

fn project_field_index(
    module: &Module,
    rec_ty: &std::collections::HashMap<String, String>,
    base: &str,
    field: &str,
) -> Result<u32, LowerError> {
    if let Some(tn) = rec_ty.get(base) {
        let decls =
            record_decls(module, tn).ok_or_else(|| LowerError::unsupported("record type"))?;
        let idx = decls
            .iter()
            .position(|f| f.name == field)
            .ok_or_else(|| LowerError::unsupported("record field"))?;
        return u32::try_from(idx).map_err(|_| LowerError::unsupported("record field"));
    }
    let mut idxs = Vec::new();
    for item in &module.items {
        let Item::TypeDef(td) = item else {
            continue;
        };
        let TypeDefKind::Record(decls) = &td.kind else {
            continue;
        };
        if let Some(i) = decls.iter().position(|f| f.name == field) {
            if let Ok(i) = u32::try_from(i) {
                idxs.push(i);
            }
        }
    }
    idxs.sort_unstable();
    idxs.dedup();
    if idxs.len() == 1 {
        Ok(idxs[0])
    } else {
        Err(LowerError::unsupported("record field"))
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
    plan: &LowerPlan<'_>,
    maps: &CalleeMaps,
) -> Result<(usize, u32), LowerError> {
    if let Rhs::Hole { span, .. } = rhs {
        let (nb, v) = b.trap_hole(block, span.clone());
        return Ok((nb, v));
    }
    let v = lower_rhs_call(b, block, rhs, env, plan, maps)?;
    Ok((block, v))
}

fn lower_rhs_call(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
    plan: &LowerPlan<'_>,
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
    lower_rhs(b, block, rhs, env, plan)
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
        Operand::Bytes(bytes, span) => Ok(b.bytes_new(block, span.clone(), bytes)),
        Operand::Float(_, _) => Err(LowerError::unsupported("operand type")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::compile_module;
    use crate::body::interp::Op;
    use crate::body::interp::Program;
    use crate::body::syntax::{self, Operand};

    fn compile_golden(file: &str, src: &str) -> Program {
        let module = syntax::parse(src, file).expect("parse");
        compile_module(&module, false).expect("compile")
    }

    fn fn_named<'a>(prog: &'a Program, name: &str) -> &'a crate::body::interp::ir::Function {
        prog.functions
            .iter()
            .find(|f| f.name == name)
            .expect("function")
    }

    #[test]
    fn match_both_ret_join_is_trap_only() {
        let src = include_str!("../../../../golden/match_both_ret.strake");
        let prog = compile_golden("golden/match_both_ret.strake", src);
        let pick = fn_named(&prog, "pick");
        let trap_blocks = pick
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.insts.iter().any(|i| i.op == Op::Trap))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        assert!(!trap_blocks.is_empty());
        assert!(
            !pick
                .blocks
                .iter()
                .any(|b| b.insts.iter().any(|i| i.op == Op::Phi)),
            "no phi when both arms ret"
        );
    }

    #[test]
    fn match_result_br_table_tag_order_ok_err() {
        let src = include_str!("../../../../golden/match_result.strake");
        let prog = compile_golden("golden/match_result.strake", src);
        let pick = fn_named(&prog, "pick");
        let br_table = pick
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .find(|i| i.op == Op::BrTable)
            .expect("br_table");
        let crate::body::interp::ir::Imm::Blocks(targets) = &br_table.imm else {
            panic!("blocks imm");
        };
        assert_eq!(targets.len(), 2);
        let err_first = r#"strake 1

fn pick(r: result i64 unit) -> i64
  match r
    err _ ->
      out = 0
    ok v ->
      out = v
  end
  ret out
end

test pick.ok
  v = ok i64 7
  n = call pick(v)
  assert.eq n 7
  ret 0
end

test pick.err
  v = err unit unit
  n = call pick(v)
  assert.eq n 0
  ret 0
end
"#;
        let prog2 = compile_golden("err_first.strake", err_first);
        let pick2 = fn_named(&prog2, "pick");
        let br2 = pick2
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .find(|i| i.op == Op::BrTable)
            .expect("br_table");
        let crate::body::interp::ir::Imm::Blocks(t2) = &br2.imm else {
            panic!("blocks imm");
        };
        assert_eq!(
            *targets, *t2,
            "tag index order is ok then err, not source order"
        );
    }

    #[test]
    fn match_loop_fallthrough_header_predecessors() {
        use crate::body::interp::ir::{function_cfg_predecessors, Imm};

        let src = include_str!("../../../../golden/match_loop_fallthrough.strake");
        let prog = compile_golden("golden/match_loop_fallthrough.strake", src);
        let run_once = fn_named(&prog, "run_once");
        let br_table_block = run_once
            .blocks
            .iter()
            .enumerate()
            .find(|(_, b)| b.insts.iter().any(|i| i.op == Op::BrTable))
            .map(|(i, _)| i)
            .expect("br_table block");
        let header = run_once
            .blocks
            .iter()
            .enumerate()
            .find(|(bi, b)| {
                b.insts.first().is_some_and(|i| i.op == Op::Phi)
                    && function_cfg_predecessors(run_once, *bi).len() == 2
            })
            .map(|(i, _)| i)
            .expect("loop header");
        let preds = function_cfg_predecessors(run_once, header);
        assert_eq!(preds.len(), 2);
        assert!(!preds.contains(&br_table_block));
        let preheader = 0usize;
        assert!(preds.contains(&preheader));
        let match_cont = preds
            .iter()
            .find(|&p| *p != preheader)
            .copied()
            .expect("cont");
        let last = run_once.blocks[match_cont]
            .insts
            .last()
            .expect("terminator");
        assert_eq!(last.op, Op::Br);
        let Imm::Block(t) = last.imm else {
            panic!("br target");
        };
        assert_eq!(t, header);
    }

    #[test]
    fn visited_match_sites_equal_plan_len() {
        let src = include_str!("../../../../golden/match_in_if.strake");
        let module = syntax::parse(src, "golden/match_in_if.strake").expect("parse");
        let sketch = crate::body::check::check(&module, false).expect("check");
        assert_eq!(sketch.carry.matches.len(), 1);
        let prog = compile_module(&module, false).expect("lower");
        assert!(prog.functions.iter().any(|f| !f.blocks.is_empty()));
    }

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
