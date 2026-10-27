//! Type and binding checks; produces a checked sketch for lowering.

#![allow(clippy::too_many_arguments)]

use std::collections::{HashMap, HashSet};

use super::diag::{Code, Diag, Kind, Span};
use super::syntax::{
    BinOp, FnItem, ForallBound, Item, MatchArm, Module, Operand, PropertyItem, Rhs, StaticI64,
    Stmt, TestItem, Type,
};

/// Checked module ready for lowering.
#[derive(Clone, Debug, PartialEq)]
pub struct Sketch {
    pub module: Module,
}

pub(crate) fn check(module: &Module, draft: bool) -> Result<Sketch, Vec<Diag>> {
    let mut diags = Vec::new();
    let fn_table: Vec<&FnItem> = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(f) => Some(f),
            _ => None,
        })
        .collect();
    let module_int_binds = module_int_bindings(module);
    for item in &module.items {
        match item {
            Item::Fn(f) => check_fn(f, &fn_table, &module_int_binds, &mut diags),
            Item::Test(t) => check_test(t, &fn_table, &module_int_binds, &mut diags),
            Item::Property(p) => check_property(p, &fn_table, &module_int_binds, &mut diags),
            _ => {}
        }
    }
    if !draft {
        collect_holes_module(module, &mut diags);
    }
    if diags.is_empty() {
        Ok(Sketch {
            module: module.clone(),
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

fn module_int_bindings(module: &Module) -> HashMap<String, i128> {
    let mut out = HashMap::new();
    for item in &module.items {
        if let Item::Bind { name, value, .. } = item {
            out.insert(name.clone(), *value);
        }
    }
    out
}

fn apply_module_binds(ctx: &mut Ctx, module_int_binds: &HashMap<String, i128>, span: Span) {
    for (name, value) in module_int_binds {
        let ty = scalar("i64", span.clone());
        if !ctx.types.contains_key(name) {
            ctx.types.insert(name.clone(), ty);
        }
        let _ = value;
    }
}

fn check_fn(
    f: &FnItem,
    fns: &[&FnItem],
    module_int_binds: &HashMap<String, i128>,
    diags: &mut Vec<Diag>,
) {
    let mut ctx = Ctx::new();
    apply_module_binds(&mut ctx, module_int_binds, f.span.clone());
    for p in &f.params {
        ctx.bind(&p.name, p.ty.clone(), p.span.clone(), diags);
    }
    for stmt in &f.body {
        check_stmt(
            stmt,
            &mut ctx,
            fns,
            module_int_binds,
            true,
            Some(&f.ret),
            diags,
        );
    }
}

fn check_test(
    t: &TestItem,
    fns: &[&FnItem],
    module_int_binds: &HashMap<String, i128>,
    diags: &mut Vec<Diag>,
) {
    let mut ctx = Ctx::new();
    apply_module_binds(&mut ctx, module_int_binds, t.span.clone());
    for stmt in &t.body {
        check_stmt(stmt, &mut ctx, fns, module_int_binds, false, None, diags);
    }
}

fn check_property(
    p: &PropertyItem,
    fns: &[&FnItem],
    module_int_binds: &HashMap<String, i128>,
    diags: &mut Vec<Diag>,
) {
    let mut ctx = Ctx::new();
    apply_module_binds(&mut ctx, module_int_binds, p.span.clone());
    for stmt in &p.body {
        check_stmt_property(stmt, &mut ctx, fns, module_int_binds, diags);
    }
}

fn check_stmt_property(
    stmt: &Stmt,
    ctx: &mut Ctx,
    fns: &[&FnItem],
    module_int_binds: &HashMap<String, i128>,
    diags: &mut Vec<Diag>,
) {
    match stmt {
        Stmt::Forall {
            span,
            binder,
            bound,
            body,
        } => {
            check_forall_bound(bound, module_int_binds, diags);
            let ty = forall_binder_ty(bound);
            ctx.bind(binder, ty, span.clone(), diags);
            for s in body {
                check_stmt_forall(
                    s,
                    &mut *ctx,
                    binder,
                    fns,
                    module_int_binds,
                    false,
                    None,
                    diags,
                );
            }
        }
        _ => check_stmt(stmt, ctx, fns, module_int_binds, false, None, diags),
    }
}

fn check_stmt(
    stmt: &Stmt,
    ctx: &mut Ctx,
    fns: &[&FnItem],
    module_int_binds: &HashMap<String, i128>,
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
            cond,
            then_body,
            else_body,
            ..
        } => {
            operand_ty(cond, &ctx.types, &ctx.phi_gaps, fns, None, diags);
            check_if(
                then_body,
                else_body,
                ctx,
                fns,
                module_int_binds,
                in_fn,
                fn_ret,
                diags,
            );
        }
        Stmt::For {
            span,
            binder,
            list,
            body,
        } => {
            let list_ty = ctx.types.get(list).cloned();
            match list_ty {
                Some(Type::List(elem, _)) => {
                    ctx.bind(binder, (*elem).clone(), span.clone(), diags);
                }
                _ => {
                    diags.push(type_diag(span.clone()));
                    ctx.bind(binder, scalar("i64", span.clone()), span.clone(), diags);
                }
            }
            for s in body {
                check_stmt(s, ctx, fns, module_int_binds, in_fn, fn_ret, diags);
            }
        }
        Stmt::Loop { params, body, .. } => {
            let mut loop_ctx = Ctx {
                types: ctx.types.clone(),
                phi_gaps: ctx.phi_gaps.clone(),
            };
            let param_names: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
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
                    &param_names,
                    fns,
                    module_int_binds,
                    in_fn,
                    fn_ret,
                    diags,
                );
            }
        }
        Stmt::Break { .. } | Stmt::Trap { .. } => {}
        Stmt::Match {
            span,
            scrutinee,
            arms,
        } => {
            let scr_ty = ctx.types.get(scrutinee).cloned().unwrap_or_else(|| {
                diags.push(type_diag(span.clone()));
                scalar("i64", span.clone())
            });
            check_match_exhaustive(&scr_ty, arms, span.clone(), diags);
            check_match_join(arms, ctx, fns, module_int_binds, in_fn, fn_ret, diags);
        }
        Stmt::Forall {
            span,
            binder,
            bound,
            body,
        } => {
            check_forall_bound(bound, module_int_binds, diags);
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
                    &forall_name,
                    fns,
                    module_int_binds,
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

fn check_stmt_loop(
    stmt: &Stmt,
    ctx: &mut Ctx,
    loop_params: &HashSet<String>,
    fns: &[&FnItem],
    module_int_binds: &HashMap<String, i128>,
    in_fn: bool,
    fn_ret: Option<&Type>,
    diags: &mut Vec<Diag>,
) {
    if let Stmt::Continue { span, edges } = stmt {
        let mut seen = HashSet::new();
        for e in edges {
            if !loop_params.contains(&e.name) {
                diags.push(type_diag(span.clone()));
            }
            if !seen.insert(e.name.clone()) {
                diags.push(type_diag(span.clone()));
            }
            operand_ty(&e.value, &ctx.types, &ctx.phi_gaps, fns, None, diags);
        }
        if edges.len() != loop_params.len() {
            diags.push(type_diag(span.clone()));
        }
        return;
    }
    if let Stmt::Assign { span, name, .. } = stmt {
        if loop_params.contains(name) {
            diags.push(shadow_diag(span.clone()));
        }
    }
    check_stmt(stmt, ctx, fns, module_int_binds, in_fn, fn_ret, diags);
}

fn check_stmt_forall(
    stmt: &Stmt,
    ctx: &mut Ctx,
    binder: &str,
    fns: &[&FnItem],
    module_int_binds: &HashMap<String, i128>,
    in_fn: bool,
    fn_ret: Option<&Type>,
    diags: &mut Vec<Diag>,
) {
    if let Stmt::Assign { span, name, .. } = stmt {
        if name == binder {
            diags.push(shadow_diag(span.clone()));
        }
    }
    check_stmt(stmt, ctx, fns, module_int_binds, in_fn, fn_ret, diags);
}

fn check_if(
    then_body: &[Stmt],
    else_body: &[Stmt],
    ctx: &mut Ctx,
    fns: &[&FnItem],
    module_int_binds: &HashMap<String, i128>,
    in_fn: bool,
    fn_ret: Option<&Type>,
    diags: &mut Vec<Diag>,
) {
    let pre = ctx.types.clone();
    let then_tops = top_level_assigns(then_body);
    let else_tops = top_level_assigns(else_body);

    let mut then_ctx = Ctx {
        types: pre.clone(),
        phi_gaps: ctx.phi_gaps.clone(),
    };
    for s in then_body {
        check_stmt_in_arm(
            s,
            &mut then_ctx,
            &pre,
            fns,
            module_int_binds,
            in_fn,
            fn_ret,
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
            fns,
            module_int_binds,
            in_fn,
            fn_ret,
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
        } else {
            ctx.phi_gaps.insert(name.clone());
        }
    }
    for name in else_tops.keys() {
        if !then_tops.contains_key(name) && !pre.contains_key(name) {
            ctx.phi_gaps.insert(name.clone());
        }
    }
}

fn check_stmt_in_arm(
    stmt: &Stmt,
    ctx: &mut Ctx,
    pre: &HashMap<String, Type>,
    fns: &[&FnItem],
    module_int_binds: &HashMap<String, i128>,
    in_fn: bool,
    fn_ret: Option<&Type>,
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
            cond,
            then_body,
            else_body,
            ..
        } => {
            operand_ty(cond, &ctx.types, &ctx.phi_gaps, fns, None, diags);
            check_if(
                then_body,
                else_body,
                ctx,
                fns,
                module_int_binds,
                in_fn,
                fn_ret,
                diags,
            );
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
                check_stmt_in_arm(s, ctx, pre, fns, module_int_binds, in_fn, fn_ret, diags);
            }
        }
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

fn check_forall_bound(
    bound: &ForallBound,
    module_int_binds: &HashMap<String, i128>,
    diags: &mut Vec<Diag>,
) {
    if let ForallBound::Range { lo, hi, span, .. } = bound {
        let lo_v = resolve_static_i64(lo, module_int_binds, diags);
        let hi_v = resolve_static_i64(hi, module_int_binds, diags);
        if let (Some(lo_v), Some(hi_v)) = (lo_v, hi_v) {
            if !fits_i64(lo_v) || !fits_i64(hi_v) {
                diags.push(forall_diag(span.clone()));
                return;
            }
            let span128 = hi_v - lo_v + 1;
            if hi_v < lo_v || span128 <= 0 || span128 > i64::MAX as i128 {
                diags.push(forall_diag(span.clone()));
            }
        } else {
            diags.push(forall_diag(span.clone()));
        }
    }
}

fn check_match_exhaustive(scr_ty: &Type, arms: &[MatchArm], span: Span, diags: &mut Vec<Diag>) {
    if let Type::Result(_, _, _) = scr_ty {
        let has_ok = arms.iter().any(|a| a.tag == "ok");
        let has_err = arms.iter().any(|a| a.tag == "err");
        if !has_ok || !has_err {
            diags.push(match_diag(span));
        }
    }
}

fn check_match_join(
    arms: &[MatchArm],
    ctx: &mut Ctx,
    fns: &[&FnItem],
    module_int_binds: &HashMap<String, i128>,
    in_fn: bool,
    fn_ret: Option<&Type>,
    diags: &mut Vec<Diag>,
) {
    let pre = ctx.types.clone();
    let arm_tops: Vec<HashMap<String, (Type, Span)>> =
        arms.iter().map(|a| top_level_assigns(&a.body)).collect();
    let mut all_join = HashSet::new();
    for tops in &arm_tops {
        all_join.extend(tops.keys().cloned());
    }
    for join in all_join {
        let in_any = arm_tops.iter().filter(|t| t.contains_key(&join)).count();
        if in_any > 0 && in_any < arms.len() {
            continue;
        }
        if in_any == arms.len() && !pre.contains_key(&join) {
            let ty = arm_tops[0]
                .get(&join)
                .map(|(t, _)| t.clone())
                .unwrap_or_else(|| scalar("i64", empty_span()));
            if ctx.types.contains_key(&join) {
                diags.push(shadow_diag(
                    arm_tops[0]
                        .get(&join)
                        .map(|(_, s)| s.clone())
                        .unwrap_or_else(empty_span),
                ));
            } else {
                ctx.types.insert(join.clone(), ty);
            }
        }
    }
    for arm in arms {
        let mut arm_ctx = Ctx {
            types: pre.clone(),
            phi_gaps: ctx.phi_gaps.clone(),
        };
        if let Some(binder) = &arm.binder {
            if binder != "_" {
                let payload_ty = match arm.tag.as_str() {
                    "ok" | "err" => scalar("i64", arm.span.clone()),
                    _ => scalar("i64", arm.span.clone()),
                };
                arm_ctx.bind(binder, payload_ty, arm.span.clone(), diags);
            }
        }
        for s in &arm.body {
            check_stmt_in_arm(
                s,
                &mut arm_ctx,
                &pre,
                fns,
                module_int_binds,
                in_fn,
                fn_ret,
                diags,
            );
        }
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
        Rhs::Ok { ty, value, span } => {
            operand_ty(value, env, &gaps, fns, Some(ty), diags);
            Type::Result(
                Box::new(ty.clone()),
                Box::new(scalar("unit", span.clone())),
                span.clone(),
            )
        }
        Rhs::Err { ty, value, span } => {
            operand_ty(value, env, &gaps, fns, Some(ty), diags);
            Type::Result(
                Box::new(scalar("unit", span.clone())),
                Box::new(ty.clone()),
                span.clone(),
            )
        }
        Rhs::ListEmpty { elem, span } => Type::List(Box::new(elem.clone()), span.clone()),
        Rhs::ListCons { head, tail, span } => {
            let ht = operand_ty(head, env, &gaps, fns, None, diags);
            let tt = operand_ty(tail, env, &gaps, fns, None, diags);
            match tt {
                Type::List(elem, _) if types_compatible(&ht, &elem) => {
                    Type::List(elem, span.clone())
                }
                _ => {
                    diags.push(type_diag(span.clone()));
                    Type::List(Box::new(ht), span.clone())
                }
            }
        }
        Rhs::ListLen { list, span } => {
            operand_ty(list, env, &gaps, fns, None, diags);
            scalar("i64", span.clone())
        }
        Rhs::ListGet { list, index, span } => {
            let lt = operand_ty(list, env, &gaps, fns, None, diags);
            operand_ty(
                index,
                env,
                &gaps,
                fns,
                Some(&scalar("i64", span.clone())),
                diags,
            );
            let elem = match lt {
                Type::List(e, _) => e.clone(),
                _ => {
                    diags.push(type_diag(span.clone()));
                    Box::new(scalar("i64", span.clone()))
                }
            };
            Type::Result(elem, Box::new(scalar("unit", span.clone())), span.clone())
        }
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

fn forall_diag(span: Span) -> Diag {
    Diag {
        code: Code::EForall,
        kind: Kind::Check,
        span,
        want: None,
        got: None,
        patch: None,
    }
}

fn match_diag(span: Span) -> Diag {
    Diag {
        code: Code::EMatch,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::syntax;

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
    fn forall_lo0_hi_max_is_forall() {
        let src = "strake 1\n\nproperty p\n  forall n: i64 in 0..9223372036854775807\n  end\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        let err = check(&m, false).unwrap_err();
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
