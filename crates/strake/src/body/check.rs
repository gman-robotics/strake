//! Type and binding checks; produces a checked sketch for lowering.

#![allow(clippy::too_many_arguments)]

use std::collections::{HashMap, HashSet};

use super::carry::{
    self, operands_for_continue, publishes_loop_join, ArmSrc, BackEdge, BreakEdge, CarryPlan,
    JoinSite, LoopSite, SiteId,
};
use super::diag::{Code, Diag, Kind, Span};
use super::syntax::{
    BinOp, ContinueEdge, FnItem, ForallBound, Item, Module, Operand, PropertyItem, Rhs, Stmt,
    TestItem, Type,
};

/// Checked module ready for lowering.
#[derive(Clone, Debug, PartialEq)]
pub struct Sketch {
    pub module: Module,
    pub carry: CarryPlan,
}

struct CheckState {
    carry: CarryPlan,
    loop_stack: Vec<usize>,
    next_site_id: u32,
}

impl CheckState {
    fn new() -> Self {
        Self {
            carry: CarryPlan::default(),
            loop_stack: Vec::new(),
            next_site_id: 0,
        }
    }

    fn alloc_site_id(&mut self) -> SiteId {
        let id = SiteId(self.next_site_id);
        self.next_site_id += 1;
        id
    }
}

pub(crate) fn check(module: &Module, draft: bool) -> Result<Sketch, Vec<Diag>> {
    let mut diags = Vec::new();
    let mut state = CheckState::new();
    let fn_table: Vec<&FnItem> = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(f) => Some(f),
            _ => None,
        })
        .collect();
    for item in &module.items {
        match item {
            Item::Fn(f) => check_fn(f, &fn_table, &mut state, &mut diags),
            Item::Test(t) => check_test(t, &fn_table, &mut state, &mut diags),
            Item::Property(p) => check_property(p, &fn_table, &mut state, &mut diags),
            _ => {}
        }
    }
    if !draft {
        collect_holes_module(module, &mut diags);
    }
    if diags.is_empty() {
        Ok(Sketch {
            module: module.clone(),
            carry: state.carry,
        })
    } else {
        Err(diags)
    }
}

/// Whether a test's direct `call` sites are only to `only_fn` (§2.15).
pub(crate) fn test_matches_only_fn(test: &TestItem, only_fn: &str, module: &Module) -> bool {
    let direct = direct_calls_in_block(&test.body);
    if !direct.iter().all(|c| c == only_fn) {
        return false;
    }
    if direct.is_empty() {
        return true;
    }
    !fn_reaches_other_export_or_priv(only_fn, module)
}

fn direct_calls_in_block(stmts: &[Stmt]) -> Vec<String> {
    let mut out = Vec::new();
    for stmt in stmts {
        collect_direct_calls_stmt(stmt, &mut out);
    }
    out
}

fn collect_direct_calls_stmt(stmt: &Stmt, out: &mut Vec<String>) {
    match stmt {
        Stmt::Assign {
            rhs: Rhs::Call { callee, .. },
            ..
        } => out.push(callee.clone()),
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                collect_direct_calls_stmt(s, out);
            }
            for s in else_body {
                collect_direct_calls_stmt(s, out);
            }
        }
        Stmt::Loop { body, .. } | Stmt::For { body, .. } | Stmt::Forall { body, .. } => {
            for s in body {
                collect_direct_calls_stmt(s, out);
            }
        }
        Stmt::Match { arms, .. } => {
            for arm in arms {
                for s in &arm.body {
                    collect_direct_calls_stmt(s, out);
                }
            }
        }
        _ => {}
    }
}

fn fn_reaches_other_export_or_priv(fn_name: &str, module: &Module) -> bool {
    let mut by_name: HashMap<String, &FnItem> = HashMap::new();
    let mut priv_names = HashSet::new();
    let mut export_names = HashSet::new();
    for item in &module.items {
        if let Item::Fn(f) = item {
            by_name.insert(f.name.clone(), f);
            if f.private {
                priv_names.insert(f.name.clone());
            } else {
                export_names.insert(f.name.clone());
            }
        }
    }
    let Some(root) = by_name.get(fn_name) else {
        return false;
    };
    let mut stack: Vec<String> = direct_calls_in_block(&root.body);
    let mut visited = HashSet::new();
    while let Some(c) = stack.pop() {
        if !visited.insert(c.clone()) {
            continue;
        }
        if c != fn_name && (priv_names.contains(&c) || export_names.contains(&c)) {
            return true;
        }
        if let Some(f) = by_name.get(&c) {
            stack.extend(direct_calls_in_block(&f.body));
        }
    }
    false
}

fn collect_holes_module(module: &Module, diags: &mut Vec<Diag>) {
    for item in &module.items {
        match item {
            Item::Fn(f) => {
                for stmt in &f.body {
                    walk_holes_stmt(stmt, diags);
                }
            }
            Item::Test(t) => {
                for stmt in &t.body {
                    walk_holes_stmt(stmt, diags);
                }
            }
            Item::Property(p) => {
                for stmt in &p.body {
                    walk_holes_stmt(stmt, diags);
                }
            }
            _ => {}
        }
    }
}

fn walk_holes_stmt(stmt: &Stmt, diags: &mut Vec<Diag>) {
    match stmt {
        Stmt::Assign { rhs, .. } => walk_holes_rhs(rhs, diags),
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                walk_holes_stmt(s, diags);
            }
            for s in else_body {
                walk_holes_stmt(s, diags);
            }
        }
        Stmt::Loop { body, .. } | Stmt::For { body, .. } | Stmt::Forall { body, .. } => {
            for s in body {
                walk_holes_stmt(s, diags);
            }
        }
        Stmt::Match { arms, .. } => {
            for arm in arms {
                for s in &arm.body {
                    walk_holes_stmt(s, diags);
                }
            }
        }
        _ => {}
    }
}

fn walk_holes_rhs(rhs: &Rhs, diags: &mut Vec<Diag>) {
    if let Rhs::Hole { span, .. } = rhs {
        diags.push(hole_diag(span.clone()));
    }
}

struct Ctx {
    types: HashMap<String, Type>,
    phi_gaps: HashSet<String>,
}

impl Ctx {
    fn new() -> Self {
        Self {
            types: HashMap::new(),
            phi_gaps: HashSet::new(),
        }
    }

    fn bind(&mut self, name: &str, ty: Type, span: Span, diags: &mut Vec<Diag>) {
        if self.types.contains_key(name) {
            diags.push(shadow_diag(span));
            return;
        }
        self.phi_gaps.remove(name);
        self.types.insert(name.to_string(), ty);
    }
}

fn check_fn(f: &FnItem, fns: &[&FnItem], state: &mut CheckState, diags: &mut Vec<Diag>) {
    check_no_stmt_after_terminator(&f.body, diags);
    let mut ctx = Ctx::new();
    for p in &f.params {
        ctx.bind(&p.name, p.ty.clone(), p.span.clone(), diags);
    }
    for stmt in &f.body {
        check_stmt(stmt, &mut ctx, state, fns, true, Some(&f.ret), diags);
    }
}

fn check_test(t: &TestItem, fns: &[&FnItem], state: &mut CheckState, diags: &mut Vec<Diag>) {
    check_no_stmt_after_terminator(&t.body, diags);
    let mut ctx = Ctx::new();
    for stmt in &t.body {
        check_stmt(stmt, &mut ctx, state, fns, false, None, diags);
    }
}

fn check_property(
    p: &PropertyItem,
    fns: &[&FnItem],
    state: &mut CheckState,
    diags: &mut Vec<Diag>,
) {
    check_no_stmt_after_terminator(&p.body, diags);
    let mut ctx = Ctx::new();
    for stmt in &p.body {
        check_stmt_property(stmt, &mut ctx, state, fns, diags);
    }
}

fn stmt_check_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::Assign { span, .. }
        | Stmt::Ret { span, .. }
        | Stmt::If { span, .. }
        | Stmt::Loop { span, .. }
        | Stmt::Break { span, .. }
        | Stmt::Continue { span, .. }
        | Stmt::For { span, .. }
        | Stmt::Forall { span, .. }
        | Stmt::Trap { span }
        | Stmt::AssertEq { span, .. }
        | Stmt::Match { span, .. } => span.clone(),
    }
}

fn stmts_may_fall_through(stmts: &[Stmt]) -> bool {
    let mut sealed = false;
    for stmt in stmts {
        if sealed {
            return false;
        }
        sealed = stmt_seals_block(stmt);
    }
    !sealed
}

fn stmt_seals_block(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Ret { .. } => true,
        Stmt::If {
            then_body,
            else_body,
            ..
        } => !stmts_may_fall_through(then_body) && !stmts_may_fall_through(else_body),
        Stmt::Match { arms, .. } => arms.iter().all(|a| !stmts_may_fall_through(&a.body)),
        _ => false,
    }
}

fn check_no_stmt_after_terminator(stmts: &[Stmt], diags: &mut Vec<Diag>) {
    let mut sealed = false;
    for stmt in stmts {
        if sealed {
            diags.push(seq_diag(stmt_check_span(stmt)));
            return;
        }
        match stmt {
            Stmt::Ret { .. } | Stmt::Break { .. } | Stmt::Continue { .. } => sealed = true,
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                check_no_stmt_after_terminator(then_body, diags);
                check_no_stmt_after_terminator(else_body, diags);
                if !stmts_may_fall_through(then_body) && !stmts_may_fall_through(else_body) {
                    sealed = true;
                }
            }
            Stmt::Loop { body, .. } => {
                check_no_stmt_after_terminator(body, diags);
            }
            Stmt::For { body, .. } | Stmt::Forall { body, .. } => {
                check_no_stmt_after_terminator(body, diags);
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    check_no_stmt_after_terminator(&arm.body, diags);
                }
            }
            _ => {}
        }
    }
}

fn check_stmt_property(
    stmt: &Stmt,
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) {
    match stmt {
        Stmt::Forall {
            span,
            binder,
            bound,
            body,
        } => {
            let ty = forall_binder_ty(bound);
            ctx.bind(binder, ty, span.clone(), diags);
            for s in body {
                check_stmt_forall(s, &mut *ctx, state, binder, fns, false, None, diags);
            }
        }
        _ => check_stmt(stmt, ctx, state, fns, false, None, diags),
    }
}

fn check_stmt(
    stmt: &Stmt,
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    diags: &mut Vec<Diag>,
) {
    match stmt {
        Stmt::Assign { span, name, rhs } => {
            let ty = check_rhs(rhs, &ctx.types, fns, diags);
            ctx.bind(name, ty, span.clone(), diags);
        }
        Stmt::Ret { value, span } => {
            let got = operand_ty(value, &ctx.types, &ctx.phi_gaps, fns, None, diags);
            if let Some(expected) = fn_ret {
                if !types_compatible(&got, expected) {
                    diags.push(type_diag(span.clone()));
                }
            }
        }
        Stmt::If {
            span,
            cond,
            then_body,
            else_body,
            ..
        } => {
            operand_ty(cond, &ctx.types, &ctx.phi_gaps, fns, None, diags);
            check_if(
                then_body,
                else_body,
                span.clone(),
                ctx,
                state,
                fns,
                in_fn,
                fn_ret,
                None,
                diags,
            );
        }
        Stmt::Break { span } => {
            diags.push(type_diag(span.clone()));
        }
        Stmt::Continue { span, .. } => {
            diags.push(type_diag(span.clone()));
        }
        Stmt::For {
            span,
            binder,
            list,
            body,
        } => {
            if !ctx.types.contains_key(list) {
                diags.push(type_diag(span.clone()));
            }
            ctx.bind(binder, scalar("i64", span.clone()), span.clone(), diags);
            for s in body {
                check_stmt(s, ctx, state, fns, in_fn, fn_ret, diags);
            }
        }
        Stmt::Loop {
            span, params, body, ..
        } => {
            let site_idx = state.carry.loops.len();
            let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            let site_id = state.alloc_site_id();
            state.carry.loops.push(LoopSite {
                id: site_id,
                span: span.clone(),
                params: param_names.clone(),
                backedges: Vec::new(),
                breaks: Vec::new(),
            });
            state.loop_stack.push(site_idx);
            let mut loop_ctx = Ctx {
                types: ctx.types.clone(),
                phi_gaps: ctx.phi_gaps.clone(),
            };
            let param_set: HashSet<String> = param_names.iter().cloned().collect();
            for p in params {
                let ty = operand_ty(
                    &p.init,
                    &loop_ctx.types,
                    &loop_ctx.phi_gaps,
                    fns,
                    None,
                    diags,
                );
                loop_ctx.bind(&p.name, ty, p.span.clone(), diags);
            }
            for s in body {
                check_stmt_loop(
                    s,
                    &mut loop_ctx,
                    state,
                    &param_set,
                    fns,
                    in_fn,
                    fn_ret,
                    diags,
                );
            }
            if stmts_may_fall_through(body) {
                let operands: Vec<Operand> = param_names
                    .iter()
                    .map(|n| Operand::Name(n.clone(), span.clone()))
                    .collect();
                state.carry.loops[site_idx].backedges.push(BackEdge {
                    span: span.clone(),
                    operands,
                });
            }
            state.loop_stack.pop();
            let site = &state.carry.loops[site_idx];
            if site.breaks.is_empty() {
                // no exit values published after the loop
            } else {
                for p in params {
                    if let Some(ty) = loop_ctx.types.get(&p.name) {
                        ctx.phi_gaps.remove(&p.name);
                        ctx.types.insert(p.name.clone(), ty.clone());
                    }
                }
            }
        }
        Stmt::Forall {
            span,
            binder,
            bound,
            body,
        } => {
            let ty = forall_binder_ty(bound);
            ctx.bind(binder, ty, span.clone(), diags);
            let mut inner = Ctx {
                types: ctx.types.clone(),
                phi_gaps: ctx.phi_gaps.clone(),
            };
            let forall_name = binder.clone();
            for s in body {
                check_stmt_forall(
                    s,
                    &mut inner,
                    state,
                    &forall_name,
                    fns,
                    in_fn,
                    fn_ret,
                    diags,
                );
            }
        }
        Stmt::AssertEq { span, got, want } => {
            if in_fn {
                diags.push(parse_diag(span.clone()));
                return;
            }
            let g = operand_ty(got, &ctx.types, &ctx.phi_gaps, fns, None, diags);
            let w = operand_ty(want, &ctx.types, &ctx.phi_gaps, fns, Some(&g), diags);
            if !types_compatible(&g, &w) {
                diags.push(type_diag(span.clone()));
            }
        }
        _ => {}
    }
}

fn arm_supplies_loop_exit(arm: &[Stmt], other: &[Stmt], name: &str) -> bool {
    let supplies = carry::join_name_supplies(arm, other);
    supplies
        .iter()
        .find(|s| s.name == name)
        .is_some_and(publishes_loop_join)
}

fn check_continue_edges(
    span: Span,
    edges: &[ContinueEdge],
    loop_params: &HashSet<String>,
    ctx: &mut Ctx,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> bool {
    let mut seen = HashSet::new();
    for e in edges {
        if !loop_params.contains(&e.name) {
            diags.push(type_diag(span.clone()));
            return false;
        }
        if !seen.insert(e.name.clone()) {
            diags.push(type_diag(span.clone()));
            return false;
        }
        let val_ty = operand_ty(&e.value, &ctx.types, &ctx.phi_gaps, fns, None, diags);
        if let Some(param_ty) = ctx.types.get(&e.name) {
            if !types_compatible(&val_ty, param_ty) {
                diags.push(type_diag(span.clone()));
                return false;
            }
        }
    }
    for name in loop_params {
        if !seen.contains(name) {
            diags.push(type_diag(span.clone()));
            return false;
        }
    }
    true
}

fn check_stmt_loop(
    stmt: &Stmt,
    ctx: &mut Ctx,
    state: &mut CheckState,
    loop_params: &HashSet<String>,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    diags: &mut Vec<Diag>,
) {
    if let Stmt::Assign { span, name, .. } = stmt {
        if loop_params.contains(name) {
            diags.push(shadow_diag(span.clone()));
            return;
        }
    }
    match stmt {
        Stmt::Continue { edges, span, .. } => {
            if check_continue_edges(span.clone(), edges, loop_params, ctx, fns, diags) {
                if let Some(&idx) = state.loop_stack.last() {
                    if let Some(ops) = operands_for_continue(&state.carry.loops[idx].params, edges)
                    {
                        state.carry.loops[idx].backedges.push(BackEdge {
                            span: span.clone(),
                            operands: ops,
                        });
                    }
                }
            }
        }
        Stmt::Break { span } => {
            if let Some(&idx) = state.loop_stack.last() {
                state.carry.loops[idx]
                    .breaks
                    .push(BreakEdge { span: span.clone() });
            }
        }
        Stmt::If {
            span,
            cond,
            then_body,
            else_body,
            ..
        } => {
            operand_ty(cond, &ctx.types, &ctx.phi_gaps, fns, None, diags);
            check_if(
                then_body,
                else_body,
                span.clone(),
                ctx,
                state,
                fns,
                in_fn,
                fn_ret,
                Some(loop_params),
                diags,
            );
        }
        Stmt::Loop {
            span, params, body, ..
        } => check_nested_loop_in_loop_body(
            span, params, body, ctx, state, fns, in_fn, fn_ret, diags,
        ),
        _ => check_stmt(stmt, ctx, state, fns, in_fn, fn_ret, diags),
    }
}

fn check_nested_loop_in_loop_body(
    span: &Span,
    params: &[super::syntax::LoopParam],
    body: &[Stmt],
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    diags: &mut Vec<Diag>,
) {
    let site_idx = state.carry.loops.len();
    let param_names_vec: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    let site_id = state.alloc_site_id();
    state.carry.loops.push(LoopSite {
        id: site_id,
        span: span.clone(),
        params: param_names_vec.clone(),
        backedges: Vec::new(),
        breaks: Vec::new(),
    });
    state.loop_stack.push(site_idx);
    let mut loop_ctx = Ctx {
        types: ctx.types.clone(),
        phi_gaps: ctx.phi_gaps.clone(),
    };
    let inner_params: HashSet<String> = param_names_vec.iter().cloned().collect();
    for p in params {
        let ty = operand_ty(
            &p.init,
            &loop_ctx.types,
            &loop_ctx.phi_gaps,
            fns,
            None,
            diags,
        );
        loop_ctx.bind(&p.name, ty, p.span.clone(), diags);
    }
    for s in body {
        check_stmt_loop(
            s,
            &mut loop_ctx,
            state,
            &inner_params,
            fns,
            in_fn,
            fn_ret,
            diags,
        );
    }
    if stmts_may_fall_through(body) {
        let operands: Vec<Operand> = param_names_vec
            .iter()
            .map(|n| Operand::Name(n.clone(), span.clone()))
            .collect();
        state.carry.loops[site_idx].backedges.push(BackEdge {
            span: span.clone(),
            operands,
        });
    }
    state.loop_stack.pop();
    for p in params {
        if let Some(ty) = loop_ctx.types.get(&p.name) {
            loop_ctx.phi_gaps.remove(&p.name);
            ctx.phi_gaps.remove(&p.name);
            ctx.types.insert(p.name.clone(), ty.clone());
        }
    }
}

fn check_stmt_forall(
    stmt: &Stmt,
    ctx: &mut Ctx,
    state: &mut CheckState,
    binder: &str,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    diags: &mut Vec<Diag>,
) {
    if let Stmt::Assign { span, name, .. } = stmt {
        if name == binder {
            diags.push(shadow_diag(span.clone()));
        }
    }
    check_stmt(stmt, ctx, state, fns, in_fn, fn_ret, diags);
}

#[allow(clippy::too_many_arguments)]
fn check_if(
    then_body: &[Stmt],
    else_body: &[Stmt],
    span: Span,
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    loop_params: Option<&HashSet<String>>,
    diags: &mut Vec<Diag>,
) {
    let pre = ctx.types.clone();
    let then_tops = top_level_assigns(then_body);
    let else_tops = top_level_assigns(else_body);

    let join_site_idx = state.carry.joins.len();
    let join_id = state.alloc_site_id();
    state.carry.joins.push(JoinSite {
        id: join_id,
        span: span.clone(),
        names: carry::join_name_supplies(then_body, else_body),
    });

    let mut then_ctx = Ctx {
        types: pre.clone(),
        phi_gaps: ctx.phi_gaps.clone(),
    };
    for s in then_body {
        check_stmt_in_arm(
            s,
            &mut then_ctx,
            &pre,
            state,
            fns,
            in_fn,
            fn_ret,
            loop_params,
            diags,
        );
    }
    let mut else_ctx = Ctx {
        types: pre.clone(),
        phi_gaps: ctx.phi_gaps.clone(),
    };
    for s in else_body {
        check_stmt_in_arm(
            s,
            &mut else_ctx,
            &pre,
            state,
            fns,
            in_fn,
            fn_ret,
            loop_params,
            diags,
        );
    }

    for (name, (ty, span)) in &then_tops {
        if else_tops.contains_key(name) {
            if pre.contains_key(name) {
                continue;
            }
            if ctx.types.contains_key(name) {
                diags.push(shadow_diag(span.clone()));
            } else {
                ctx.types.insert(name.clone(), ty.clone());
                ctx.phi_gaps.remove(name);
            }
        } else if arm_supplies_loop_exit(then_body, else_body, name) {
            if pre.contains_key(name) {
                continue;
            }
            if let Some(ety) = else_ctx.types.get(name) {
                if ctx.types.contains_key(name) {
                    diags.push(shadow_diag(span.clone()));
                } else {
                    ctx.types.insert(name.clone(), ety.clone());
                    ctx.phi_gaps.remove(name);
                }
            } else {
                ctx.phi_gaps.insert(name.clone());
            }
        } else {
            ctx.phi_gaps.insert(name.clone());
        }
    }
    for name in else_tops.keys() {
        if !then_tops.contains_key(name) && !pre.contains_key(name) {
            if then_ctx.types.contains_key(name) {
                ctx.types.insert(name.clone(), then_ctx.types[name].clone());
                ctx.phi_gaps.remove(name);
            } else if arm_supplies_loop_exit(else_body, then_body, name) {
                if let Some(ety) = else_ctx.types.get(name) {
                    ctx.types.insert(name.clone(), ety.clone());
                    ctx.phi_gaps.remove(name);
                } else {
                    ctx.phi_gaps.insert(name.clone());
                }
            } else {
                ctx.phi_gaps.insert(name.clone());
            }
        }
    }

    let site = &state.carry.joins[join_site_idx];
    for supply in &site.names {
        if !publishes_loop_join(supply) {
            continue;
        }
        let name = &supply.name;
        if pre.contains_key(name) {
            continue;
        }
        let t_ty = join_arm_type(
            then_body,
            &supply.then_arm,
            name,
            &then_tops,
            &then_ctx.types,
            &then_ctx.phi_gaps,
            fns,
            diags,
        );
        let e_ty = join_arm_type(
            else_body,
            &supply.else_arm,
            name,
            &else_tops,
            &else_ctx.types,
            &else_ctx.phi_gaps,
            fns,
            diags,
        );
        match (t_ty, e_ty) {
            (Some(a), Some(b)) if types_compatible(&a, &b) => {
                if let Some(existing) = ctx.types.get(name) {
                    if !types_compatible(existing, &a) {
                        diags.push(type_diag(span.clone()));
                    }
                } else {
                    ctx.types.insert(name.clone(), a);
                    ctx.phi_gaps.remove(name);
                }
            }
            (Some(_), Some(_)) => diags.push(type_diag(span.clone())),
            _ => diags.push(type_diag(span.clone())),
        }
    }
}

fn join_arm_type(
    arm: &[Stmt],
    src: &ArmSrc,
    name: &str,
    tops: &HashMap<String, (Type, Span)>,
    env: &HashMap<String, Type>,
    phi_gaps: &HashSet<String>,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Option<Type> {
    match src {
        ArmSrc::LoopExit => loop_param_init_ty(arm, name, env, phi_gaps, fns, diags),
        ArmSrc::Assign => tops.get(name).map(|(ty, _)| ty.clone()),
        ArmSrc::Absent => None,
    }
}

fn loop_param_init_ty(
    arm: &[Stmt],
    name: &str,
    env: &HashMap<String, Type>,
    phi_gaps: &HashSet<String>,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Option<Type> {
    for stmt in arm {
        if let Stmt::Loop { params, .. } = stmt {
            for p in params {
                if p.name == name {
                    return Some(operand_ty(&p.init, env, phi_gaps, fns, None, diags));
                }
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn check_stmt_in_arm(
    stmt: &Stmt,
    ctx: &mut Ctx,
    pre: &HashMap<String, Type>,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    loop_params: Option<&HashSet<String>>,
    diags: &mut Vec<Diag>,
) {
    match stmt {
        Stmt::Assign { span, name, rhs } => {
            if pre.contains_key(name) {
                diags.push(shadow_diag(span.clone()));
                return;
            }
            let ty = check_rhs(rhs, &ctx.types, fns, diags);
            if ctx.types.contains_key(name) {
                diags.push(shadow_diag(span.clone()));
            } else {
                ctx.types.insert(name.clone(), ty);
            }
        }
        Stmt::If {
            span,
            cond,
            then_body,
            else_body,
            ..
        } => {
            operand_ty(cond, &ctx.types, &ctx.phi_gaps, fns, None, diags);
            check_if(
                then_body,
                else_body,
                span.clone(),
                ctx,
                state,
                fns,
                in_fn,
                fn_ret,
                loop_params,
                diags,
            );
        }
        Stmt::Continue { edges, span, .. } => {
            if let Some(params) = loop_params {
                if check_continue_edges(span.clone(), edges, params, ctx, fns, diags) {
                    if let Some(&idx) = state.loop_stack.last() {
                        if let Some(ops) =
                            operands_for_continue(&state.carry.loops[idx].params, edges)
                        {
                            state.carry.loops[idx].backedges.push(BackEdge {
                                span: span.clone(),
                                operands: ops,
                            });
                        }
                    }
                }
            } else {
                diags.push(type_diag(span.clone()));
            }
        }
        Stmt::Break { span } => {
            if loop_params.is_some() {
                if let Some(&idx) = state.loop_stack.last() {
                    state.carry.loops[idx]
                        .breaks
                        .push(BreakEdge { span: span.clone() });
                }
            } else {
                diags.push(type_diag(span.clone()));
            }
        }
        Stmt::For {
            span,
            binder,
            list,
            body,
        } => {
            if !ctx.types.contains_key(list) {
                diags.push(type_diag(span.clone()));
            }
            if ctx.types.contains_key(binder) {
                diags.push(shadow_diag(span.clone()));
            } else {
                ctx.types
                    .insert(binder.clone(), scalar("i64", span.clone()));
            }
            for s in body {
                check_stmt_in_arm(s, ctx, pre, state, fns, in_fn, fn_ret, loop_params, diags);
            }
        }
        Stmt::Loop {
            span, params, body, ..
        } => check_nested_loop_in_loop_body(
            span, params, body, ctx, state, fns, in_fn, fn_ret, diags,
        ),
        Stmt::Ret { value, span } => {
            let got = operand_ty(value, &ctx.types, &ctx.phi_gaps, fns, None, diags);
            if let Some(expected) = fn_ret {
                if !types_compatible(&got, expected) {
                    diags.push(type_diag(span.clone()));
                }
            }
        }
        _ => {}
    }
}

fn top_level_assigns(body: &[Stmt]) -> HashMap<String, (Type, Span)> {
    let mut out = HashMap::new();
    for stmt in body {
        if let Stmt::Assign { name, rhs, span } = stmt {
            out.insert(name.clone(), (rhs_ty_hint(rhs), span.clone()));
        }
    }
    out
}

fn rhs_ty_hint(rhs: &Rhs) -> Type {
    match rhs {
        Rhs::BinOp { ty, .. } => ty.clone(),
        Rhs::Hole { ty, .. } => ty.clone(),
        _ => scalar("i64", empty_span()),
    }
}

fn forall_binder_ty(bound: &ForallBound) -> Type {
    match bound {
        ForallBound::Range { ty, .. } | ForallBound::List { ty, .. } => ty.clone(),
    }
}

fn check_rhs(
    rhs: &Rhs,
    env: &HashMap<String, Type>,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    let gaps = HashSet::new();
    match rhs {
        Rhs::BinOp {
            ty,
            lhs,
            rhs,
            span,
            op,
        } => {
            if matches!(op, BinOp::Eq | BinOp::Ne) && type_is_list(ty) {
                diags.push(type_diag(span.clone()));
                return ty.clone();
            }
            let lt = operand_ty(lhs, env, &gaps, fns, Some(ty), diags);
            let rt = operand_ty(rhs, env, &gaps, fns, Some(ty), diags);
            if !types_compatible(&lt, ty) || !types_compatible(&rt, ty) {
                diags.push(type_diag(span.clone()));
            }
            ty.clone()
        }
        Rhs::Call { span, callee, args } => {
            let sig = resolve_fn(callee, fns);
            if sig.is_none() {
                diags.push(import_diag(span.clone()));
                return scalar("i64", span.clone());
            }
            let (params, ret) = sig.unwrap();
            if params.len() != args.len() {
                diags.push(type_diag(span.clone()));
            }
            for (arg, param_ty) in args.iter().zip(params.iter()) {
                let at = operand_ty(arg, env, &gaps, fns, Some(param_ty), diags);
                if !types_compatible(&at, param_ty) {
                    diags.push(type_diag(span.clone()));
                }
            }
            ret
        }
        Rhs::Not { operand, span } => {
            let t = operand_ty(
                operand,
                env,
                &gaps,
                fns,
                Some(&scalar("bool", span.clone())),
                diags,
            );
            if !types_compatible(&t, &scalar("bool", span.clone())) {
                diags.push(type_diag(span.clone()));
            }
            scalar("bool", span.clone())
        }
        Rhs::Operand(op) => operand_ty(op, env, &gaps, fns, None, diags),
        Rhs::Hole { ty, .. } => ty.clone(),
        _ => scalar("i64", empty_span()),
    }
}

fn type_is_list(ty: &Type) -> bool {
    matches!(ty, Type::List(_, _))
}

fn resolve_fn(name: &str, fns: &[&FnItem]) -> Option<(Vec<Type>, Type)> {
    fns.iter().find(|f| f.name == name).map(|f| {
        (
            f.params.iter().map(|p| p.ty.clone()).collect(),
            f.ret.clone(),
        )
    })
}

fn operand_ty(
    op: &Operand,
    env: &HashMap<String, Type>,
    phi_gaps: &HashSet<String>,
    _fns: &[&FnItem],
    expected: Option<&Type>,
    diags: &mut Vec<Diag>,
) -> Type {
    match op {
        Operand::Name(name, span) => {
            if phi_gaps.contains(name) {
                diags.push(if_phi_diag(span.clone()));
                return scalar("i64", span.clone());
            }
            env.get(name).cloned().unwrap_or_else(|| {
                diags.push(type_diag(span.clone()));
                scalar("i64", span.clone())
            })
        }
        Operand::Int(n, span) => int_literal_ty(*n, span.clone(), expected, diags),
        Operand::Unit(span) => Type::Named("unit".into(), span.clone()),
        Operand::Bool(_, span) => Type::Named("bool".into(), span.clone()),
        Operand::Float(_, span) => Type::Named("f64".into(), span.clone()),
        Operand::Bytes(_, span) => Type::Named("bytes".into(), span.clone()),
    }
}

fn int_literal_ty(n: i128, span: Span, expected: Option<&Type>, diags: &mut Vec<Diag>) -> Type {
    if let Some(Type::Named(name, _)) = expected {
        match name.as_str() {
            "i32" if fits_i32(n) => return scalar("i32", span),
            "i64" if fits_i64(n) => return scalar("i64", span),
            "i32" | "i64" => {
                diags.push(type_diag(span.clone()));
                return scalar(name, span);
            }
            _ => {}
        }
    }
    if fits_i64(n) {
        scalar("i64", span)
    } else {
        diags.push(type_diag(span.clone()));
        scalar("i64", span)
    }
}

fn types_compatible(found: &Type, expected: &Type) -> bool {
    if types_equal(found, expected) {
        return true;
    }
    if let (Type::Named(f, _), Type::Named(e, _)) = (found, expected) {
        if f == e {
            return true;
        }
        if (f == "i32" || f == "i64") && (e == "i32" || e == "i64") {
            return true;
        }
    }
    false
}

fn types_equal(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Named(na, _), Type::Named(nb, _)) => na == nb,
        (Type::List(ia, _), Type::List(ib, _)) => types_equal(ia, ib),
        (Type::Result(oa, ea, _), Type::Result(ob, eb, _)) => {
            types_equal(oa, ob) && types_equal(ea, eb)
        }
        _ => false,
    }
}

fn scalar(name: &str, span: Span) -> Type {
    Type::Named(name.to_string(), span)
}

fn empty_span() -> Span {
    Span {
        file: String::new(),
        start: 0,
        end: 0,
    }
}

fn fits_i64(n: i128) -> bool {
    n >= i64::MIN as i128 && n <= i64::MAX as i128
}

fn fits_i32(n: i128) -> bool {
    n >= i32::MIN as i128 && n <= i32::MAX as i128
}

fn type_diag(span: Span) -> Diag {
    Diag {
        code: Code::EType,
        kind: Kind::Check,
        span,
        want: None,
        got: None,
        patch: None,
    }
}

fn shadow_diag(span: Span) -> Diag {
    Diag {
        code: Code::EShadow,
        kind: Kind::Check,
        span,
        want: None,
        got: None,
        patch: None,
    }
}

fn if_phi_diag(span: Span) -> Diag {
    Diag {
        code: Code::EIfPhi,
        kind: Kind::Check,
        span,
        want: None,
        got: None,
        patch: None,
    }
}

fn import_diag(span: Span) -> Diag {
    Diag {
        code: Code::EImport,
        kind: Kind::Check,
        span,
        want: None,
        got: None,
        patch: None,
    }
}

fn hole_diag(span: Span) -> Diag {
    Diag {
        code: Code::EHole,
        kind: Kind::Check,
        span,
        want: None,
        got: None,
        patch: None,
    }
}

fn parse_diag(span: Span) -> Diag {
    Diag {
        code: Code::EParse,
        kind: Kind::Check,
        span,
        want: None,
        got: None,
        patch: None,
    }
}

fn seq_diag(span: Span) -> Diag {
    Diag {
        code: Code::ESeq,
        kind: Kind::Check,
        span,
        want: None,
        got: None,
        patch: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::syntax;

    #[test]
    fn stmt_after_ret_is_eseq_at_check() {
        let src =
            "strake 1\n\nfn f() -> i64\n  ret 1\n  x = add i64 1 2\nend\n\ntest t\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let sketch = check(&m, false);
        assert!(sketch.is_err());
        let diags = sketch.unwrap_err();
        assert!(diags.iter().any(|d| d.code == Code::ESeq));
    }

    #[test]
    fn call_i32_param_accepts_small_literal() {
        let src = "strake 1\n\nfn id32(a: i32) -> i32\n  ret a\nend\n\ntest t\n  r = call id32(3)\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        assert!(check(&m, false).is_ok());
    }

    #[test]
    fn shadow_rebind_in_fn() {
        let src = "strake 1\n\nfn f() -> i64\n  x = add i64 1 2\n  x = add i64 x 1\n  ret x\nend\n\ntest t\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let err = check(&m, false).unwrap_err();
        assert!(err.iter().any(|d| d.code == Code::EShadow));
    }

    #[test]
    fn if_phi_missing_arm() {
        let src = "strake 1\n\nfn f(c: bool) -> i64\n  if c\n    t = add i64 1 2\n  else\n    u = add i64 3 4\n  end\n  ret t\nend\n\ntest t\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let err = check(&m, false).unwrap_err();
        assert!(err.iter().any(|d| d.code == Code::EIfPhi));
    }

    #[test]
    fn if_join_ok() {
        let src = "strake 1\n\nfn f(c: bool) -> i64\n  if c\n    t = add i64 1 2\n  else\n    t = add i64 3 4\n  end\n  ret t\nend\n\ntest t\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        assert!(check(&m, false).is_ok());
    }

    #[test]
    fn hole_ship_rejected() {
        let src =
            "strake 1\n\nfn f() -> i64\n  x = hole i64\n  ret x\nend\n\ntest t\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let err = check(&m, false).unwrap_err();
        assert!(err
            .iter()
            .any(|d| d.code == Code::EHole && d.kind == Kind::Check));
    }

    #[test]
    fn hole_draft_allowed_in_check() {
        let src =
            "strake 1\n\nfn f() -> i64\n  x = hole i64\n  ret x\nend\n\ntest t\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        assert!(check(&m, true).is_ok());
    }

    #[test]
    fn test_only_fn_direct_calls() {
        let src = "strake 1\n\nfn a() -> i64\n  ret 1\nend\n\nfn b() -> i64\n  ret 2\nend\n\ntest ok\n  x = call a()\n  ret 0\nend\n\ntest bad\n  y = call b()\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let tests: Vec<_> = m
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Test(t) => Some(t),
                _ => None,
            })
            .collect();
        assert!(test_matches_only_fn(tests[0], "a", &m));
        assert!(!test_matches_only_fn(tests[1], "a", &m));
    }

    #[test]
    fn property_forall_binder_rebind_shadow() {
        let src =
            "strake 1\n\nproperty p\n  forall n: i64 in 0..1\n    n = add i64 n 1\n  end\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let err = check(&m, false).unwrap_err();
        assert!(err.iter().any(|d| d.code == Code::EShadow));
    }

    #[test]
    fn only_fn_rejects_nested_call_in_if() {
        let src = "strake 1\n\nfn a() -> i64\n  ret 1\nend\n\nfn b() -> i64\n  ret 2\nend\n\ntest nested\n  if true\n    x = call b()\n  end\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let tests: Vec<_> = m
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Test(t) => Some(t),
                _ => None,
            })
            .collect();
        assert!(!test_matches_only_fn(tests[0], "a", &m));
    }

    #[test]
    fn only_fn_rejects_export_reaching_second_export_via_helper() {
        let src = "strake 1\n\nfn other() -> i64\n  ret 9\nend\n\nfn helper() -> i64\n  r = call other()\n  ret r\nend\n\nfn target() -> i64\n  r = call helper()\n  ret r\nend\n\ntest ok\n  x = call target()\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let tests: Vec<_> = m
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Test(t) => Some(t),
                _ => None,
            })
            .collect();
        assert!(!test_matches_only_fn(tests[0], "target", &m));
    }
}
