//! Type and binding checks; produces a checked sketch for lowering.

#![allow(clippy::too_many_arguments)]

use std::collections::{HashMap, HashSet};

use super::carry::{
    self, match_arm_delivered, match_phi_names, operands_for_continue, publishes_loop_join, ArmSrc,
    BackEdge, BreakEdge, CarryPlan, JoinSite, LoopSite, MatchArmSupply, MatchSite, SiteId,
};
use super::diag::{Code, Diag, Kind, Span};
use super::interp::ScalarTy;
use super::syntax::{
    BinOp, ContinueEdge, FnItem, ForallBound, Item, LoopParam, Module, Operand, PropertyItem, Rhs,
    StaticI64, Stmt, TestItem, Type, TypeDefKind,
};

pub(crate) const MATCH_JOIN_CODELESS: &str = "match arm missing the join value";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CheckFailure {
    Diags(Vec<Diag>),
    Codeless(&'static str),
}

struct SumTypeInfo {
    tags: Vec<String>,
    payloads: Vec<Option<Type>>,
}

/// Checked module ready for lowering.
#[derive(Clone, Debug, PartialEq)]
pub struct Sketch {
    pub module: Module,
    pub carry: CarryPlan,
}

enum Ctrl {
    Loop,
    For,
}

struct CheckState {
    carry: CarryPlan,
    loop_stack: Vec<usize>,
    /// Innermost `loop` or `for`. `break` targets the top.
    ctrl: Vec<Ctrl>,
    next_site_id: u32,
    partial_used: bool,
    sum_types: HashMap<String, SumTypeInfo>,
    record_types: HashMap<String, Vec<(String, Type)>>,
    module_int_binds: HashMap<String, i128>,
    module_list_binds: HashMap<String, Type>,
    /// `import region mem : region` is present (`load`/`store` allowed).
    has_region: bool,
}

impl CheckState {
    fn new(
        sum_types: HashMap<String, SumTypeInfo>,
        record_types: HashMap<String, Vec<(String, Type)>>,
        module_int_binds: HashMap<String, i128>,
        has_region: bool,
    ) -> Self {
        Self {
            carry: CarryPlan::default(),
            loop_stack: Vec::new(),
            ctrl: Vec::new(),
            next_site_id: 0,
            partial_used: false,
            sum_types,
            record_types,
            module_int_binds,
            module_list_binds: HashMap::new(),
            has_region,
        }
    }

    fn alloc_site_id(&mut self) -> SiteId {
        let id = SiteId(self.next_site_id);
        self.next_site_id += 1;
        id
    }
}

pub(crate) fn check(module: &Module, draft: bool) -> Result<Sketch, CheckFailure> {
    let mut diags = Vec::new();
    reject_wide_int_binds(module, &mut diags);
    let sum_types = build_sum_type_index(module);
    let record_types = build_record_type_index(module, &mut diags);
    let module_int_binds = module_int_bindings(module);
    let has_region = super::syntax::region_pages(module).is_some();
    let mut state = CheckState::new(sum_types, record_types, module_int_binds, has_region);
    seed_module_list_binds(module, &mut state, &mut diags);
    let fn_table: Vec<&FnItem> = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(f) => Some(f),
            _ => None,
        })
        .collect();
    for item in &module.items {
        check_module_item(item, &fn_table, &mut state, &mut diags);
    }
    if !draft {
        collect_holes_module(module, &mut diags);
    }
    if !diags.is_empty() {
        return Err(CheckFailure::Diags(diags));
    }
    if state.partial_used {
        return Err(CheckFailure::Codeless(MATCH_JOIN_CODELESS));
    }
    Ok(Sketch {
        module: module.clone(),
        carry: state.carry,
    })
}

fn check_module_item(
    item: &Item,
    fn_table: &[&FnItem],
    state: &mut CheckState,
    diags: &mut Vec<Diag>,
) {
    match item {
        Item::Fn(f) => check_fn(f, fn_table, state, diags),
        Item::Test(t) => check_test(t, fn_table, state, diags),
        Item::Property(p) => check_property(p, fn_table, state, diags),
        _ => {}
    }
}

fn build_sum_type_index(module: &Module) -> HashMap<String, SumTypeInfo> {
    let mut out = HashMap::new();
    for item in &module.items {
        if let Item::TypeDef(td) = item {
            if let TypeDefKind::Sum(variants) = &td.kind {
                let tags: Vec<String> = variants.iter().map(|v| v.tag.clone()).collect();
                let payloads: Vec<Option<Type>> =
                    variants.iter().map(|v| v.payload.clone()).collect();
                out.insert(td.name.clone(), SumTypeInfo { tags, payloads });
            }
        }
    }
    out
}

fn build_record_type_index(
    module: &Module,
    diags: &mut Vec<Diag>,
) -> HashMap<String, Vec<(String, Type)>> {
    let mut out = HashMap::new();
    for item in &module.items {
        if let Item::TypeDef(td) = item {
            if let TypeDefKind::Record(fields) = &td.kind {
                note_repeated_record(&out, td, diags);
                let decls = fields
                    .iter()
                    .map(|f| (f.name.clone(), f.ty.clone()))
                    .collect();
                out.insert(td.name.clone(), decls);
            }
        }
    }
    out
}

fn note_repeated_record(
    seen: &HashMap<String, Vec<(String, Type)>>,
    def: &super::syntax::TypeDef,
    diags: &mut Vec<Diag>,
) {
    if seen.contains_key(&def.name) {
        diags.push(type_diag(def.span.clone()));
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
    match_partial: HashSet<String>,
}

impl Ctx {
    fn new() -> Self {
        Self {
            types: HashMap::new(),
            phi_gaps: HashSet::new(),
            match_partial: HashSet::new(),
        }
    }

    fn bind(&mut self, name: &str, ty: Type, span: Span, diags: &mut Vec<Diag>) {
        if self.types.contains_key(name) {
            diags.push(shadow_diag(span));
            return;
        }
        self.phi_gaps.remove(name);
        self.match_partial.remove(name);
        self.types.insert(name.to_string(), ty);
    }

    fn publish_if_name(&mut self, name: &str, ty: Type) {
        self.match_partial.remove(name);
        self.phi_gaps.remove(name);
        self.types.insert(name.to_string(), ty);
    }
}

fn bind_for_body(ctx: &mut Ctx, name: &str, ty: Type, span: Span, diags: &mut Vec<Diag>) -> bool {
    let fresh = !ctx.types.contains_key(name);
    ctx.bind(name, ty, span, diags);
    fresh
}

fn release_body_bind(ctx: &mut Ctx, name: &str, fresh: bool) {
    if fresh {
        ctx.types.remove(name);
    }
}

fn note_break(state: &mut CheckState, span: &Span, diags: &mut Vec<Diag>) {
    match state.ctrl.last() {
        Some(Ctrl::For) => {}
        Some(Ctrl::Loop) => {
            if let Some(&idx) = state.loop_stack.last() {
                state.carry.loops[idx]
                    .breaks
                    .push(BreakEdge { span: span.clone() });
            }
        }
        None => diags.push(type_diag(span.clone())),
    }
}

fn note_bare_jump(state: &mut CheckState, stmt: &Stmt, diags: &mut Vec<Diag>) {
    match stmt {
        Stmt::Break { span } => note_break(state, span, diags),
        Stmt::Continue { span, .. } => diags.push(type_diag(span.clone())),
        _ => {}
    }
}

fn note_loop_continue(
    state: &mut CheckState,
    span: &Span,
    edges: &[ContinueEdge],
    loop_params: Option<&HashSet<String>>,
    ctx: &mut Ctx,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) {
    if matches!(state.ctrl.last(), Some(Ctrl::For)) {
        diags.push(type_diag(span.clone()));
        return;
    }
    let Some(params) = loop_params else {
        diags.push(type_diag(span.clone()));
        return;
    };
    if check_continue_edges(span.clone(), edges, params, ctx, state, fns, diags) {
        record_continue_edge(state, span, edges);
    }
}

fn record_continue_edge(state: &mut CheckState, span: &Span, edges: &[ContinueEdge]) {
    if let Some(&idx) = state.loop_stack.last() {
        if let Some(ops) = operands_for_continue(&state.carry.loops[idx].params, edges) {
            state.carry.loops[idx].backedges.push(BackEdge {
                span: span.clone(),
                operands: ops,
            });
        }
    }
}

fn module_int_bindings(module: &Module) -> HashMap<String, i128> {
    let mut out = HashMap::new();
    for item in &module.items {
        if let Item::Bind { name, value, .. } = item {
            out.insert(name.clone(), *value);
        }
    }
    out
}

fn reject_wide_int_binds(module: &Module, diags: &mut Vec<Diag>) {
    for item in &module.items {
        if let Item::Bind { value, span, .. } = item {
            if !fits_i64(*value) {
                diags.push(type_diag(span.clone()));
            }
        }
    }
}

fn apply_module_binds(ctx: &mut Ctx, state: &CheckState, span: Span) {
    for name in state.module_int_binds.keys() {
        let ty = scalar("i64", span.clone());
        if !ctx.types.contains_key(name) {
            ctx.types.insert(name.clone(), ty);
        }
    }
    for (name, ty) in &state.module_list_binds {
        if !ctx.types.contains_key(name) {
            ctx.types.insert(name.clone(), ty.clone());
        }
    }
}

fn seed_module_list_binds(module: &Module, state: &mut CheckState, diags: &mut Vec<Diag>) {
    let mut ctx = Ctx::new();
    for item in &module.items {
        let Item::ListBind { name, rhs, .. } = item else {
            continue;
        };
        let fns: &[&FnItem] = &[];
        let ty = check_rhs(rhs, &ctx, state, fns, diags);
        ctx.types.insert(name.clone(), ty.clone());
        state.module_list_binds.insert(name.clone(), ty);
    }
}

fn check_fn(f: &FnItem, fns: &[&FnItem], state: &mut CheckState, diags: &mut Vec<Diag>) {
    check_no_stmt_after_terminator(&f.body, diags);
    let mut ctx = Ctx::new();
    apply_module_binds(&mut ctx, state, f.span.clone());
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
    apply_module_binds(&mut ctx, state, t.span.clone());
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
    apply_module_binds(&mut ctx, state, p.span.clone());
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
        Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Ret { .. } | Stmt::Trap { .. } => true,
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
            Stmt::Ret { .. } | Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Trap { .. } => {
                sealed = true
            }
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
                if arms.iter().all(|a| !stmts_may_fall_through(&a.body)) {
                    sealed = true;
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
            check_forall_bound(bound, state, diags);
            let ty = forall_binder_ty(bound, ctx, diags);
            let fresh = bind_for_body(ctx, binder, ty, span.clone(), diags);
            for s in body {
                check_stmt_forall(s, &mut *ctx, state, binder, fns, false, None, diags);
            }
            release_body_bind(ctx, binder, fresh);
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
            check_stmt_assign(ctx, state, fns, span, name, rhs, diags);
        }
        Stmt::Ret { value, span } => check_stmt_ret(ctx, state, fns, fn_ret, value, span, diags),
        Stmt::AssertEq { span, got, want } => {
            check_stmt_assert_eq(ctx, state, fns, in_fn, span, got, want, diags);
        }
        Stmt::Trap { .. } => {}
        other => check_stmt_control(other, ctx, state, fns, in_fn, fn_ret, diags),
    }
}

fn check_stmt_control(
    stmt: &Stmt,
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    diags: &mut Vec<Diag>,
) {
    match stmt {
        Stmt::If {
            span,
            cond,
            then_body,
            else_body,
            ..
        } => check_stmt_if(
            ctx, state, fns, in_fn, fn_ret, span, cond, then_body, else_body, diags,
        ),
        Stmt::Break { .. } | Stmt::Continue { .. } => note_bare_jump(state, stmt, diags),
        Stmt::For {
            span,
            binder,
            list,
            body,
        } => check_stmt_for(
            ctx, state, fns, in_fn, fn_ret, span, binder, list, body, diags,
        ),
        Stmt::Loop {
            span, params, body, ..
        } => check_stmt_loop_block(
            ctx, state, fns, in_fn, fn_ret, span, params, body, diags, false,
        ),
        Stmt::Forall {
            span,
            binder,
            bound,
            body,
        } => check_stmt_forall_ctx(
            ctx, state, fns, in_fn, fn_ret, span, binder, bound, body, diags,
        ),
        Stmt::Match {
            span,
            scrutinee,
            arms,
        } => check_stmt_match(span, scrutinee, arms, ctx, state, fns, in_fn, fn_ret, diags),
        _ => {}
    }
}

fn check_stmt_assign(
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    span: &Span,
    name: &str,
    rhs: &Rhs,
    diags: &mut Vec<Diag>,
) {
    let ty = check_rhs(rhs, ctx, state, fns, diags);
    ctx.bind(name, ty, span.clone(), diags);
}

fn check_stmt_ret(
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    fn_ret: Option<&Type>,
    value: &Operand,
    span: &Span,
    diags: &mut Vec<Diag>,
) {
    let got = operand_ty(value, ctx, state, fns, fn_ret, diags);
    if let Some(expected) = fn_ret {
        if !types_compatible(&got, expected) {
            diags.push(type_diag(span.clone()));
        }
    }
}

fn check_stmt_assert_eq(
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    span: &Span,
    got: &Operand,
    want: &Operand,
    diags: &mut Vec<Diag>,
) {
    if in_fn {
        diags.push(parse_diag(span.clone()));
        return;
    }
    let want_ty = match want {
        Operand::Name(name, _) => ctx.types.get(name).cloned(),
        _ => None,
    };
    let g = operand_ty(got, ctx, state, fns, want_ty.as_ref(), diags);
    let w = operand_ty(want, ctx, state, fns, Some(&g), diags);
    if !types_compatible(&g, &w) {
        diags.push(type_diag(span.clone()));
    }
}

fn check_stmt_match(
    span: &Span,
    scrutinee: &str,
    arms: &[super::syntax::MatchArm],
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    diags: &mut Vec<Diag>,
) {
    check_match(
        span.clone(),
        scrutinee,
        arms,
        ctx,
        state,
        fns,
        in_fn,
        fn_ret,
        None,
        diags,
    );
}

fn check_stmt_if(
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    span: &Span,
    cond: &Operand,
    then_body: &[Stmt],
    else_body: &[Stmt],
    diags: &mut Vec<Diag>,
) {
    operand_ty(cond, ctx, state, fns, None, diags);
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

fn with_for_binder(
    ctx: &mut Ctx,
    state: &mut CheckState,
    span: &Span,
    binder: &str,
    list: &str,
    body: &[Stmt],
    diags: &mut Vec<Diag>,
    mut each: impl FnMut(&Stmt, &mut Ctx, &mut CheckState, &mut Vec<Diag>),
) {
    let ty = match ctx.types.get(list).cloned() {
        Some(Type::List(elem, _)) => (*elem).clone(),
        _ => {
            diags.push(type_diag(span.clone()));
            scalar("i64", span.clone())
        }
    };
    let fresh = bind_for_body(ctx, binder, ty, span.clone(), diags);
    state.ctrl.push(Ctrl::For);
    for s in body {
        each(s, ctx, state, diags);
    }
    state.ctrl.pop();
    release_body_bind(ctx, binder, fresh);
}

fn check_stmt_for(
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    span: &Span,
    binder: &str,
    list: &str,
    body: &[Stmt],
    diags: &mut Vec<Diag>,
) {
    with_for_binder(
        ctx,
        state,
        span,
        binder,
        list,
        body,
        diags,
        |s, ctx, state, diags| check_stmt(s, ctx, state, fns, in_fn, fn_ret, diags),
    );
}

fn check_stmt_loop_block(
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    span: &Span,
    params: &[LoopParam],
    body: &[Stmt],
    diags: &mut Vec<Diag>,
    nested: bool,
) {
    if params.is_empty() {
        diags.push(type_diag(span.clone()));
        return;
    }
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
    state.ctrl.push(Ctrl::Loop);
    let mut loop_ctx = Ctx {
        types: ctx.types.clone(),
        phi_gaps: ctx.phi_gaps.clone(),
        match_partial: ctx.match_partial.clone(),
    };
    let param_set: HashSet<String> = param_names.iter().cloned().collect();
    for p in params {
        let ty = operand_ty(&p.init, &loop_ctx, state, fns, None, diags);
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
    state.ctrl.pop();
    let site = &state.carry.loops[site_idx];
    if !site.breaks.is_empty() {
        for p in params {
            if let Some(ty) = loop_ctx.types.get(&p.name) {
                if nested {
                    loop_ctx.phi_gaps.remove(&p.name);
                }
                ctx.phi_gaps.remove(&p.name);
                ctx.types.insert(p.name.clone(), ty.clone());
            }
        }
    }
}

fn check_continue_edges(
    span: Span,
    edges: &[ContinueEdge],
    loop_params: &HashSet<String>,
    ctx: &mut Ctx,
    state: &mut CheckState,
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
        let param_ty = ctx.types.get(&e.name).cloned();
        let val_ty = operand_ty(&e.value, ctx, state, fns, param_ty.as_ref(), diags);
        if let Some(param_ty) = &param_ty {
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

fn loop_param_assign_shadow(
    stmt: &Stmt,
    loop_params: &HashSet<String>,
    diags: &mut Vec<Diag>,
) -> bool {
    if let Stmt::Assign { span, name, .. } = stmt {
        if loop_params.contains(name) {
            diags.push(shadow_diag(span.clone()));
            return true;
        }
    }
    false
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
    if loop_param_assign_shadow(stmt, loop_params, diags) {
        return;
    }
    match stmt {
        Stmt::Continue { edges, span, .. } => {
            note_loop_continue(state, span, edges, Some(loop_params), ctx, fns, diags);
        }
        Stmt::Break { span } => note_break(state, span, diags),
        Stmt::If {
            span,
            cond,
            then_body,
            else_body,
            ..
        } => {
            operand_ty(cond, ctx, state, fns, None, diags);
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
        Stmt::Match {
            span,
            scrutinee,
            arms,
        } => check_match(
            span.clone(),
            scrutinee,
            arms,
            ctx,
            state,
            fns,
            in_fn,
            fn_ret,
            Some(loop_params),
            diags,
        ),
        Stmt::Trap { .. } => {}
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
    check_stmt_loop_block(
        ctx, state, fns, in_fn, fn_ret, span, params, body, diags, true,
    );
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
        match_partial: ctx.match_partial.clone(),
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
        match_partial: ctx.match_partial.clone(),
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

    let supplies = state.carry.joins[join_site_idx].names.clone();
    for supply in &supplies {
        let name = &supply.name;
        if pre.contains_key(name) {
            continue;
        }
        match (supply.then_arm, supply.else_arm) {
            (ArmSrc::Assign, ArmSrc::Assign) => {
                bind_both_assign_join(
                    name,
                    &span,
                    then_ctx.types.get(name),
                    else_ctx.types.get(name),
                    &then_tops,
                    &else_tops,
                    ctx,
                    diags,
                );
            }
            (ArmSrc::Assign, ArmSrc::LoopExit)
            | (ArmSrc::LoopExit, ArmSrc::Assign)
            | (ArmSrc::LoopExit, ArmSrc::LoopExit) => {
                if !publishes_loop_join(supply) {
                    ctx.phi_gaps.insert(name.clone());
                    continue;
                }
                let t_ty = join_arm_type(
                    then_body,
                    &supply.then_arm,
                    name,
                    &then_ctx,
                    state,
                    fns,
                    diags,
                );
                let e_ty = join_arm_type(
                    else_body,
                    &supply.else_arm,
                    name,
                    &else_ctx,
                    state,
                    fns,
                    diags,
                );
                match (t_ty, e_ty) {
                    (Some(a), Some(b)) if types_equal(&a, &b) => {
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
                    _ => {
                        ctx.phi_gaps.insert(name.clone());
                    }
                }
            }
            (ArmSrc::Absent, ArmSrc::Absent) => {}
            _ => {
                ctx.phi_gaps.insert(name.clone());
            }
        }
    }

    let join_site = &state.carry.joins[join_site_idx];
    let bind_names = carry::if_join_bind_names(&state.carry, join_site, then_body, else_body);
    for name in bind_names {
        if pre.contains_key(&name) || ctx.types.contains_key(&name) {
            continue;
        }
        let ty = then_ctx
            .types
            .get(&name)
            .or_else(|| else_ctx.types.get(&name))
            .cloned();
        if let Some(ty) = ty {
            ctx.publish_if_name(&name, ty);
        }
    }
}

fn bind_both_assign_join(
    name: &str,
    span: &Span,
    then_ty: Option<&Type>,
    else_ty: Option<&Type>,
    then_tops: &HashMap<String, (Type, Span)>,
    else_tops: &HashMap<String, (Type, Span)>,
    ctx: &mut Ctx,
    diags: &mut Vec<Diag>,
) {
    match (then_ty, else_ty) {
        (Some(a), Some(b)) if types_equal(a, b) => {
            insert_join_type(ctx, name, a.clone(), span, diags);
        }
        (Some(_), Some(_)) => diags.push(type_diag(span.clone())),
        _ => {
            let ty = then_tops
                .get(name)
                .map(|(t, _)| t.clone())
                .or_else(|| else_tops.get(name).map(|(t, _)| t.clone()));
            if let Some(ty) = ty {
                insert_join_type(ctx, name, ty, span, diags);
            }
        }
    }
}

fn insert_join_type(ctx: &mut Ctx, name: &str, ty: Type, span: &Span, diags: &mut Vec<Diag>) {
    if ctx.types.contains_key(name) {
        diags.push(shadow_diag(span.clone()));
    } else {
        ctx.types.insert(name.to_string(), ty);
        ctx.phi_gaps.remove(name);
    }
}

fn join_arm_type(
    arm: &[Stmt],
    src: &ArmSrc,
    name: &str,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Option<Type> {
    match src {
        ArmSrc::LoopExit => loop_param_init_ty(arm, name, ctx, state, fns, diags),
        ArmSrc::Assign => assign_arm_type(ctx, name),
        ArmSrc::Absent => None,
    }
}

fn assign_arm_type(ctx: &Ctx, name: &str) -> Option<Type> {
    ctx.types.get(name).cloned()
}

fn loop_param_init_ty(
    arm: &[Stmt],
    name: &str,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Option<Type> {
    for stmt in arm {
        if let Stmt::Loop { params, .. } = stmt {
            for p in params {
                if p.name == name {
                    return Some(operand_ty(&p.init, ctx, state, fns, None, diags));
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
            check_arm_assign(ctx, pre, state, fns, span, name, rhs, diags);
        }
        Stmt::Ret { value, span } => check_stmt_ret(ctx, state, fns, fn_ret, value, span, diags),
        Stmt::AssertEq { span, got, want } => {
            check_stmt_assert_eq(ctx, state, fns, in_fn, span, got, want, diags);
        }
        Stmt::Trap { .. } => {}
        other => check_arm_control(
            other,
            ctx,
            pre,
            state,
            fns,
            in_fn,
            fn_ret,
            loop_params,
            diags,
        ),
    }
}

fn check_arm_assign(
    ctx: &mut Ctx,
    pre: &HashMap<String, Type>,
    state: &mut CheckState,
    fns: &[&FnItem],
    span: &Span,
    name: &str,
    rhs: &Rhs,
    diags: &mut Vec<Diag>,
) {
    if pre.contains_key(name) {
        diags.push(shadow_diag(span.clone()));
        return;
    }
    let ty = check_rhs(rhs, ctx, state, fns, diags);
    if ctx.types.contains_key(name) {
        diags.push(shadow_diag(span.clone()));
    } else {
        ctx.types.insert(name.to_string(), ty);
    }
}

fn check_arm_control(
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
        Stmt::If {
            span,
            cond,
            then_body,
            else_body,
            ..
        } => {
            operand_ty(cond, ctx, state, fns, None, diags);
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
            note_loop_continue(state, span, edges, loop_params, ctx, fns, diags);
        }
        Stmt::Break { span } => note_break(state, span, diags),
        Stmt::For {
            span,
            binder,
            list,
            body,
        } => check_stmt_for_in_arm(
            ctx,
            pre,
            state,
            fns,
            in_fn,
            fn_ret,
            loop_params,
            span,
            binder,
            list,
            body,
            diags,
        ),
        Stmt::Forall {
            span,
            binder,
            bound,
            body,
        } => check_stmt_forall_in_arm(
            ctx,
            pre,
            state,
            fns,
            in_fn,
            fn_ret,
            loop_params,
            span,
            binder,
            bound,
            body,
            diags,
        ),
        Stmt::Loop {
            span, params, body, ..
        } => check_nested_loop_in_loop_body(
            span, params, body, ctx, state, fns, in_fn, fn_ret, diags,
        ),
        Stmt::Match {
            span,
            scrutinee,
            arms,
        } => check_match(
            span.clone(),
            scrutinee,
            arms,
            ctx,
            state,
            fns,
            in_fn,
            fn_ret,
            loop_params,
            diags,
        ),
        _ => {}
    }
}

fn check_stmt_for_in_arm(
    ctx: &mut Ctx,
    pre: &HashMap<String, Type>,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    loop_params: Option<&HashSet<String>>,
    span: &Span,
    binder: &str,
    list: &str,
    body: &[Stmt],
    diags: &mut Vec<Diag>,
) {
    if pre.contains_key(binder) {
        diags.push(shadow_diag(span.clone()));
        return;
    }
    with_for_binder(
        ctx,
        state,
        span,
        binder,
        list,
        body,
        diags,
        |s, ctx, state, diags| {
            check_stmt_in_arm(s, ctx, pre, state, fns, in_fn, fn_ret, loop_params, diags);
        },
    );
}

fn check_stmt_forall_in_arm(
    ctx: &mut Ctx,
    pre: &HashMap<String, Type>,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    loop_params: Option<&HashSet<String>>,
    span: &Span,
    binder: &str,
    bound: &ForallBound,
    body: &[Stmt],
    diags: &mut Vec<Diag>,
) {
    check_forall_bound(bound, state, diags);
    let ty = forall_binder_ty(bound, ctx, diags);
    if pre.contains_key(binder) {
        diags.push(shadow_diag(span.clone()));
    }
    let mut inner = Ctx {
        types: ctx.types.clone(),
        phi_gaps: ctx.phi_gaps.clone(),
        match_partial: ctx.match_partial.clone(),
    };
    if inner.types.contains_key(binder) {
        diags.push(shadow_diag(span.clone()));
    } else {
        inner.bind(binder, ty, span.clone(), diags);
    }
    let binder_name = binder.to_string();
    for s in body {
        if let Stmt::Assign {
            name, span: sspan, ..
        } = s
        {
            if name == &binder_name {
                diags.push(shadow_diag(sspan.clone()));
            }
        }
        check_stmt_in_arm(
            s,
            &mut inner,
            pre,
            state,
            fns,
            in_fn,
            fn_ret,
            loop_params,
            diags,
        );
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
        Rhs::BinOp { op, ty, span, .. } => binop_result_ty(op, ty, span),
        Rhs::Hole { ty, .. } => ty.clone(),
        _ => scalar("i64", empty_span()),
    }
}

fn forall_binder_ty(bound: &ForallBound, ctx: &Ctx, diags: &mut Vec<Diag>) -> Type {
    match bound {
        ForallBound::Range { ty, span, .. } => {
            let i64_ty = scalar("i64", span.clone());
            if !types_compatible(ty, &i64_ty) {
                diags.push(type_diag(span.clone()));
            }
            i64_ty
        }
        ForallBound::List { ty, name, span } => match ctx.types.get(name) {
            Some(Type::List(elem, _)) => {
                if !types_compatible(ty, elem) {
                    diags.push(type_diag(span.clone()));
                }
                elem.as_ref().clone()
            }
            _ => {
                diags.push(type_diag(span.clone()));
                ty.clone()
            }
        },
    }
}

fn resolve_static_i64(
    b: &StaticI64,
    module_int_binds: &HashMap<String, i128>,
    diags: &mut Vec<Diag>,
) -> Option<i128> {
    match b {
        StaticI64::Lit(n, _) => Some(*n),
        StaticI64::Name(name, span) => {
            if let Some(v) = module_int_binds.get(name) {
                Some(*v)
            } else {
                diags.push(forall_diag(span.clone()));
                None
            }
        }
    }
}

fn check_forall_bound(bound: &ForallBound, state: &CheckState, diags: &mut Vec<Diag>) {
    if let ForallBound::Range { lo, hi, span, .. } = bound {
        let lo_v = resolve_static_i64(lo, &state.module_int_binds, diags);
        let hi_v = resolve_static_i64(hi, &state.module_int_binds, diags);
        if let (Some(lo_v), Some(hi_v)) = (lo_v, hi_v) {
            if !fits_i64(lo_v) || !fits_i64(hi_v) {
                diags.push(forall_diag(span.clone()));
                return;
            }
            let span128 = hi_v - lo_v + 1;
            if hi_v < lo_v || span128 <= 0 || span128 > i64::MAX as i128 {
                diags.push(forall_diag(span.clone()));
            }
        }
    }
}

fn catalog_check_diag(code: Code, span: Span) -> Diag {
    Diag {
        code,
        kind: Kind::Check,
        span,
        want: None,
        got: None,
        patch: None,
    }
}

fn forall_diag(span: Span) -> Diag {
    catalog_check_diag(Code::EForall, span)
}

fn check_stmt_forall_ctx(
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    span: &Span,
    binder: &str,
    bound: &ForallBound,
    body: &[Stmt],
    diags: &mut Vec<Diag>,
) {
    check_forall_bound(bound, state, diags);
    let ty = forall_binder_ty(bound, ctx, diags);
    let fresh = bind_for_body(ctx, binder, ty, span.clone(), diags);
    let mut inner = Ctx {
        types: ctx.types.clone(),
        phi_gaps: ctx.phi_gaps.clone(),
        match_partial: ctx.match_partial.clone(),
    };
    let forall_name = binder.to_string();
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
    release_body_bind(ctx, binder, fresh);
}

fn check_rhs(
    rhs: &Rhs,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    match rhs {
        Rhs::BinOp { .. } | Rhs::Call { .. } | Rhs::Not { .. } => {
            check_rhs_op(rhs, ctx, state, fns, diags)
        }
        Rhs::ListEmpty { .. }
        | Rhs::ListCons { .. }
        | Rhs::ListLen { .. }
        | Rhs::ListGet { .. } => check_rhs_list(rhs, ctx, state, fns, diags),
        Rhs::BytesNew { .. }
        | Rhs::BytesLen { .. }
        | Rhs::BytesGet { .. }
        | Rhs::Record { .. }
        | Rhs::Project { .. } => check_rhs_data(rhs, ctx, state, fns, diags),
        Rhs::Load { .. } | Rhs::Store { .. } => check_rhs_region(rhs, ctx, state, fns, diags),
        Rhs::Ok { ty, value, span } => {
            check_ok_err_rhs(true, ty, value, span, ctx, state, fns, diags)
        }
        Rhs::Err { ty, value, span } => {
            check_ok_err_rhs(false, ty, value, span, ctx, state, fns, diags)
        }
        Rhs::Operand(op) => operand_ty(op, ctx, state, fns, None, diags),
        Rhs::Hole { ty, .. } => ty.clone(),
    }
}

fn check_rhs_op(
    rhs: &Rhs,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    match rhs {
        Rhs::BinOp {
            ty,
            lhs,
            rhs,
            span,
            op,
        } => check_binop(op, ty, lhs, rhs, span, ctx, state, fns, diags),
        Rhs::Call { span, callee, args } => check_call(span, callee, args, ctx, state, fns, diags),
        Rhs::Not { operand, span } => check_not(operand, span, ctx, state, fns, diags),
        _ => unreachable!("check_rhs_op: op forms only"),
    }
}

#[allow(clippy::too_many_arguments)]
fn check_binop(
    op: &BinOp,
    ty: &Type,
    lhs: &Operand,
    rhs: &Operand,
    span: &Span,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    if cmp_forbidden(op, ty, lhs, rhs, ctx, state) {
        diags.push(type_diag(span.clone()));
        return ty.clone();
    }
    let lt = operand_ty(lhs, ctx, state, fns, Some(ty), diags);
    let rt = operand_ty(rhs, ctx, state, fns, Some(ty), diags);
    if !types_compatible(&lt, ty) || !types_compatible(&rt, ty) {
        diags.push(type_diag(span.clone()));
    }
    binop_result_ty(op, ty, span)
}

fn check_call(
    span: &Span,
    callee: &str,
    args: &[Operand],
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    let Some((params, ret)) = resolve_fn(callee, fns) else {
        diags.push(import_diag(span.clone()));
        return scalar("i64", span.clone());
    };
    if params.len() != args.len() {
        diags.push(type_diag(span.clone()));
    }
    for (arg, param_ty) in args.iter().zip(params.iter()) {
        let at = operand_ty(arg, ctx, state, fns, Some(param_ty), diags);
        if !types_compatible(&at, param_ty) {
            diags.push(type_diag(span.clone()));
        }
    }
    ret
}

fn check_not(
    operand: &Operand,
    span: &Span,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    let bool_ty = scalar("bool", span.clone());
    let t = operand_ty(operand, ctx, state, fns, Some(&bool_ty), diags);
    if !types_compatible(&t, &bool_ty) {
        diags.push(type_diag(span.clone()));
    }
    bool_ty
}

fn check_rhs_list(
    rhs: &Rhs,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    match rhs {
        Rhs::ListEmpty { span, elem } => Type::List(Box::new(elem.clone()), span.clone()),
        Rhs::ListCons { span, head, tail } => {
            check_list_cons(span, head, tail, ctx, state, fns, diags)
        }
        Rhs::ListLen { span, list } => check_list_len(span, list, ctx, state, fns, diags),
        Rhs::ListGet { span, list, index } => {
            check_list_get(span, list, index, ctx, state, fns, diags)
        }
        _ => unreachable!("check_rhs_list: list forms only"),
    }
}

fn check_rhs_data(
    rhs: &Rhs,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    match rhs {
        Rhs::BytesNew { span, .. } => scalar("bytes", span.clone()),
        Rhs::BytesLen { span, bytes } => check_bytes_len(span, bytes, ctx, state, fns, diags),
        Rhs::BytesGet { span, bytes, index } => {
            check_bytes_get(span, bytes, index, ctx, state, fns, diags)
        }
        Rhs::Record { span, fields } => check_record(span, fields, ctx, state, fns, diags),
        Rhs::Project { span, base, field } => check_project(span, base, field, ctx, state, diags),
        _ => unreachable!("check_rhs_data: bytes/record forms only"),
    }
}

/// `load T addr` is `T`; `store T addr v` is `unit` and `v` must be `T`.
fn check_rhs_region(
    rhs: &Rhs,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    match rhs {
        Rhs::Load { span, ty, addr } => {
            check_region_access(span, ty, addr, ctx, state, fns, diags);
            ty.clone()
        }
        Rhs::Store {
            span,
            ty,
            addr,
            value,
        } => {
            check_region_access(span, ty, addr, ctx, state, fns, diags);
            let vt = operand_ty(value, ctx, state, fns, Some(ty), diags);
            if !types_compatible(&vt, ty) {
                diags.push(type_diag(span.clone()));
            }
            scalar("unit", span.clone())
        }
        _ => unreachable!("check_rhs_region: load/store only"),
    }
}

/// Compares yield `bool`; arithmetic yields the operand type.
fn binop_result_ty(op: &BinOp, ty: &Type, span: &Span) -> Type {
    match op {
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            scalar("bool", span.clone())
        }
        _ => ty.clone(),
    }
}

fn cmp_forbidden(
    op: &BinOp,
    ty: &Type,
    lhs: &Operand,
    rhs: &Operand,
    ctx: &Ctx,
    state: &CheckState,
) -> bool {
    let forbid = match op {
        BinOp::Eq | BinOp::Ne => eq_forbidden_ty,
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => ordered_forbidden_ty,
        _ => return false,
    };
    forbid(ty, state)
        || forbid_operand(lhs, ctx, state, forbid)
        || forbid_operand(rhs, ctx, state, forbid)
}

fn eq_forbidden_ty(ty: &Type, state: &CheckState) -> bool {
    match ty {
        Type::List(_, _) | Type::Result(_, _, _) => true,
        Type::Named(n, _) => {
            n == "unit" || state.sum_types.contains_key(n) || state.record_types.contains_key(n)
        }
    }
}

fn ordered_forbidden_ty(ty: &Type, state: &CheckState) -> bool {
    match ty {
        Type::Named(n, _) if n == "bytes" || n == "bool" => true,
        _ => eq_forbidden_ty(ty, state),
    }
}

fn forbid_operand(
    op: &Operand,
    ctx: &Ctx,
    state: &CheckState,
    forbid: fn(&Type, &CheckState) -> bool,
) -> bool {
    match op {
        Operand::Name(name, _) => ctx.types.get(name).is_some_and(|t| forbid(t, state)),
        _ => false,
    }
}

fn check_list_cons(
    span: &Span,
    head: &Operand,
    tail: &Operand,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    let tail_ty = operand_ty(tail, ctx, state, fns, None, diags);
    let Type::List(inner, _) = tail_ty else {
        let head_ty = operand_ty(head, ctx, state, fns, None, diags);
        diags.push(type_diag(span.clone()));
        return Type::List(Box::new(head_ty), span.clone());
    };
    let head_ty = operand_ty(head, ctx, state, fns, Some(inner.as_ref()), diags);
    if !types_compatible(&head_ty, &inner) {
        diags.push(type_diag(span.clone()));
    }
    Type::List(inner, span.clone())
}

fn check_list_len(
    span: &Span,
    list: &Operand,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    let ty = operand_ty(list, ctx, state, fns, None, diags);
    if !matches!(ty, Type::List(_, _)) {
        diags.push(type_diag(span.clone()));
    }
    scalar("i64", span.clone())
}

fn check_list_get(
    span: &Span,
    list: &Operand,
    index: &Operand,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    let list_ty = operand_ty(list, ctx, state, fns, None, diags);
    let index_ty = operand_ty(
        index,
        ctx,
        state,
        fns,
        Some(&scalar("i64", span.clone())),
        diags,
    );
    if !types_compatible(&index_ty, &scalar("i64", span.clone())) {
        diags.push(type_diag(span.clone()));
    }
    let elem = match list_ty {
        Type::List(inner, _) => *inner,
        _ => {
            diags.push(type_diag(span.clone()));
            scalar("i64", span.clone())
        }
    };
    Type::Result(
        Box::new(elem),
        Box::new(scalar("unit", span.clone())),
        span.clone(),
    )
}

/// `load`/`store`: `E_REGION` without `import region mem : region`; `E_TYPE`
/// unless `T` is `i32 i64 f32 f64 bool` and the address is `i64`
/// (SPEC "Traps and region").
fn check_region_access(
    span: &Span,
    ty: &Type,
    addr: &Operand,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) {
    if !state.has_region {
        diags.push(catalog_check_diag(Code::ERegion, span.clone()));
    }
    let scalar_ok = matches!(ty, Type::Named(n, _) if ScalarTy::from_name(n).is_some());
    let i64_ty = scalar("i64", span.clone());
    let at = operand_ty(addr, ctx, state, fns, Some(&i64_ty), diags);
    if !scalar_ok || !types_compatible(&at, &i64_ty) {
        diags.push(type_diag(span.clone()));
    }
}

fn is_bytes_ty(ty: &Type) -> bool {
    matches!(ty, Type::Named(n, _) if n == "bytes")
}

fn check_bytes_len(
    span: &Span,
    bytes: &Operand,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    let ty = operand_ty(bytes, ctx, state, fns, None, diags);
    if !is_bytes_ty(&ty) {
        diags.push(type_diag(span.clone()));
    }
    scalar("i64", span.clone())
}

fn check_bytes_get(
    span: &Span,
    bytes: &Operand,
    index: &Operand,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    let bytes_ty = operand_ty(bytes, ctx, state, fns, None, diags);
    let index_ty = operand_ty(
        index,
        ctx,
        state,
        fns,
        Some(&scalar("i64", span.clone())),
        diags,
    );
    if !is_bytes_ty(&bytes_ty) || !types_compatible(&index_ty, &scalar("i64", span.clone())) {
        diags.push(type_diag(span.clone()));
    }
    Type::Result(
        Box::new(scalar("i64", span.clone())),
        Box::new(scalar("unit", span.clone())),
        span.clone(),
    )
}

fn check_record(
    span: &Span,
    fields: &[(String, Operand)],
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    let mut names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
    names.sort_unstable();
    if names.windows(2).any(|w| w[0] == w[1]) {
        diags.push(type_diag(span.clone()));
        return scalar("i64", span.clone());
    }
    let records = state.record_types.clone();
    let Some(rname) = unique_record(&records, &names, fields, ctx, state, fns) else {
        diags.push(type_diag(span.clone()));
        for (_, op) in fields {
            operand_ty(op, ctx, state, fns, None, diags);
        }
        return scalar("i64", span.clone());
    };
    let decls = state.record_types.get(&rname).cloned().unwrap_or_default();
    for (fname, fty) in &decls {
        let Some((_, op)) = fields.iter().find(|(n, _)| n == fname) else {
            diags.push(type_diag(span.clone()));
            continue;
        };
        let got = operand_ty(op, ctx, state, fns, Some(fty), diags);
        if !types_compatible(&got, fty) {
            diags.push(type_diag(span.clone()));
        }
    }
    Type::Named(rname, span.clone())
}

fn unique_record(
    records: &HashMap<String, Vec<(String, Type)>>,
    names: &[&str],
    fields: &[(String, Operand)],
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
) -> Option<String> {
    let saved_partial = state.partial_used;
    let mut hits = Vec::new();
    for (rname, decls) in records {
        let mut dn: Vec<&str> = decls.iter().map(|(n, _)| n.as_str()).collect();
        dn.sort_unstable();
        if dn == names && record_operands_fit(decls, fields, ctx, state, fns) {
            hits.push(rname.clone());
        }
    }
    state.partial_used = saved_partial;
    if hits.len() == 1 {
        hits.pop()
    } else {
        None
    }
}

fn record_operands_fit(
    decls: &[(String, Type)],
    fields: &[(String, Operand)],
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
) -> bool {
    let mut scratch = Vec::new();
    for (fname, fty) in decls {
        let Some((_, op)) = fields.iter().find(|(n, _)| n == fname) else {
            return false;
        };
        let before = scratch.len();
        let got = operand_ty(op, ctx, state, fns, Some(fty), &mut scratch);
        if scratch.len() != before || !types_compatible(&got, fty) {
            return false;
        }
    }
    true
}

fn check_project(
    span: &Span,
    base: &str,
    field: &str,
    ctx: &Ctx,
    state: &CheckState,
    diags: &mut Vec<Diag>,
) -> Type {
    let Some(Type::Named(rname, _)) = ctx.types.get(base) else {
        diags.push(type_diag(span.clone()));
        return scalar("i64", span.clone());
    };
    let Some(fields) = state.record_types.get(rname) else {
        diags.push(type_diag(span.clone()));
        return scalar("i64", span.clone());
    };
    match fields.iter().find(|(n, _)| n == field) {
        Some((_, ty)) => ty.clone(),
        None => {
            diags.push(type_diag(span.clone()));
            scalar("i64", span.clone())
        }
    }
}

fn check_ok_err_rhs(
    is_ok: bool,
    written: &Type,
    value: &Operand,
    span: &Span,
    ctx: &Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    let val_ty = operand_ty(value, ctx, state, fns, Some(written), diags);
    if !types_compatible(&val_ty, written) {
        diags.push(type_diag(span.clone()));
    }
    let unit = scalar("unit", span.clone());
    if is_ok {
        Type::Result(Box::new(written.clone()), Box::new(unit), span.clone())
    } else {
        Type::Result(Box::new(unit), Box::new(written.clone()), span.clone())
    }
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
    ctx: &Ctx,
    state: &mut CheckState,
    _fns: &[&FnItem],
    expected: Option<&Type>,
    diags: &mut Vec<Diag>,
) -> Type {
    match op {
        Operand::Name(name, span) => {
            if let Some(ty) = ctx.types.get(name) {
                return ty.clone();
            }
            if ctx.match_partial.contains(name) {
                state.partial_used = true;
                return scalar("i64", span.clone());
            }
            if ctx.phi_gaps.contains(name) {
                diags.push(if_phi_diag(span.clone()));
                return scalar("i64", span.clone());
            }
            diags.push(type_diag(span.clone()));
            scalar("i64", span.clone())
        }
        Operand::Int(n, span) => int_literal_ty(*n, span.clone(), expected, diags),
        Operand::Unit(span) => Type::Named("unit".into(), span.clone()),
        Operand::Bool(_, span) => Type::Named("bool".into(), span.clone()),
        Operand::Float(x, span) => float_literal_ty(*x, span.clone(), expected),
        Operand::Bytes(_, span) => Type::Named("bytes".into(), span.clone()),
    }
}

/// A float literal takes the operand type (SPEC "ANF"): `f32` where `f32` is
/// expected and the literal is exact in `f32`, else `f64`. An inexact literal
/// stays `f64`, so an `f32` slot reports `E_TYPE` instead of rounding silently.
fn float_literal_ty(x: f64, span: Span, expected: Option<&Type>) -> Type {
    let exact_f32 = f64::from(x as f32) == x;
    match expected {
        Some(Type::Named(name, _)) if name == "f32" && exact_f32 => scalar("f32", span),
        _ => scalar("f64", span),
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
    // i32 and i64 are distinct types: no implicit coercion (GMA-372).
    // A constructed `ok`/`err` leaves the other side as `unit`. That found
    // `unit` still fits the expected side. Joins do not use this fit.
    match (found, expected) {
        (Type::Result(ok_found, err_found, _), Type::Result(ok_expected, err_expected, _)) => {
            side_fits(ok_found, ok_expected) && side_fits(err_found, err_expected)
        }
        _ => types_equal(found, expected),
    }
}

fn side_fits(found: &Type, expected: &Type) -> bool {
    types_equal(found, expected) || found_unit(found)
}

fn found_unit(ty: &Type) -> bool {
    matches!(ty, Type::Named(name, _) if name == "unit")
}

fn result_side_compatible(a: &Type, b: &Type) -> bool {
    types_equal(a, b)
}

fn types_equal(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Named(na, _), Type::Named(nb, _)) => na == nb,
        (Type::List(ia, _), Type::List(ib, _)) => types_equal(ia, ib),
        (Type::Result(oa, ea, _), Type::Result(ob, eb, _)) => {
            result_side_compatible(oa, ob) && result_side_compatible(ea, eb)
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
    catalog_check_diag(Code::EType, span)
}

fn shadow_diag(span: Span) -> Diag {
    catalog_check_diag(Code::EShadow, span)
}

fn if_phi_diag(span: Span) -> Diag {
    catalog_check_diag(Code::EIfPhi, span)
}

fn import_diag(span: Span) -> Diag {
    catalog_check_diag(Code::EImport, span)
}

fn hole_diag(span: Span) -> Diag {
    catalog_check_diag(Code::EHole, span)
}

fn parse_diag(span: Span) -> Diag {
    catalog_check_diag(Code::EParse, span)
}

#[allow(clippy::too_many_arguments)]
fn check_match(
    span: Span,
    scrutinee: &str,
    arms: &[super::syntax::MatchArm],
    ctx: &mut Ctx,
    state: &mut CheckState,
    fns: &[&FnItem],
    in_fn: bool,
    fn_ret: Option<&Type>,
    loop_params: Option<&HashSet<String>>,
    diags: &mut Vec<Diag>,
) {
    let scrut_ty = ctx.types.get(scrutinee).cloned();
    let (tags, payloads) = match scrut_ty {
        Some(Type::Result(ok, err, _)) => (
            vec!["ok".to_string(), "err".to_string()],
            vec![Some(ok.as_ref().clone()), Some(err.as_ref().clone())],
        ),
        Some(Type::Named(name, _)) => {
            if let Some(info) = state.sum_types.get(&name) {
                (info.tags.clone(), info.payloads.clone())
            } else {
                diags.push(type_diag(span.clone()));
                return;
            }
        }
        _ => {
            diags.push(type_diag(span.clone()));
            return;
        }
    };

    let mut seen_tags = HashSet::new();
    let mut arm_supplies = Vec::new();
    let mut arm_local_types: Vec<HashMap<String, Type>> = Vec::new();
    let site_id = state.alloc_site_id();
    let match_idx = state.carry.matches.len();

    for arm in arms {
        if !tags.iter().any(|t| t == &arm.tag) {
            diags.push(type_diag(arm.span.clone()));
            continue;
        }
        if !seen_tags.insert(arm.tag.clone()) {
            diags.push(type_diag(arm.span.clone()));
            continue;
        }
        let tag_index = tags.iter().position(|t| t == &arm.tag).unwrap() as u32;
        let expected_payload = payloads.get(tag_index as usize).and_then(|p| p.as_ref());

        let mut arm_ctx = Ctx {
            types: ctx.types.clone(),
            phi_gaps: ctx.phi_gaps.clone(),
            match_partial: ctx.match_partial.clone(),
        };
        if let Some(binder) = &arm.binder {
            if binder != "_" {
                if arm_ctx.types.contains_key(binder) {
                    diags.push(shadow_diag(arm.span.clone()));
                } else if let Some(payload_ty) = expected_payload {
                    arm_ctx.bind(binder, payload_ty.clone(), arm.span.clone(), diags);
                }
            }
        }

        for stmt in &arm.body {
            if let Some(params) = loop_params {
                check_stmt_loop(stmt, &mut arm_ctx, state, params, fns, in_fn, fn_ret, diags);
            } else {
                check_stmt_in_arm(
                    stmt,
                    &mut arm_ctx,
                    &ctx.types,
                    state,
                    fns,
                    in_fn,
                    fn_ret,
                    None,
                    diags,
                );
            }
        }

        let reaches = stmts_may_fall_through(&arm.body);
        let delivered = if reaches {
            match_arm_delivered(&arm.body, &state.carry)
        } else {
            Vec::new()
        };
        let mut local = HashMap::new();
        for n in &delivered {
            if let Some(ty) = arm_ctx.types.get(n) {
                local.insert(n.clone(), ty.clone());
            }
        }
        arm_local_types.push(local);
        arm_supplies.push(MatchArmSupply {
            tag_index,
            reaches,
            delivered,
        });
    }

    for tag in &tags {
        if !seen_tags.contains(tag) {
            diags.push(match_diag(span.clone()));
        }
    }

    state.carry.matches.push(MatchSite {
        id: site_id,
        span: span.clone(),
        scrutinee: scrutinee.to_string(),
        tags: tags.clone(),
        arms: arm_supplies,
    });

    let site = &state.carry.matches[match_idx];
    let phi_names = match_phi_names(site);
    let reaching: Vec<&MatchArmSupply> = site.arms.iter().filter(|a| a.reaches).collect();
    if reaching.is_empty() {
        return;
    }

    let mut all_delivered: HashSet<String> = HashSet::new();
    for arm in &reaching {
        for n in &arm.delivered {
            all_delivered.insert(n.clone());
        }
    }

    for name in all_delivered {
        let in_all = reaching
            .iter()
            .all(|a| a.delivered.iter().any(|d| d == &name));
        let in_some = reaching
            .iter()
            .any(|a| a.delivered.iter().any(|d| d == &name));
        if in_all {
            publish_match_arm_type(ctx, &name, &span, &site.arms, &arm_local_types, diags);
        } else if in_some {
            ctx.match_partial.insert(name.clone());
        }
    }

    for name in &phi_names {
        ctx.match_partial.remove(name);
    }
}

fn delivered_arm_type<'a>(
    name: &str,
    supply: &MatchArmSupply,
    locals: &'a HashMap<String, Type>,
) -> Option<&'a Type> {
    if !supply.reaches {
        return None;
    }
    locals.get(name)
}

enum ArmAgreement {
    Same(Type),
    Conflict,
}

fn match_arm_agreement(
    name: &str,
    span: &Span,
    arms: &[MatchArmSupply],
    locals: &[HashMap<String, Type>],
) -> ArmAgreement {
    let mut agreed: Option<Type> = None;
    for (supply, local) in arms.iter().zip(locals) {
        let Some(ty) = delivered_arm_type(name, supply, local) else {
            continue;
        };
        match &agreed {
            None => agreed = Some(ty.clone()),
            Some(prev) => {
                if !types_equal(prev, ty) {
                    return ArmAgreement::Conflict;
                }
            }
        }
    }
    let ty = agreed.unwrap_or_else(|| scalar("i64", span.clone()));
    ArmAgreement::Same(ty)
}

fn publish_match_arm_type(
    ctx: &mut Ctx,
    name: &str,
    span: &Span,
    arms: &[MatchArmSupply],
    locals: &[HashMap<String, Type>],
    diags: &mut Vec<Diag>,
) {
    match match_arm_agreement(name, span, arms, locals) {
        ArmAgreement::Conflict => diags.push(type_diag(span.clone())),
        ArmAgreement::Same(ty) => bind_agreed_match_type(ctx, name, ty),
    }
}

fn bind_agreed_match_type(ctx: &mut Ctx, name: &str, ty: Type) {
    ctx.match_partial.remove(name);
    if !ctx.types.contains_key(name) {
        ctx.types.insert(name.to_string(), ty);
    }
    ctx.phi_gaps.remove(name);
}

fn match_diag(span: Span) -> Diag {
    catalog_check_diag(Code::EMatch, span)
}

fn seq_diag(span: Span) -> Diag {
    catalog_check_diag(Code::ESeq, span)
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
        let diags = match sketch.unwrap_err() {
            CheckFailure::Diags(d) => d,
            CheckFailure::Codeless(_) => panic!("expected diags"),
        };
        assert!(diags.iter().any(|d| d.code == Code::ESeq));
    }

    #[test]
    fn call_i32_param_accepts_small_literal() {
        let src = "strake 1\n\nfn id32(a: i32) -> i32\n  ret a\nend\n\ntest t\n  r = call id32(3)\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        assert!(check(&m, false).is_ok());
    }

    fn check_errs(src: &str) -> bool {
        let m = syntax::parse(src, "t.strake").expect("parse");
        matches!(
            check(&m, false),
            Err(CheckFailure::Diags(d)) if d.iter().any(|diag| diag.code == Code::EType)
        )
    }

    fn check_ok(src: &str) -> bool {
        let m = syntax::parse(src, "t.strake").expect("parse");
        check(&m, false).is_ok()
    }

    #[test]
    fn list_cons_type_mismatch_is_etype() {
        let src = "strake 1\n\nfn f() -> i64\n  e = list.empty bool\n  xs = list.cons 1 e\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_errs(src));
        let ok = "strake 1\n\nfn f() -> i64\n  e = list.empty i64\n  xs = list.cons 1 e\n  n = list.len xs\n  ret n\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_ok(ok));
    }

    #[test]
    fn list_get_i32_index_is_etype() {
        let src = "strake 1\n\nfn f() -> i64\n  e = list.empty i64\n  i = add i32 0 0\n  r = list.get e i\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_errs(src));
    }

    #[test]
    fn eq_list_and_record_is_etype() {
        let list = "strake 1\n\nfn f() -> i64\n  e = list.empty i64\n  c = eq list i64 e e\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let rec = "strake 1\n\ntype Point { x: i64, y: i64 }\n\nfn f(a: Point, b: Point) -> i64\n  c = eq Point a b\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let sum = "strake 1\n\ntype Color { Blue unit | Red i64 }\n\nfn f(a: Color, b: Color) -> i64\n  c = eq Color a b\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_errs(list));
        assert!(check_errs(rec));
        assert!(check_errs(sum));
    }

    #[test]
    fn record_construct_and_project_types() {
        let ok = "strake 1\n\ntype Point { x: i64, y: i64 }\n\nfn f() -> i64\n  p = { y: 5, x: 4 }\n  v = p.x\n  ret v\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_ok(ok));
        let missing = "strake 1\n\ntype Point { x: i64, y: i64 }\n\nfn f() -> i64\n  p = { x: 4 }\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let bad_ty = "strake 1\n\ntype Point { x: i64, y: i64 }\n\nfn f() -> i64\n  p = { x: true, y: 1 }\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let unk = "strake 1\n\ntype Point { x: i64, y: i64 }\n\nfn g(p: Point) -> i64\n  v = p.z\n  ret v\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_errs(missing));
        assert!(check_errs(bad_ty));
        assert!(check_errs(unk));
    }

    #[test]
    fn ret_i32_accepts_small_literal() {
        let src = "strake 1\n\nfn f() -> i32\n  ret 1\nend\n\ntest t\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        assert!(check(&m, false).is_ok());
    }

    #[test]
    fn ret_i32_rejects_literal_outside_i32() {
        let src = "strake 1\n\nfn f() -> i32\n  ret 3000000000\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_errs(src));
    }

    #[test]
    fn continue_i32_param_accepts_small_literal() {
        let src = "strake 1\n\ntest t\n  x = add i32 1 2\n  loop p = x\n    continue p = 1\n  end\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        assert!(check(&m, false).is_ok());
    }

    #[test]
    fn assert_eq_i32_literal_either_side() {
        let src = "strake 1\n\ntest t\n  x = add i32 1 2\n  assert.eq 1 x\n  assert.eq x 1\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        assert!(check(&m, false).is_ok());
    }

    #[test]
    fn types_compatible_rejects_i32_i64_both_directions() {
        let s = empty_span;
        assert!(!types_compatible(&scalar("i32", s()), &scalar("i64", s())));
        assert!(!types_compatible(&scalar("i64", s()), &scalar("i32", s())));
        assert!(types_compatible(&scalar("i32", s()), &scalar("i32", s())));
        assert!(types_compatible(&scalar("i64", s()), &scalar("i64", s())));
    }

    #[test]
    fn call_i32_param_rejects_i64_value() {
        let src = "strake 1\n\nfn id32(a: i32) -> i32\n  ret a\nend\n\ntest t\n  x = add i64 1 2\n  r = call id32(x)\n  ret 0\nend\n";
        assert!(check_errs(src));
    }

    #[test]
    fn call_i64_param_rejects_i32_value() {
        let src = "strake 1\n\nfn id64(a: i64) -> i64\n  ret a\nend\n\ntest t\n  x = add i32 1 2\n  r = call id64(x)\n  ret 0\nend\n";
        assert!(check_errs(src));
    }

    #[test]
    fn ret_rejects_i32_i64_mismatch_both_directions() {
        let a = "strake 1\n\nfn f(a: i32) -> i64\n  ret a\nend\n\ntest t\n  ret 0\nend\n";
        let b = "strake 1\n\nfn f(a: i64) -> i32\n  ret a\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_errs(a));
        assert!(check_errs(b));
    }

    #[test]
    fn same_width_values_still_check() {
        let src = "strake 1\n\nfn id32(a: i32) -> i32\n  ret a\nend\n\ntest t\n  x = add i32 1 2\n  r = call id32(x)\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        assert!(check(&m, false).is_ok());
    }

    #[test]
    fn shadow_rebind_in_fn() {
        let src = "strake 1\n\nfn f() -> i64\n  x = add i64 1 2\n  x = add i64 x 1\n  ret x\nend\n\ntest t\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let err = match check(&m, false).unwrap_err() {
            CheckFailure::Diags(d) => d,
            CheckFailure::Codeless(_) => panic!("expected diags"),
        };
        assert!(err.iter().any(|d| d.code == Code::EShadow));
    }

    #[test]
    fn if_phi_missing_arm() {
        let src = "strake 1\n\nfn f(c: bool) -> i64\n  if c\n    t = add i64 1 2\n  else\n    u = add i64 3 4\n  end\n  ret t\nend\n\ntest t\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let err = match check(&m, false).unwrap_err() {
            CheckFailure::Diags(d) => d,
            CheckFailure::Codeless(_) => panic!("expected diags"),
        };
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
        let err = match check(&m, false).unwrap_err() {
            CheckFailure::Diags(d) => d,
            CheckFailure::Codeless(_) => panic!("expected diags"),
        };
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
    fn forall_lo0_hi_max_is_forall() {
        let src = "strake 1\n\nproperty p\n  forall n: i64 in 0..9223372036854775807\n  end\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let err = match check(&m, false).unwrap_err() {
            CheckFailure::Diags(d) => d,
            CheckFailure::Codeless(_) => panic!("expected diags"),
        };
        assert!(err.iter().any(|d| d.code == Code::EForall));
    }

    #[test]
    fn forall_lo1_hi_max_check_ok() {
        let src = "strake 1\n\nproperty p\n  forall n: i64 in 1..9223372036854775807\n  end\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        assert!(check(&m, false).is_ok());
    }

    #[test]
    fn property_forall_binder_rebind_shadow() {
        let src =
            "strake 1\n\nproperty p\n  forall n: i64 in 0..1\n    n = add i64 n 1\n  end\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let err = match check(&m, false).unwrap_err() {
            CheckFailure::Diags(d) => d,
            CheckFailure::Codeless(_) => panic!("expected diags"),
        };
        assert!(err.iter().any(|d| d.code == Code::EShadow));
    }

    #[test]
    fn forall_range_unknown_module_bind_is_forall() {
        let src = "strake 1\n\nproperty p\n  forall n: i64 in lo..hi\n  end\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let err = match check(&m, false).unwrap_err() {
            CheckFailure::Diags(d) => d,
            CheckFailure::Codeless(_) => panic!("expected diags"),
        };
        assert!(err.iter().all(|d| d.code == Code::EForall));
        assert_eq!(err.len(), 2, "one E_FORALL per missing module bind");
    }

    #[test]
    fn forall_rev_in_match_arm_is_e_forall() {
        let src = include_str!("../../../../golden/forall_rev_match_arm.strake");
        let m = syntax::parse(src, "forall_rev_match_arm.strake").expect("parse");
        let err = match check(&m, false).unwrap_err() {
            CheckFailure::Diags(d) => d,
            CheckFailure::Codeless(_) => panic!("expected diags"),
        };
        assert!(err.iter().any(|d| d.code == Code::EForall));
    }

    fn t4_check_codes(src: &str) -> Vec<Code> {
        let m = syntax::parse(src, "t.strake").expect("parse");
        match check(&m, false) {
            Ok(_) => Vec::new(),
            Err(CheckFailure::Diags(d)) => d.iter().map(|d| d.code).collect(),
            Err(CheckFailure::Codeless(c)) => panic!("unexpected codeless: {c:?}"),
        }
    }

    #[test]
    fn t4_named_bounds_check_like_literals() {
        let max = i64::MAX.to_string();
        for (lo, hi) in [
            ("0", "10"),
            ("5", "1"),
            ("0", max.as_str()),
            ("1", max.as_str()),
            ("-3", "-3"),
        ] {
            let lit =
                format!("strake 1\n\nproperty p\n  forall n: i64 in {lo}..{hi}\n  end\nend\n");
            let named = format!(
                "strake 1\n\nlo = {lo}\nhi = {hi}\n\nproperty p\n  forall n: i64 in lo..hi\n  end\nend\n"
            );
            assert_eq!(t4_check_codes(&lit), t4_check_codes(&named), "{lo}..{hi}");
        }
    }

    #[test]
    fn t4_module_list_name_as_lo_is_forall() {
        let src =
            "strake 1\n\nxs = list.empty i64\n\nproperty p\n  forall n: i64 in xs..3\n  end\nend\n";
        assert_eq!(t4_check_codes(src), vec![Code::EForall]);
    }

    #[test]
    fn t4_module_list_name_as_hi_is_forall() {
        let src =
            "strake 1\n\nxs = list.empty i64\n\nproperty p\n  forall n: i64 in 0..xs\n  end\nend\n";
        assert_eq!(t4_check_codes(src), vec![Code::EForall]);
    }

    #[test]
    fn t4_unknown_name_bound_is_forall() {
        let src = "strake 1\n\nproperty p\n  forall n: i64 in 0..nope\n  end\nend\n";
        assert_eq!(t4_check_codes(src), vec![Code::EForall]);
    }

    #[test]
    fn t5_binop_one_bad_operand_is_e_type() {
        for body in ["x = add i64 1 true", "x = add i64 true 1"] {
            let src = format!("strake 1\n\ntest t\n  {body}\n  ret 0\nend\n");
            assert_eq!(t4_check_codes(&src), vec![Code::EType], "{body}");
        }
    }

    #[test]
    fn forall_range_module_binds_check_ok() {
        let src =
            "strake 1\n\nlo = 0\nhi = 1\n\nproperty p\n  forall n: i64 in lo..hi\n  end\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        assert!(check(&m, false).is_ok());
    }

    #[test]
    fn check_fn_for_over_module_list_binds() {
        let src = "strake 1\n\nfn f() -> unit\n  t0 = list.empty i64\n  for x in t0\n    ret unit\n  end\n  ret unit\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        assert!(check(&m, false).is_ok());
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

    #[test]
    fn list_cons_onto_i32_accepts_fitting_literal() {
        let ok = "strake 1\n\nfn f() -> i64\n  e = list.empty i32\n  xs = list.cons 1 e\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_ok(ok));
        let wide = "strake 1\n\nfn f() -> i64\n  e = list.empty i32\n  xs = list.cons 3000000000 e\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let bool_head = "strake 1\n\nfn f() -> i64\n  e = list.empty i32\n  xs = list.cons true e\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_errs(wide));
        assert!(check_errs(bool_head));
    }

    #[test]
    fn ordered_cmp_on_list_record_sum_result_bytes_is_etype() {
        let list = "strake 1\n\nfn f() -> i64\n  e = list.empty i64\n  c = lt list i64 e e\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let rec = "strake 1\n\ntype Point { x: i64, y: i64 }\n\nfn f(a: Point, b: Point) -> i64\n  c = le Point a b\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let sum = "strake 1\n\ntype Color { Blue unit | Red i64 }\n\nfn f(a: Color, b: Color) -> i64\n  c = gt Color a b\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let result = "strake 1\n\nfn f(a: result i64 unit, b: result i64 unit) -> i64\n  c = ge result i64 unit a b\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let bytes = "strake 1\n\nfn f(a: bytes, b: bytes) -> i64\n  c = lt bytes a b\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_errs(list));
        assert!(check_errs(rec));
        assert!(check_errs(sum));
        assert!(check_errs(result));
        assert!(check_errs(bytes));
        let eq_bytes = "strake 1\n\nfn f(a: bytes, b: bytes) -> i64\n  c = eq bytes a b\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let lt_i64 =
            "strake 1\n\nfn f() -> i64\n  c = lt i64 1 2\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_ok(eq_bytes));
        assert!(check_ok(lt_i64));
    }

    #[test]
    fn eq_unit_ne_unit_and_lt_bool_are_etype() {
        let eq_unit = "strake 1\n\nfn f() -> i64\n  c = eq unit unit unit\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let ne_unit = "strake 1\n\nfn f() -> i64\n  c = ne unit unit unit\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let lt_bool = "strake 1\n\nfn f(a: bool, b: bool) -> i64\n  c = lt bool a b\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_errs(eq_unit));
        assert!(check_errs(ne_unit));
        assert!(check_errs(lt_bool));
        let eq_bool = "strake 1\n\nfn f(a: bool, b: bool) -> i64\n  c = eq bool a b\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let eq_bytes = "strake 1\n\nfn f(a: bytes, b: bytes) -> i64\n  c = eq bytes a b\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_ok(eq_bool));
        assert!(check_ok(eq_bytes));
    }

    #[test]
    fn record_literal_keeps_single_field_type_survivor() {
        let ok = "strake 1\n\ntype A { x: i64 }\n\ntype B { x: bool }\n\nfn f() -> i64\n  p = { x: true }\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_ok(ok));
        let amb = "strake 1\n\ntype A { x: i64 }\n\ntype C { x: i32 }\n\nfn f() -> i64\n  p = { x: 1 }\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let neither = "strake 1\n\ntype A { x: i64 }\n\ntype B { x: bool }\n\nfn f() -> i64\n  p = { x: x\"00\" }\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_errs(amb));
        assert!(check_errs(neither));
    }

    #[test]
    fn for_over_list_and_module_int_bind() {
        let for_src = "strake 1\n\nfn f() -> unit\n  t0 = list.empty i64\n  for x in t0\n    ret unit\n  end\n  ret unit\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_ok(for_src));
        let bind_src = "strake 1\n\nlo = 0\n\nproperty p\n  forall n: i64 in lo..1\n  end\nend\n";
        assert!(check_ok(bind_src));
    }

    #[test]
    fn check_stmt_if_and_assign_paths() {
        let src = "strake 1\n\nfn f(flag: bool) -> i64\n  if flag\n    x = add i64 1 2\n  else\n    x = add i64 3 4\n  end\n  ret x\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_ok(src));
    }

    #[test]
    fn check_stmt_for_and_forall_in_property() {
        let src = "strake 1\n\nproperty p\n  t0 = list.empty i64\n  for x in t0\n    assert.eq x x\n  end\n  forall n: i64 in 0..1\n    assert.eq n n\n  end\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        assert!(check(&m, false).is_ok());
    }

    #[test]
    fn check_rhs_list_and_binop_paths() {
        let src = "strake 1\n\nfn f() -> i64\n  e = list.empty i64\n  xs = list.cons 1 e\n  n = list.len xs\n  s = add i64 n 0\n  ret s\nend\n\ntest t\n  ret 0\nend\n";
        assert!(check_ok(src));
    }

    fn golden_path(name: &str) -> String {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../golden")
            .join(name)
            .to_str()
            .expect("utf8")
            .to_string()
    }

    fn cli_check(name: &str) -> crate::Exit {
        crate::check(&golden_path(name), crate::Opts::default())
    }

    fn cli_test(name: &str) -> crate::Exit {
        crate::test(&golden_path(name), crate::Opts::default())
    }

    fn write_src(name: &str, src: &str) -> String {
        let path = std::env::temp_dir().join(format!("strake-gma497-{name}.strake"));
        std::fs::write(&path, src).expect("write");
        path.to_str().expect("utf8").to_string()
    }

    /// A repeated record name is `E_TYPE` at check. One record still runs.
    #[test]
    fn duplicate_record_name_is_etype() {
        let dup = cli_check("dup_record.strake");
        assert_eq!(dup.code, 1, "{}", dup.stderr);
        assert!(dup.stderr.contains("\"code\":\"E_TYPE\""), "{}", dup.stderr);
        assert!(!dup.stderr.contains("lower:"), "{}", dup.stderr);
        let projected = cli_test("record_project.strake");
        assert_eq!(projected.code, 0, "{}", projected.stderr);
        let last_fits = "strake 1\n\ntype A { n: bool }\n\ntype A { n: i64 }\n\nfn f() -> i64\n  p = { n: 1 }\n  v = p.n\n  ret v\nend\n\ntest t\n  n = call f()\n  assert.eq n 1\n  ret 0\nend\n";
        let path = write_src("dup-last-fits", last_fits);
        let exit = crate::check(&path, crate::Opts::default());
        assert_eq!(exit.code, 1, "{}", exit.stderr);
        assert!(
            exit.stderr.contains("\"code\":\"E_TYPE\""),
            "{}",
            exit.stderr
        );
    }

    /// `unit` matches `unit` on a result side, and no other type.
    #[test]
    fn result_unit_matches_only_unit() {
        let mixed = cli_check("result_unit_join.strake");
        assert_eq!(mixed.code, 1, "{}", mixed.stderr);
        assert!(
            mixed.stderr.contains("\"code\":\"E_TYPE\""),
            "{}",
            mixed.stderr
        );
        let same = cli_test("result_i64_join.strake");
        assert_eq!(same.code, 0, "{}", same.stderr);
        let unit_unit = "strake 1\n\nfn f(c: bool) -> unit\n  if c\n    r = ok unit unit\n  else\n    r = ok unit unit\n  end\n  match r\n    ok v ->\n      ret v\n    err _ ->\n      ret unit\n  end\nend\n\ntest t\n  n = call f(true)\n  ret 0\nend\n";
        let ok_path = write_src("result-unit-unit", unit_unit);
        let ok = crate::test(&ok_path, crate::Opts::default());
        assert_eq!(ok.code, 0, "{}", ok.stderr);
        let err_sides = "strake 1\n\nfn f(c: bool) -> i64\n  if c\n    r = err i64 1\n  else\n    r = err unit unit\n  end\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let err_path = write_src("result-err-unit", err_sides);
        let err = crate::check(&err_path, crate::Opts::default());
        assert_eq!(err.code, 1, "{}", err.stderr);
        assert!(err.stderr.contains("\"code\":\"E_TYPE\""), "{}", err.stderr);
        let swapped = "strake 1\n\nfn f(c: bool) -> i64\n  if c\n    r = ok unit unit\n  else\n    r = ok i64 4\n  end\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let swapped_path = write_src("result-unit-swapped", swapped);
        let swapped_exit = crate::check(&swapped_path, crate::Opts::default());
        assert_eq!(swapped_exit.code, 1, "{}", swapped_exit.stderr);
        let ctor = cli_test("match_result.strake");
        assert_eq!(ctor.code, 0, "{}", ctor.stderr);
        let wrong_ok = "strake 1\n\nfn f(r: result bool unit) -> i64\n  ret 0\nend\n\ntest t\n  v = ok i64 1\n  n = call f(v)\n  ret 0\nend\n";
        let wrong_path = write_src("result-ok-bool", wrong_ok);
        let wrong = crate::check(&wrong_path, crate::Opts::default());
        assert_eq!(wrong.code, 1, "{}", wrong.stderr);
        assert!(
            wrong.stderr.contains("\"code\":\"E_TYPE\""),
            "{}",
            wrong.stderr
        );
    }

    /// An assign joined with a loop exit uses the assign's type, not an i64 hint.
    #[test]
    fn loop_exit_join_rejects_record_and_bytes() {
        let record = cli_check("loop_exit_record.strake");
        assert_eq!(record.code, 1, "{}", record.stderr);
        assert!(
            record.stderr.contains("\"code\":\"E_TYPE\""),
            "{}",
            record.stderr
        );
        let bytes = cli_check("loop_exit_bytes.strake");
        assert_eq!(bytes.code, 1, "{}", bytes.stderr);
        assert!(
            bytes.stderr.contains("\"code\":\"E_TYPE\""),
            "{}",
            bytes.stderr
        );
        let joined = cli_test("loop_arm_join_true.strake");
        assert_eq!(joined.code, 0, "{}", joined.stderr);
        let list = "strake 1\n\nfn f(c: bool) -> i64\n  if c\n    e = list.empty i64\n    r = list.cons 1 e\n  else\n    loop r = 0\n      break\n    end\n  end\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let path = write_src("loop-exit-list", list);
        let exit = crate::check(&path, crate::Opts::default());
        assert_eq!(exit.code, 1, "{}", exit.stderr);
        assert!(
            exit.stderr.contains("\"code\":\"E_TYPE\""),
            "{}",
            exit.stderr
        );
    }

    /// Match arms that deliver one name must agree on its type.
    #[test]
    fn match_arm_types_must_agree() {
        let records = cli_check("match_arm_records.strake");
        assert_eq!(records.code, 1, "{}", records.stderr);
        assert!(
            records.stderr.contains("\"code\":\"E_TYPE\""),
            "{}",
            records.stderr
        );
        let project = cli_check("match_arm_project.strake");
        assert_eq!(project.code, 1, "{}", project.stderr);
        assert!(
            project.stderr.contains("\"code\":\"E_TYPE\""),
            "{}",
            project.stderr
        );
        assert!(!project.stderr.contains("lower:"), "{}", project.stderr);
        let agreed = cli_test("match_arm_agree.strake");
        assert_eq!(agreed.code, 0, "{}", agreed.stderr);
        let trap = "strake 1\n\ntype A { x: i64, y: i64 }\n\ntype B { y: i64, z: i64 }\n\ntype S { A unit | B unit | C unit }\n\nfn f(s: S) -> i64\n  match s\n    A _ ->\n      p = { x: 1, y: 2 }\n    B _ ->\n      p = { x: 3, y: 4 }\n    C _ ->\n      p = { y: 9, z: 8 }\n      trap\n  end\n  v = p.x\n  ret v\nend\n\ntest t\n  ret 0\nend\n";
        let path = write_src("match-arm-trap", trap);
        let exit = crate::check(&path, crate::Opts::default());
        assert_eq!(exit.code, 0, "{}", exit.stderr);
    }
}
