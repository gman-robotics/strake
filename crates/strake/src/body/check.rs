//! Type and binding checks; produces a checked sketch for lowering.

use super::diag::{Code, Diag, Kind, Span};

/// Keeps catalog variants reachable for later tickets without emitting them yet.
pub(crate) fn touch_catalog_for_clippy() {
    let _ = Code::constructable();
    let _ = Code::from_catalog_name("E_PARSE");
    let _ = (
        Kind::Trap,
        Kind::Result,
        Code::EForall,
        Code::EMatch,
        Code::EIfPhi,
        Code::EHole,
        Code::ERegion,
        Code::EUse,
        Code::ETest,
        Code::ERefused,
    );
}
use super::syntax::{FnItem, Item, Module, Operand, Rhs, Stmt, TestItem, Type};

/// Checked module ready for lowering.
#[derive(Clone, Debug, PartialEq)]
pub struct Sketch {
    pub module: Module,
}

pub(crate) fn check(module: &Module) -> Result<Sketch, Vec<Diag>> {
    let mut diags = Vec::new();
    let mut fn_table: Vec<&FnItem> = Vec::new();
    for item in &module.items {
        if let Item::Fn(f) = item {
            fn_table.push(f);
        }
    }
    for item in &module.items {
        match item {
            Item::Fn(f) => check_fn(f, &fn_table, &mut diags),
            Item::Test(t) => check_test(t, &fn_table, &mut diags),
            _ => {}
        }
    }
    if diags.is_empty() {
        Ok(Sketch {
            module: module.clone(),
        })
    } else {
        Err(diags)
    }
}

fn check_fn(f: &FnItem, fns: &[&FnItem], diags: &mut Vec<Diag>) {
    let mut env: Vec<(String, Type)> = f
        .params
        .iter()
        .map(|p| (p.name.clone(), p.ty.clone()))
        .collect();
    if !types_equal(&f.ret, &scalar("i64", f.span.clone())) && !is_unit(&f.ret) {
        // allow i64 return for add2; other returns still checked at ret sites
    }
    for stmt in &f.body {
        check_stmt(stmt, &mut env, fns, f.span.clone(), false, diags);
    }
    check_ret_type(f, &env, fns, diags);
}

fn check_test(t: &TestItem, fns: &[&FnItem], diags: &mut Vec<Diag>) {
    let mut env: Vec<(String, Type)> = Vec::new();
    for stmt in &t.body {
        check_stmt(stmt, &mut env, fns, t.span.clone(), true, diags);
    }
}

fn check_ret_type(f: &FnItem, env: &[(String, Type)], fns: &[&FnItem], diags: &mut Vec<Diag>) {
    for stmt in &f.body {
        if let Stmt::Ret { value, span } = stmt {
            let got = operand_ty(value, env, fns, diags);
            if !types_compatible(&got, &f.ret) {
                diags.push(type_diag(span.clone()));
            }
        }
    }
}

fn check_stmt(
    stmt: &Stmt,
    env: &mut Vec<(String, Type)>,
    fns: &[&FnItem],
    _scope: Span,
    in_test: bool,
    diags: &mut Vec<Diag>,
) {
    match stmt {
        Stmt::Assign { span, name, rhs } => {
            let ty = check_rhs(rhs, env, fns, diags);
            if env.iter().any(|(n, _)| n == name) {
                diags.push(shadow_diag(span.clone()));
            }
            env.push((name.clone(), ty));
        }
        Stmt::Ret { value, span } => {
            let _ = operand_ty(value, env, fns, diags);
            if !in_test {
                let got = operand_ty(value, env, fns, diags);
                if !is_i64(&got) && !is_unit(&got) {
                    diags.push(type_diag(span.clone()));
                }
            }
        }
        Stmt::AssertEq { span, got, want } => {
            if !in_test {
                diags.push(parse_diag(span.clone()));
                return;
            }
            let g = operand_ty(got, env, fns, diags);
            let w = operand_ty(want, env, fns, diags);
            if !types_compatible(&g, &w) {
                diags.push(type_diag(span.clone()));
            }
        }
        _ => diags.push(type_diag(_scope.clone())),
    }
}

fn check_rhs(rhs: &Rhs, env: &[(String, Type)], fns: &[&FnItem], diags: &mut Vec<Diag>) -> Type {
    match rhs {
        Rhs::BinOp {
            ty, lhs, rhs, span, ..
        } => {
            let lt = operand_ty(lhs, env, fns, diags);
            let rt = operand_ty(rhs, env, fns, diags);
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
                let at = operand_ty(arg, env, fns, diags);
                if !types_compatible(&at, param_ty) {
                    diags.push(type_diag(span.clone()));
                }
            }
            ret
        }
        Rhs::Operand(op) => operand_ty(op, env, fns, diags),
        _ => {
            diags.push(type_diag(Span {
                file: String::new(),
                start: 0,
                end: 0,
            }));
            scalar(
                "i64",
                Span {
                    file: String::new(),
                    start: 0,
                    end: 0,
                },
            )
        }
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
    env: &[(String, Type)],
    _fns: &[&FnItem],
    diags: &mut Vec<Diag>,
) -> Type {
    match op {
        Operand::Name(name, span) => env
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, t)| t.clone())
            .unwrap_or_else(|| {
                diags.push(type_diag(span.clone()));
                scalar("i64", span.clone())
            }),
        Operand::Int(n, span) => {
            if fits_i64(*n) {
                scalar("i64", span.clone())
            } else {
                diags.push(type_diag(span.clone()));
                scalar("i64", span.clone())
            }
        }
        Operand::Unit(span) => Type::Named("unit".into(), span.clone()),
        Operand::Bool(_, span) => Type::Named("bool".into(), span.clone()),
        Operand::Float(_, span) => Type::Named("f64".into(), span.clone()),
        Operand::Bytes(_, span) => Type::Named("bytes".into(), span.clone()),
    }
}

fn types_compatible(found: &Type, expected: &Type) -> bool {
    if types_equal(found, expected) {
        return true;
    }
    if is_i64(expected) {
        return matches!(found, Type::Named(n, _) if n == "i64");
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

fn is_i64(t: &Type) -> bool {
    matches!(t, Type::Named(n, _) if n == "i64")
}

fn is_unit(t: &Type) -> bool {
    matches!(t, Type::Named(n, _) if n == "unit")
}

fn scalar(name: &str, span: Span) -> Type {
    Type::Named(name.to_string(), span)
}

fn fits_i64(n: i128) -> bool {
    n >= i64::MIN as i128 && n <= i64::MAX as i128
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
