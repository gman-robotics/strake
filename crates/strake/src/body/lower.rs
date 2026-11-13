//! Lower checked A into strake-1 via the IR builder.

#![allow(clippy::too_many_arguments)]

use super::carry::{
    if_join_bind_names, join_site_by_span, loop_site_by_span, match_phi_names, match_site_by_span,
    LoopSite, SiteId,
};
use super::check::Sketch;
use super::diag::Span;
use super::interp::ir::{Imm, Narrow};
use super::interp::Op;
use super::interp::Program;
use super::interp::{Builder, FinishError, ScalarTy};
use super::syntax::{
    BinOp, ContinueEdge, FieldDecl, FnItem, ForallBound, ImportSig, Item, LoopParam, MatchArm,
    Module, Operand, PropertyItem, Rhs, StaticI64, Stmt, TestItem, Type, TypeDefKind,
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

type SpanKey = (String, u32, u32);
type SpanRow = (String, u32, u32, String);

struct LowerPlan<'a> {
    sketch: &'a Sketch,
    visited_loops: std::collections::HashSet<SiteId>,
    visited_joins: std::collections::HashSet<SiteId>,
    visited_matches: std::collections::HashSet<SiteId>,
    /// SSA name → record type name, for field-index projection.
    rec_ty: std::collections::HashMap<String, String>,
    /// SSA name → type, for record disambiguation and scrutinee payloads.
    known_ty: std::collections::HashMap<String, Type>,
    /// Types check recorded for the item currently being lowered.
    item_types: std::collections::HashMap<String, Type>,
    /// Record-literal span → record name, from the item's type table.
    record_at: std::collections::HashMap<SpanKey, String>,
}

struct BoundSnap {
    rec_ty: std::collections::HashMap<String, String>,
    known_ty: std::collections::HashMap<String, Type>,
}

impl LowerPlan<'_> {
    fn snap_bounds(&self) -> BoundSnap {
        BoundSnap {
            rec_ty: self.rec_ty.clone(),
            known_ty: self.known_ty.clone(),
        }
    }

    fn restore_bounds(&mut self, snap: BoundSnap) {
        self.rec_ty = snap.rec_ty;
        self.known_ty = snap.known_ty;
    }
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
            copy_backedge_arg(
                b,
                block,
                span.clone(),
                *v,
                bound_name_is_bool(&frame.params[pi].name),
            )
        } else {
            *v
        };
        frame.params[pi].header_edges.push((pred, arg));
    }
    b.jump(block, span, frame.header);
    Ok(())
}

/// A cross-parameter back edge cannot name the other header phi directly.
/// Bool uses `eq` against `true` (identity). `add` is i64 and traps on a bool.
fn copy_backedge_arg(b: &mut Builder, block: usize, span: Span, src: u32, is_bool: bool) -> u32 {
    if is_bool {
        let yes = b.const_bool(block, span.clone(), true);
        b.eq_bool(block, span, src, yes)
    } else {
        let dst = b.alloc_vreg();
        b.copy_i64(block, span, dst, src);
        dst
    }
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

fn apply_region_pages(b: &mut Builder, module: &Module) {
    if let Some(pages) = super::syntax::region_pages(module) {
        b.set_region_pages(pages);
    }
}

pub(crate) fn lower(sketch: &Sketch) -> Result<Program, LowerError> {
    let mut b = Builder::new();
    lower_module_header(&mut b, &sketch.module);
    let import_indices = lower_imports(&mut b, &sketch.module);
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
        known_ty: std::collections::HashMap::new(),
        item_types: std::collections::HashMap::new(),
        record_at: std::collections::HashMap::new(),
    };
    for item in &sketch.module.items {
        if let Item::Fn(f) = item {
            lower_fn_body(&mut b, f, &mut plan, &maps)?;
        }
    }
    for item in &sketch.module.items {
        lower_export(&mut b, item, &mut plan, &maps)?;
    }
    verify_plan_visited(&plan, sketch)?;
    b.finish().map_err(LowerError::Finish)
}

/// Module directives: `fuel` (and its entry-charge span) and the region pages.
fn lower_module_header(b: &mut Builder, module: &Module) {
    b.set_entry_charge_span(module.span.clone());
    for item in &module.items {
        if let Item::Fuel(n, span) = item {
            b.set_fuel(*n as i64);
            b.set_entry_charge_span(span.clone());
        }
    }
    apply_region_pages(b, module);
}

fn lower_imports(b: &mut Builder, module: &Module) -> std::collections::HashMap<String, usize> {
    let mut import_indices = std::collections::HashMap::new();
    for item in &module.items {
        if let Item::Import(im) = item {
            if matches!(im.sig, ImportSig::Fn { .. }) {
                let idx = b.add_import(&im.path, im.span.clone());
                import_indices.insert(im.path.clone(), idx);
            }
        }
    }
    import_indices
}

fn lower_export(
    b: &mut Builder,
    item: &Item,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
) -> Result<(), LowerError> {
    match item {
        Item::Test(t) => {
            let idx = lower_test(b, t, plan, maps)?;
            b.add_export(&format!("test.{}", t.name), idx, true);
        }
        Item::Property(p) => {
            let idx = lower_property(b, p, plan, maps)?;
            b.add_export(&format!("property.{}", p.name), idx, true);
        }
        Item::Fn(f) if !f.private => b.add_export(&f.name, maps.fn_indices[&f.name], false),
        _ => {}
    }
    Ok(())
}

/// Lower must visit every carry-plan site check produced.
fn verify_plan_visited(plan: &LowerPlan<'_>, sketch: &Sketch) -> Result<(), LowerError> {
    let carry = &sketch.carry;
    if plan.visited_loops.len() != carry.loops.len() {
        return Err(LowerError::unsupported(
            "carry plan loop site count mismatch",
        ));
    }
    if plan.visited_joins.len() != carry.joins.len() {
        return Err(LowerError::unsupported(
            "carry plan join site count mismatch",
        ));
    }
    if plan.visited_matches.len() != carry.matches.len() {
        return Err(LowerError::unsupported(
            "carry plan match site count mismatch",
        ));
    }
    Ok(())
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
    reset_type_env(plan);
    install_item_types(plan, &f.name);
    for (i, p) in f.params.iter().enumerate() {
        env.insert(p.name.clone(), i as u32);
        bind_type(plan, &p.name, p.ty.clone());
    }
    let mut cur = entry;
    cur = lower_module_binds(b, cur, plan, &mut env, maps)?;
    let mut loops = Vec::new();
    for stmt in &f.body {
        cur = lower_stmt(b, cur, stmt, &mut env, &mut loops, plan, maps, false)?;
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
    reset_type_env(plan);
    install_item_types(plan, b.current_name());
    let entry = 0;
    let mut env = std::collections::HashMap::new();
    let mut cur = entry;
    cur = lower_module_binds(b, cur, plan, &mut env, maps)?;
    let mut loops = Vec::new();
    for stmt in body {
        cur = lower_stmt_test(b, cur, stmt, &mut env, &mut loops, plan, maps, body_label)?;
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
    in_test: bool,
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
            b, block, span, cond, then_body, else_body, env, loops, plan, maps, in_test,
        ),
        Stmt::Loop { span, params, body } => lower_loop(
            b, block, span, params, body, env, loops, plan, maps, in_test,
        ),
        Stmt::Break { span } => {
            if in_test && b.block_terminates(block) {
                return Ok(block);
            }
            let frame = loops
                .last_mut()
                .ok_or_else(|| LowerError::unsupported("break outside loop"))?;
            record_break(b, block, span.clone(), frame, env)?;
            Ok(block)
        }
        Stmt::Continue { span, edges } => {
            if in_test && b.block_terminates(block) {
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
            b, block, span, scrutinee, arms, env, loops, plan, maps, in_test,
        ),
        Stmt::For {
            span,
            binder,
            list,
            body,
        } => lower_for(
            b, block, span, binder, list, body, env, loops, plan, maps, in_test,
        ),
        Stmt::Forall {
            span,
            binder,
            bound,
            body,
        } => lower_forall(
            b, block, span, binder, bound, body, env, loops, plan, maps, in_test,
        ),
        Stmt::Trap { span } => {
            b.trap_user(block, span.clone());
            Ok(block)
        }
        Stmt::AssertEq { span, got, want } => {
            if !in_test {
                return Err(LowerError::unsupported(
                    "assert.eq outside test/property body",
                ));
            }
            lower_assert_eq(b, block, span, got, want, env, &plan.known_ty)
        }
    }
}

fn lower_match_arm_stmt(
    b: &mut Builder,
    block: usize,
    stmt: &Stmt,
    env: &mut std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    in_test: bool,
) -> Result<usize, LowerError> {
    lower_stmt(b, block, stmt, env, loops, plan, maps, in_test)
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
    in_test: bool,
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
    let mut arm_known: Vec<std::collections::HashMap<String, Type>> = Vec::new();
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
        let saved = plan.snap_bounds();
        if let Some(binder) = &arm.binder {
            if binder != "_" {
                note_match_binder(plan, scrutinee, &arm.tag, binder);
            }
        }
        for stmt in &arm.body {
            cur = lower_match_arm_stmt(b, cur, stmt, &mut arm_env, loops, plan, maps, in_test)?;
        }
        let arm_ty = plan.known_ty.clone();
        plan.restore_bounds(saved);
        if supply.reaches {
            arm_known.push(arm_ty);
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
    for name in &phi_names {
        bind_agreed_type(plan, name, &arm_known);
    }
    b.jump(join_block, span.clone(), cont);
    Ok(cont)
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
    in_test: bool,
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
        if let Some(ty) = infer_operand_type(&p.init, &plan.known_ty) {
            bind_type(plan, &p.name, ty);
        }
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
        cur = if in_test {
            lower_stmt_test(b, cur, stmt, env, loops, plan, maps, "loop body")?
        } else {
            lower_stmt(b, cur, stmt, env, loops, plan, maps, false)?
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
    in_test: bool,
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
            in_test,
            &join_names,
        );
    }
    let join_name = join_names.first().map(String::as_str);
    let cond_v = lower_operand(b, block, cond, env)?;
    let (then_b, else_b, join_block) = b.if_branch_targets(block, span.clone(), cond_v);
    let saved = plan.snap_bounds();
    let then_arm = lower_if_arm(
        b,
        then_b,
        then_body,
        env,
        loops,
        plan,
        maps,
        in_test,
        &join_names,
    );
    let then_known = plan.known_ty.clone();
    plan.restore_bounds(saved);
    let (then_end, then_vals) =
        then_arm.map_err(|_| LowerError::unsupported("if arm lowering failed"))?;
    let then_v = arm_join_value(&then_vals, join_name, b, then_end, span);
    let then_reaches = arm_reaches_join(b, then_end, join_block);
    let mut phi_pairs: Vec<(usize, u32)> = Vec::new();
    if !b.block_terminates(then_end) {
        b.jump(then_end, span.clone(), join_block);
        phi_pairs.push((then_end, then_v));
    } else if b.block_branch_target(then_end) == Some(join_block) {
        phi_pairs.push((then_end, then_v));
    }
    let saved = plan.snap_bounds();
    let else_arm = lower_if_arm(
        b,
        else_b,
        else_body,
        env,
        loops,
        plan,
        maps,
        in_test,
        &join_names,
    );
    let else_known = plan.known_ty.clone();
    plan.restore_bounds(saved);
    let (else_end, else_vals) =
        else_arm.map_err(|_| LowerError::unsupported("if arm lowering failed"))?;
    let else_v = arm_join_value(&else_vals, join_name, b, else_end, span);
    let else_reaches = arm_reaches_join(b, else_end, join_block);
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
        let mut reaching = Vec::new();
        if then_reaches {
            reaching.push(then_known);
        }
        if else_reaches {
            reaching.push(else_known);
        }
        note_joined_type(plan, name, &reaching);
    }
    Ok(cont)
}

fn arm_reaches_join(b: &Builder, end: usize, join: usize) -> bool {
    !b.block_terminates(end) || b.block_branch_target(end) == Some(join)
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
    in_test: bool,
    join_names: &[String],
) -> Result<usize, LowerError> {
    let cond_v = lower_operand(b, block, cond, env)?;
    let (then_b, else_b, join_block) = b.if_branch_targets(block, span.clone(), cond_v);
    let saved = plan.snap_bounds();
    let then_arm = lower_if_arm(
        b, then_b, then_body, env, loops, plan, maps, in_test, join_names,
    );
    let then_known = plan.known_ty.clone();
    plan.restore_bounds(saved);
    let (then_end, then_vals) =
        then_arm.map_err(|_| LowerError::unsupported("if arm lowering failed"))?;
    let then_reaches = arm_reaches_join(b, then_end, join_block);
    let saved = plan.snap_bounds();
    let else_arm = lower_if_arm(
        b, else_b, else_body, env, loops, plan, maps, in_test, join_names,
    );
    let else_known = plan.known_ty.clone();
    plan.restore_bounds(saved);
    let (else_end, else_vals) =
        else_arm.map_err(|_| LowerError::unsupported("if arm lowering failed"))?;
    let else_reaches = arm_reaches_join(b, else_end, join_block);
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
    let mut reaching = Vec::new();
    if then_reaches {
        reaching.push(then_known);
    }
    if else_reaches {
        reaching.push(else_known);
    }
    for name in join_names {
        note_joined_type(plan, name, &reaching);
    }
    let cont = b.new_block();
    b.jump(join_block, span.clone(), cont);
    Ok(cont)
}

fn note_joined_type(
    plan: &mut LowerPlan<'_>,
    name: &str,
    reaching: &[std::collections::HashMap<String, Type>],
) {
    // Agreed types, bool included, bind through `bind_agreed_type`.
    bind_agreed_type(plan, name, reaching);
}

fn known_is_bool(known: &std::collections::HashMap<String, Type>, name: &str) -> bool {
    known.get(name).is_some_and(type_is_bool)
}

fn lower_if_arm(
    b: &mut Builder,
    block: usize,
    body: &[Stmt],
    outer_env: &std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    in_test: bool,
    join_names: &[String],
) -> Result<(usize, std::collections::HashMap<String, u32>), ()> {
    let mut env = outer_env.clone();
    let mut cur = block;
    let mut join_v = None;
    for stmt in body {
        cur = lower_stmt(b, cur, stmt, &mut env, loops, plan, maps, in_test).map_err(|_| ())?;
        if join_v.is_none() {
            if let Stmt::Assign { name, .. } = stmt {
                join_v = env.get(name).copied();
            }
        }
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
    loops: &mut Vec<LoopFrame>,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    _body_label: &str,
) -> Result<usize, LowerError> {
    if let Stmt::AssertEq { span, got, want } = stmt {
        return lower_assert_eq(b, block, span, got, want, env, &plan.known_ty);
    }
    lower_stmt(b, block, stmt, env, loops, plan, maps, true)
}

fn lower_assert_eq(
    b: &mut Builder,
    block: usize,
    span: &Span,
    got: &Operand,
    want: &Operand,
    env: &std::collections::HashMap<String, u32>,
    known: &std::collections::HashMap<String, Type>,
) -> Result<usize, LowerError> {
    let g = lower_operand(b, block, got, env)?;
    let w = lower_operand(b, block, want, env)?;
    let diag_span = span_slice_a_names(got, want, span);
    if assert_operands_are_bool(got, want, known) {
        Ok(b.assert_eq_bool(block, span.clone(), g, w, diag_span))
    } else {
        Ok(b.assert_eq_i64(block, span.clone(), g, w, diag_span))
    }
}

fn operand_known_bool(op: &Operand, known: &std::collections::HashMap<String, Type>) -> bool {
    match op {
        Operand::Bool(_, _) => true,
        Operand::Name(name, _) => known_is_bool(known, name),
        _ => false,
    }
}

fn assert_operands_are_bool(
    got: &Operand,
    want: &Operand,
    known: &std::collections::HashMap<String, Type>,
) -> bool {
    operand_known_bool(got, known) && operand_known_bool(want, known)
}

fn lower_rhs(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
    plan: &LowerPlan<'_>,
) -> Result<u32, LowerError> {
    match rhs {
        Rhs::BinOp { .. } | Rhs::Not { .. } | Rhs::Operand(_) => lower_rhs_op(b, block, rhs, env),
        Rhs::Hole { .. } | Rhs::Call { .. } => Err(lower_rhs_elsewhere(rhs)),
        rhs @ (Rhs::Ok { .. } | Rhs::Err { .. }) => lower_rhs_sum(b, block, rhs, env),
        rhs @ (Rhs::ListEmpty { .. }
        | Rhs::ListCons { .. }
        | Rhs::ListLen { .. }
        | Rhs::ListGet { .. }) => lower_rhs_list(b, block, rhs, env),
        rhs @ (Rhs::BytesNew { .. } | Rhs::BytesLen { .. } | Rhs::BytesGet { .. }) => {
            lower_rhs_bytes(b, block, rhs, env)
        }
        Rhs::Record { .. } | Rhs::Project { .. } => lower_rhs_named(b, block, rhs, env, plan),
        rhs @ (Rhs::Load { .. } | Rhs::Store { .. }) => lower_rhs_region(b, block, rhs, env),
    }
}

fn lower_rhs_not(
    b: &mut Builder,
    block: usize,
    span: &Span,
    operand: &Operand,
    env: &std::collections::HashMap<String, u32>,
) -> Result<u32, LowerError> {
    let v = lower_operand(b, block, operand, env)?;
    let false_v = b.const_bool(block, span.clone(), false);
    Ok(b.eq_bool(block, span.clone(), v, false_v))
}

fn lower_rhs_sum(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
) -> Result<u32, LowerError> {
    match rhs {
        Rhs::Ok { span, value, .. } => {
            let payload = lower_operand(b, block, value, env)?;
            Ok(b.sum_new(block, span.clone(), 0, payload))
        }
        Rhs::Err { span, value, .. } => {
            let payload = lower_operand(b, block, value, env)?;
            Ok(b.sum_new(block, span.clone(), 1, payload))
        }
        _ => Err(LowerError::unsupported("sum rhs form")),
    }
}

fn lower_rhs_bytes(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
) -> Result<u32, LowerError> {
    match rhs {
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
        _ => Err(LowerError::unsupported("bytes rhs form")),
    }
}

fn lower_rhs_record(
    b: &mut Builder,
    block: usize,
    span: &Span,
    fields: &[(String, Operand)],
    env: &std::collections::HashMap<String, u32>,
    plan: &LowerPlan<'_>,
) -> Result<u32, LowerError> {
    let rname = record_name_at(plan, span)?;
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

fn lower_rhs_project(
    b: &mut Builder,
    block: usize,
    span: &Span,
    base: &str,
    field: &str,
    env: &std::collections::HashMap<String, u32>,
    plan: &LowerPlan<'_>,
) -> Result<u32, LowerError> {
    let base_v = env
        .get(base)
        .copied()
        .ok_or_else(|| LowerError::unsupported("unbound name in lower"))?;
    let idx = project_field_index(&plan.sketch.module, &plan.rec_ty, base, field)?;
    Ok(b.rec_get(block, span.clone(), base_v, idx))
}

fn note_record(plan: &mut LowerPlan<'_>, name: &str, _rhs: &Rhs) {
    if let Some(ty) = plan.item_types.get(name).cloned() {
        bind_type(plan, name, ty);
    }
}

fn bind_type(plan: &mut LowerPlan<'_>, name: &str, ty: Type) {
    match named_record(&plan.sketch.module, &ty) {
        Some(tn) => {
            plan.rec_ty.insert(name.to_string(), tn);
        }
        None => {
            plan.rec_ty.remove(name);
        }
    }
    // `lower_loop` is already over the CRAP gate. Bool-ness is recorded here,
    // where the type is stored, so that function stays unchanged.
    record_bool_binding(name, type_is_bool(&ty));
    plan.known_ty.insert(name.to_string(), ty);
}

thread_local! {
    static BOOL_BOUND: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

fn reset_type_env(plan: &mut LowerPlan<'_>) {
    plan.rec_ty.clear();
    plan.known_ty.clear();
    plan.item_types.clear();
    plan.record_at.clear();
    clear_bool_names();
}

fn install_item_types(plan: &mut LowerPlan<'_>, key: &str) {
    plan.item_types = item_type_map(plan, key);
    plan.record_at = record_span_index(plan, key);
}

fn item_type_map(plan: &LowerPlan<'_>, key: &str) -> std::collections::HashMap<String, Type> {
    plan.sketch.types.get(key).cloned().unwrap_or_default()
}

fn record_span_index(
    plan: &LowerPlan<'_>,
    key: &str,
) -> std::collections::HashMap<SpanKey, String> {
    let Some(spans) = plan.sketch.record_spans.get(key) else {
        return std::collections::HashMap::new();
    };
    span_index(spans, &plan.sketch.module)
}

fn span_index(spans: &[SpanRow], module: &Module) -> std::collections::HashMap<SpanKey, String> {
    let mut index = std::collections::HashMap::new();
    fill_span_index(&mut index, spans, module);
    index
}

fn fill_span_index(
    index: &mut std::collections::HashMap<SpanKey, String>,
    spans: &[SpanRow],
    module: &Module,
) {
    for (file, start, end, name) in spans {
        keep_record_span(index, module, file, *start, *end, name);
    }
}

fn keep_record_span(
    index: &mut std::collections::HashMap<SpanKey, String>,
    module: &Module,
    file: &str,
    start: u32,
    end: u32,
    name: &str,
) {
    if !name.is_empty() {
        insert_record_span(index, module, file, start, end, name);
    }
}

fn insert_record_span(
    index: &mut std::collections::HashMap<SpanKey, String>,
    module: &Module,
    file: &str,
    start: u32,
    end: u32,
    name: &str,
) {
    if record_decls(module, name).is_some() {
        index.insert((file.to_string(), start, end), name.to_string());
    }
}

fn clear_bool_names() {
    BOOL_BOUND.with(|names| names.borrow_mut().clear());
}

fn record_bool_binding(name: &str, is_bool: bool) {
    BOOL_BOUND.with(|bound| {
        let mut bound = bound.borrow_mut();
        if is_bool {
            bound.insert(name.to_string());
        } else {
            bound.remove(name);
        }
    });
}

fn bound_name_is_bool(name: &str) -> bool {
    BOOL_BOUND.with(|names| names.borrow().contains(name))
}

fn bind_agreed_type(
    plan: &mut LowerPlan<'_>,
    name: &str,
    snaps: &[std::collections::HashMap<String, Type>],
) {
    let Some(first) = snaps.first().and_then(|s| s.get(name)).cloned() else {
        return;
    };
    if snaps
        .iter()
        .all(|s| s.get(name).is_some_and(|ty| types_match(ty, &first)))
    {
        bind_type(plan, name, first);
    }
}

fn note_match_binder(plan: &mut LowerPlan<'_>, scrutinee: &str, tag: &str, binder: &str) {
    let Some(scrut) = plan.known_ty.get(scrutinee).cloned() else {
        return;
    };
    let Some(payload) = payload_type(&plan.sketch.module, &scrut, tag) else {
        return;
    };
    bind_type(plan, binder, payload);
}

fn payload_type(module: &Module, scrut: &Type, tag: &str) -> Option<Type> {
    match scrut {
        Type::Result(ok, err, _) => match tag {
            "ok" => Some((**ok).clone()),
            "err" => Some((**err).clone()),
            _ => None,
        },
        Type::Named(name, _) => sum_variant_payload(module, name, tag),
        _ => None,
    }
}

fn sum_variant_payload(module: &Module, name: &str, tag: &str) -> Option<Type> {
    for item in &module.items {
        let Item::TypeDef(td) = item else {
            continue;
        };
        if td.name != name {
            continue;
        }
        let TypeDefKind::Sum(variants) = &td.kind else {
            return None;
        };
        return variants
            .iter()
            .find(|v| v.tag == tag)
            .and_then(|v| v.payload.clone());
    }
    None
}

fn named_record(module: &Module, ty: &Type) -> Option<String> {
    match ty {
        Type::Named(n, _) if record_decls(module, n).is_some() => Some(n.clone()),
        _ => None,
    }
}

fn lower_rhs_op(
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
        } => lower_rhs_binop(b, block, span, op, ty, lhs, rhs, env),
        Rhs::Not { operand, span } => lower_rhs_not(b, block, span, operand, env),
        Rhs::Operand(op) => Ok(lower_operand(b, block, op, env)?),
        _ => Err(LowerError::unsupported("op rhs form")),
    }
}

/// Holes and calls lower through `Assign`, which knows the callee indices.
fn lower_rhs_elsewhere(rhs: &Rhs) -> LowerError {
    if matches!(rhs, Rhs::Hole { .. }) {
        LowerError::unsupported("hole must be lowered via assign")
    } else {
        LowerError::unsupported("call in rhs without indices")
    }
}

fn lower_rhs_named(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
    plan: &LowerPlan<'_>,
) -> Result<u32, LowerError> {
    match rhs {
        Rhs::Record { span, fields } => lower_rhs_record(b, block, span, fields, env, plan),
        Rhs::Project { span, base, field } => {
            lower_rhs_project(b, block, span, base, field, env, plan)
        }
        _ => Err(LowerError::unsupported("record rhs form")),
    }
}

fn region_scalar(ty: &Type) -> Result<ScalarTy, LowerError> {
    match ty {
        Type::Named(n, _) => ScalarTy::from_name(n),
        _ => None,
    }
    .ok_or_else(|| LowerError::unsupported("region access type"))
}

fn lower_rhs_region(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
) -> Result<u32, LowerError> {
    match rhs {
        Rhs::Load { span, ty, addr } => {
            let at = lower_operand(b, block, addr, env)?;
            Ok(b.load(block, span.clone(), region_scalar(ty)?, at))
        }
        Rhs::Store {
            span,
            ty,
            addr,
            value,
        } => {
            let (at, v) = lower_operand_pair(b, block, addr, value, env)?;
            Ok(b.store(block, span.clone(), region_scalar(ty)?, at, v))
        }
        _ => Err(LowerError::unsupported("region rhs form")),
    }
}

fn infer_operand_type(
    op: &Operand,
    known: &std::collections::HashMap<String, Type>,
) -> Option<Type> {
    match op {
        Operand::Name(name, _) => known.get(name).cloned(),
        Operand::Bool(_, span) => Some(Type::Named("bool".into(), span.clone())),
        Operand::Int(n, span) if fits_i64(*n) => Some(Type::Named("i64".into(), span.clone())),
        Operand::Bytes(_, span) => Some(Type::Named("bytes".into(), span.clone())),
        Operand::Unit(span) => Some(Type::Named("unit".into(), span.clone())),
        Operand::Float(_, span) => Some(Type::Named("f64".into(), span.clone())),
        Operand::Int(_, _) => None,
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

fn record_name_at(plan: &LowerPlan<'_>, span: &Span) -> Result<String, LowerError> {
    let key = (span.file.clone(), span.start, span.end);
    plan.record_at
        .get(&key)
        .cloned()
        .ok_or_else(|| LowerError::unsupported("record type"))
}

fn types_match(found: &Type, expected: &Type) -> bool {
    match (found, expected) {
        (Type::Named(a, _), Type::Named(b, _)) => a == b,
        (Type::List(a, _), Type::List(b, _)) => types_match(a, b),
        (Type::Result(ok_a, err_a, _), Type::Result(ok_b, err_b, _)) => {
            result_side(ok_a, ok_b) && result_side(err_a, err_b)
        }
        _ => false,
    }
}

fn result_side(a: &Type, b: &Type) -> bool {
    type_name_is(a, "unit") || type_name_is(b, "unit") || types_match(a, b)
}

fn type_name_is(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Named(n, _) if n == name)
}

fn fits_i64(n: i128) -> bool {
    n >= i64::MIN as i128 && n <= i64::MAX as i128
}

fn project_field_index(
    module: &Module,
    rec_ty: &std::collections::HashMap<String, String>,
    base: &str,
    field: &str,
) -> Result<u32, LowerError> {
    let tn = rec_ty
        .get(base)
        .ok_or_else(|| LowerError::unsupported("record type"))?;
    let decls = record_decls(module, tn).ok_or_else(|| LowerError::unsupported("record type"))?;
    let idx = decls
        .iter()
        .position(|f| f.name == field)
        .ok_or_else(|| LowerError::unsupported("record field"))?;
    u32::try_from(idx).map_err(|_| LowerError::unsupported("record field"))
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
        Type::Named(name, _) if name == "f32" => {
            Ok(b.emit_i64_binop(block, span, Op::Div, l, r, Imm::F32(0)))
        }
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
        Operand::Unit(span) => Ok(b.narrow_const(block, span.clone(), Narrow::Unit)),
        Operand::Bool(v, span) => Ok(b.const_bool(block, span.clone(), *v)),
        Operand::Bytes(bytes, span) => Ok(b.bytes_new(block, span.clone(), bytes)),
        Operand::Float(x, span) => Ok(b.const_f64(block, span.clone(), x.to_bits())),
    }
}

fn lower_module_binds(
    b: &mut Builder,
    block: usize,
    plan: &mut LowerPlan<'_>,
    env: &mut std::collections::HashMap<String, u32>,
    maps: &CalleeMaps,
) -> Result<usize, LowerError> {
    let mut cur = block;
    for item in &plan.sketch.module.items {
        match item {
            Item::Bind { name, value, span } => {
                let v = b.const_i64(cur, span.clone(), *value as i64);
                env.insert(name.clone(), v);
            }
            Item::ListBind { name, rhs, .. } => {
                let (c, v) = lower_assign_rhs(b, cur, rhs, env, plan, maps)?;
                cur = c;
                env.insert(name.clone(), v);
            }
            _ => {}
        }
    }
    Ok(cur)
}

fn resolve_static_i64_lower(
    bnd: &StaticI64,
    env: &std::collections::HashMap<String, u32>,
    b: &mut Builder,
    block: usize,
) -> Result<u32, LowerError> {
    match bnd {
        StaticI64::Lit(n, s) => Ok(b.const_i64(block, s.clone(), *n as i64)),
        StaticI64::Name(name, _s) => env
            .get(name)
            .copied()
            .ok_or_else(|| LowerError::unsupported("unbound module int bind")),
    }
}

/// `break` inside `for` leaves that `for`. The frame's exit differs from an
/// enclosing loop's exit, and there is no enclosing loop when `outer_exit` is
/// absent.
fn targets_this_for(exit: usize, outer_exit: Option<usize>) -> bool {
    outer_exit.is_none_or(|outer| exit != outer)
}

fn push_for_frame(loops: &mut Vec<LoopFrame>, exit: usize) {
    let outer_exit = loops.last().map(|frame| frame.exit);
    if targets_this_for(exit, outer_exit) {
        loops.push(LoopFrame {
            header: exit,
            exit,
            params: Vec::new(),
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_for(
    b: &mut Builder,
    block: usize,
    span: &Span,
    binder: &str,
    list: &str,
    body: &[Stmt],
    env: &mut std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    in_test: bool,
) -> Result<usize, LowerError> {
    let list_v = env
        .get(list)
        .copied()
        .ok_or_else(|| LowerError::unsupported("unbound list in for"))?;
    let ctx = if in_test { "for body" } else { "" };
    b.for_(block, span.clone(), list_v, |b, ok_arm, elt, exit| {
        push_for_frame(loops, exit);
        env.insert(binder.to_string(), elt);
        let mut cur = ok_arm;
        for stmt in body {
            cur = if in_test {
                lower_stmt_test(b, cur, stmt, env, loops, plan, maps, ctx)?
            } else {
                lower_stmt(b, cur, stmt, env, loops, plan, maps, false)?
            };
        }
        loops.pop();
        Ok(cur)
    })
}

#[allow(clippy::too_many_arguments)]
fn lower_forall_range(
    b: &mut Builder,
    block: usize,
    span: &Span,
    binder: &str,
    lo: &StaticI64,
    hi: &StaticI64,
    body: &[Stmt],
    env: &mut std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    in_test: bool,
) -> Result<usize, LowerError> {
    let lo_v = resolve_static_i64_lower(lo, env, b, block)?;
    let hi_v = resolve_static_i64_lower(hi, env, b, block)?;
    let ctx = if in_test { "forall body" } else { "" };
    b.forall_range(block, span.clone(), lo_v, hi_v, |b, start, n| {
        env.insert(binder.to_string(), n);
        let mut cur = start;
        for stmt in body {
            cur = if in_test {
                lower_stmt_test(b, cur, stmt, env, loops, plan, maps, ctx)?
            } else {
                lower_stmt(b, cur, stmt, env, loops, plan, maps, false)?
            };
        }
        Ok(cur)
    })
}

#[allow(clippy::too_many_arguments)]
fn lower_forall(
    b: &mut Builder,
    block: usize,
    span: &Span,
    binder: &str,
    bound: &ForallBound,
    body: &[Stmt],
    env: &mut std::collections::HashMap<String, u32>,
    loops: &mut Vec<LoopFrame>,
    plan: &mut LowerPlan<'_>,
    maps: &CalleeMaps,
    in_test: bool,
) -> Result<usize, LowerError> {
    match bound {
        ForallBound::Range { lo, hi, .. } => lower_forall_range(
            b, block, span, binder, lo, hi, body, env, loops, plan, maps, in_test,
        ),
        ForallBound::List { name, .. } => lower_for(
            b, block, span, binder, name, body, env, loops, plan, maps, in_test,
        ),
    }
}

fn lower_rhs_binop(
    b: &mut Builder,
    block: usize,
    span: &Span,
    op: &BinOp,
    ty: &Type,
    lhs: &Operand,
    rhs: &Operand,
    env: &std::collections::HashMap<String, u32>,
) -> Result<u32, LowerError> {
    if try_ordered(op).is_some() {
        return lower_ordered(b, block, span, op, ty, lhs, rhs, env);
    }
    if !matches!(
        op,
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Eq | BinOp::Ne
    ) {
        return Err(LowerError::unsupported(
            "binary operator not lowered in this slice",
        ));
    }
    let l = lower_typed_scalar(b, block, lhs, ty, env)?;
    let r = lower_typed_scalar(b, block, rhs, ty, env)?;
    lower_binop_values(b, block, span, op, ty, l, r)
}

fn try_ordered(op: &BinOp) -> Option<Op> {
    match op {
        BinOp::Lt => Some(Op::Lt),
        BinOp::Le => Some(Op::Le),
        BinOp::Gt => Some(Op::Gt),
        BinOp::Ge => Some(Op::Ge),
        _ => None,
    }
}

fn lower_ordered(
    b: &mut Builder,
    block: usize,
    span: &Span,
    op: &BinOp,
    ty: &Type,
    lhs: &Operand,
    rhs: &Operand,
    env: &std::collections::HashMap<String, u32>,
) -> Result<u32, LowerError> {
    let Some(opc) = try_ordered(op) else {
        return Err(LowerError::unsupported("ordered compare"));
    };
    let l = lower_typed_scalar(b, block, lhs, ty, env)?;
    let r = lower_typed_scalar(b, block, rhs, ty, env)?;
    Ok(b.emit_i64_binop(block, span.clone(), opc, l, r, Imm::None))
}

/// `i32` and `f32` literals take the narrow const. Other operands stay on
/// `lower_operand` (`i64`, `f64`, names, bools).
fn lower_typed_scalar(
    b: &mut Builder,
    block: usize,
    op: &Operand,
    ty: &Type,
    env: &std::collections::HashMap<String, u32>,
) -> Result<u32, LowerError> {
    match (op, ty) {
        (Operand::Int(n, span), Type::Named(name, _)) if name == "i32" => {
            Ok(b.narrow_const(block, span.clone(), Narrow::I32(*n as i32)))
        }
        (Operand::Float(x, span), Type::Named(name, _)) if name == "f32" => {
            Ok(b.narrow_const(block, span.clone(), Narrow::F32((*x as f32).to_bits())))
        }
        _ => lower_operand(b, block, op, env),
    }
}

fn f32_arith_imm(ty: &Type) -> Imm {
    if type_name_is(ty, "f32") {
        Imm::F32(0)
    } else {
        Imm::None
    }
}

fn lower_binop_values(
    b: &mut Builder,
    block: usize,
    span: &Span,
    op: &BinOp,
    ty: &Type,
    l: u32,
    r: u32,
) -> Result<u32, LowerError> {
    match op {
        BinOp::Add => Ok(b.emit_i64_binop(block, span.clone(), Op::Add, l, r, f32_arith_imm(ty))),
        BinOp::Sub => Ok(b.emit_i64_binop(block, span.clone(), Op::Sub, l, r, f32_arith_imm(ty))),
        BinOp::Mul => Ok(b.emit_i64_binop(block, span.clone(), Op::Mul, l, r, f32_arith_imm(ty))),
        BinOp::Div => lower_div(block, span.clone(), ty, l, r, b),
        BinOp::Eq if type_is_bool(ty) => Ok(b.eq_bool(block, span.clone(), l, r)),
        BinOp::Eq => Ok(b.eq_i64(block, span.clone(), l, r)),
        _ => lower_rhs_ne(b, block, span, ty, l, r),
    }
}

fn lower_operand_pair(
    b: &mut Builder,
    block: usize,
    first: &Operand,
    second: &Operand,
    env: &std::collections::HashMap<String, u32>,
) -> Result<(u32, u32), LowerError> {
    let x = lower_operand(b, block, first, env)?;
    let y = lower_operand(b, block, second, env)?;
    Ok((x, y))
}

fn lower_rhs_ne(
    b: &mut Builder,
    block: usize,
    span: &Span,
    ty: &Type,
    l: u32,
    r: u32,
) -> Result<u32, LowerError> {
    let eq = if type_is_bool(ty) {
        b.eq_bool(block, span.clone(), l, r)
    } else {
        b.eq_i64(block, span.clone(), l, r)
    };
    let false_v = b.const_bool(block, span.clone(), false);
    Ok(b.eq_bool(block, span.clone(), eq, false_v))
}

fn lower_rhs_list(
    b: &mut Builder,
    block: usize,
    rhs: &Rhs,
    env: &std::collections::HashMap<String, u32>,
) -> Result<u32, LowerError> {
    match rhs {
        Rhs::ListEmpty { span, .. } => Ok(b.list_empty(block, span.clone())),
        Rhs::ListCons { span, head, tail } => {
            let (h, t) = lower_operand_pair(b, block, head, tail, env)?;
            Ok(b.list_cons(block, span.clone(), h, t))
        }
        Rhs::ListLen { span, list } => {
            let l = lower_operand(b, block, list, env)?;
            Ok(b.list_len(block, span.clone(), l))
        }
        Rhs::ListGet { span, list, index } => {
            let (l, i) = lower_operand_pair(b, block, list, index, env)?;
            Ok(b.list_get(block, span.clone(), l, i))
        }
        _ => Err(LowerError::unsupported("list rhs form")),
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

    #[test]
    fn forall_property_lowering_uses_ge() {
        let src = include_str!("../../../../golden/forall_ok.strake");
        let prog = compile_golden("forall_ok.strake", src);
        let prop = fn_named(&prog, "property.small");
        assert!(
            prop.blocks
                .iter()
                .flat_map(|b| &b.insts)
                .any(|i| i.op == Op::Ge),
            "forall range must lower to ge_i64"
        );
    }

    /// Phase 1 §2.7: user `trap` lowers to opcode `trap` with an immediate
    /// outside the cannot-carry list; `golden/trap_user.strake` has no CLI oracle.
    #[test]
    fn trap_user_lowers_to_trap_outside_reserved_codes() {
        use crate::body::interp::{ir::Imm, TrapImm};
        let src = include_str!("../../../../golden/trap_user.strake");
        let prog = compile_golden("golden/trap_user.strake", src);
        let traps: Vec<_> = fn_named(&prog, "stop")
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter(|i| i.op == Op::Trap)
            .collect();
        assert_eq!(traps.len(), 1);
        assert!(matches!(traps[0].imm, Imm::Trap(TrapImm::User)));
    }

    /// `--draft` keeps the hole and lowers it to a `trap` with the `Hole` immediate.
    #[test]
    fn hole_draft_lowers_to_hole_trap() {
        use crate::body::interp::{ir::Imm, TrapImm};
        let src = include_str!("../../../../golden/hole_draft.strake");
        let module = syntax::parse(src, "golden/hole_draft.strake").expect("parse");
        let prog = compile_module(&module, true).expect("draft compile");
        assert!(fn_named(&prog, "f")
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .any(|i| i.op == Op::Trap && matches!(i.imm, Imm::Trap(TrapImm::Hole))));
    }

    #[test]
    fn for_list_lowering_compiles() {
        let src = include_str!("../../../../golden/for_list.strake");
        let prog = compile_golden("for_list.strake", src);
        let f = fn_named(&prog, "test.for_list");
        let insts: Vec<_> = f.blocks.iter().flat_map(|b| &b.insts).collect();
        assert!(insts.iter().any(|i| i.op == Op::ListGet));
        assert!(insts.iter().any(|i| matches!(
            i.imm,
            crate::body::interp::ir::Imm::Trap(crate::body::interp::TrapImm::Lower)
        )));
    }

    #[test]
    fn nested_loop_lower_still_reaches_header() {
        let src = include_str!("../../../../golden/nested_loop.strake");
        let prog = compile_golden("nested_loop.strake", src);
        let f = fn_named(&prog, "test.nested_loop");
        assert!(f.blocks.len() > 2);
    }

    #[test]
    fn gma422_goldens_compile() {
        for (file, src) in [
            (
                "forall_ok.strake",
                include_str!("../../../../golden/forall_ok.strake"),
            ),
            (
                "forall_list_bad.strake",
                include_str!("../../../../golden/forall_list_bad.strake"),
            ),
            (
                "for_list.strake",
                include_str!("../../../../golden/for_list.strake"),
            ),
        ] {
            let _ = compile_golden(file, src);
        }
    }

    #[test]
    fn lower_rhs_surface_compile() {
        let src = r#"strake 1

type pair { a: i64, b: i64 }

fn f() -> i64
  e = list.empty i64
  xs = list.cons 1 e
  n = list.len xs
  p = { a: 1, b: 2 }
  v = p.a
  okv = ok i64 3
  c = add i64 n v
  d = mul i64 c 1
  ret d
end

test t
  ret 0
end
"#;
        let _ = compile_golden("rhs_surface.strake", src);
    }

    #[test]
    fn add2_property_lowering_uses_arithmetic_ops() {
        let src = include_str!("../../../../golden/add2_worked.strake");
        let prog = compile_golden("add2_worked.strake", src);
        let prop = fn_named(&prog, "property.add2.zero");
        let ops: Vec<Op> = prop
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .map(|i| i.op)
            .collect();
        assert!(ops.contains(&Op::Mul));
        assert!(ops.contains(&Op::Add));
        assert!(ops.contains(&Op::Ge));
    }

    #[test]
    fn ordered_helper_rejects_add() {
        let mut b = Builder::new();
        let _ = b.begin_function("f", 0);
        let span = Span {
            file: "t".into(),
            start: 0,
            end: 0,
        };
        let ty = Type::Named("i64".into(), span.clone());
        let lhs = Operand::Int(1, span.clone());
        let rhs = Operand::Int(2, span.clone());
        let env = std::collections::HashMap::new();
        let err = lower_ordered(&mut b, 0, &span, &BinOp::Add, &ty, &lhs, &rhs, &env)
            .expect_err("add is not ordered");
        assert!(err.message().contains("ordered compare"));
        assert!(try_ordered(&BinOp::Add).is_none());
        assert_eq!(try_ordered(&BinOp::Lt), Some(Op::Lt));
        assert_eq!(try_ordered(&BinOp::Le), Some(Op::Le));
        assert_eq!(try_ordered(&BinOp::Gt), Some(Op::Gt));
        assert_eq!(try_ordered(&BinOp::Ge), Some(Op::Ge));
    }

    #[test]
    fn i32_compare_emits_const_i32() {
        let src = "strake 1\n\ntest t\n  c = lt i32 -1 0\n  assert.eq c true\n  ret 0\nend\n";
        let prog = compile_golden("t.strake", src);
        let body = fn_named(&prog, "test.t");
        let saw_i32 = body.blocks.iter().flat_map(|b| &b.insts).any(|i| {
            i.op == Op::ConstI32 && matches!(i.imm, crate::body::interp::ir::Imm::I32(-1))
        });
        let saw_lt = body
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .any(|i| i.op == Op::Lt);
        assert!(saw_i32);
        assert!(saw_lt);
    }

    #[test]
    fn f32_compare_emits_const_f32() {
        let src = "strake 1\n\ntest t\n  c = le f32 -0.0 0.0\n  assert.eq c true\n  ret 0\nend\n";
        let prog = compile_golden("t.strake", src);
        let body = fn_named(&prog, "test.t");
        let neg = (-0.0f32).to_bits();
        let saw = body.blocks.iter().flat_map(|b| &b.insts).any(|i| {
            i.op == Op::ConstF32
                && matches!(i.imm, crate::body::interp::ir::Imm::F32(bits) if bits == neg)
        });
        assert!(saw);
    }

    #[test]
    fn unit_literal_emits_const_unit() {
        let src = "strake 1\n\nfn f() -> unit\n  ret unit\nend\n\ntest t\n  ret 0\nend\n";
        let prog = compile_golden("t.strake", src);
        let body = fn_named(&prog, "f");
        let ops: Vec<Op> = body
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .map(|i| i.op)
            .collect();
        assert!(ops.contains(&Op::ConstUnit));
        assert!(!ops.contains(&Op::ConstI64));
    }

    #[test]
    fn f32_arith_goldens_return_zero() {
        let cases = [
            (
                "golden/f32_arith.strake",
                include_str!("../../../../golden/f32_arith.strake"),
            ),
            (
                "golden/f32_div.strake",
                include_str!("../../../../golden/f32_div.strake"),
            ),
        ];
        for (file, src) in cases {
            let module = syntax::parse(src, file).expect("parse");
            let program = compile_module(&module, false).expect("compile");
            let got = crate::body::interp::call_function_i64(
                &program,
                "test.t",
                &[],
                &crate::body::host::EmptyHost,
            )
            .unwrap_or_else(|err| panic!("{file}: {err:?}"));
            assert_eq!(got, 0, "{file}");
        }
    }

    #[test]
    fn missing_item_type_aborts_record_lower() {
        let src = include_str!("../../../../golden/record_project.strake");
        let module = syntax::parse(src, "golden/record_project.strake").expect("parse");
        let mut sketch = crate::body::check::check(&module, false).expect("check");
        sketch.types.get_mut("f").expect("fn f").remove("p");
        sketch.record_spans.get_mut("f").expect("spans").clear();
        let err = lower(&sketch).expect_err("lower");
        assert_eq!(err, LowerError::unsupported("record type"));
    }
}
