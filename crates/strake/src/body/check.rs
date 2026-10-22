//! Type and binding checks; produces a checked sketch for lowering.

use super::diag::{Code, Diag, Kind, Span};
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
    for stmt in &f.body {
        check_stmt(stmt, &mut env, fns, true, diags);
    }
    check_ret_type(f, &env, fns, diags);
}

fn check_test(t: &TestItem, fns: &[&FnItem], diags: &mut Vec<Diag>) {
    let mut env: Vec<(String, Type)> = Vec::new();
    for stmt in &t.body {
        check_stmt(stmt, &mut env, fns, false, diags);
    }
}

fn check_ret_type(f: &FnItem, env: &[(String, Type)], fns: &[&FnItem], diags: &mut Vec<Diag>) {
    for stmt in &f.body {
        if let Stmt::Ret { value, span } = stmt {
            let got = operand_ty(value, env, fns, None, diags);
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
    in_fn: bool,
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
        Stmt::Ret { value, .. } => {
            if !in_fn {
                operand_ty(value, env, fns, None, diags);
            }
        }
        Stmt::AssertEq { span, got, want } => {
            if in_fn {
                diags.push(parse_diag(span.clone()));
                return;
            }
            let g = operand_ty(got, env, fns, None, diags);
            let w = operand_ty(want, env, fns, Some(&g), diags);
            if !types_compatible(&g, &w) {
                diags.push(type_diag(span.clone()));
            }
        }
        _ => {}
    }
}

fn check_rhs(rhs: &Rhs, env: &[(String, Type)], fns: &[&FnItem], diags: &mut Vec<Diag>) -> Type {
    match rhs {
        Rhs::BinOp {
            ty, lhs, rhs, span, ..
        } => {
            let lt = operand_ty(lhs, env, fns, Some(ty), diags);
            let rt = operand_ty(rhs, env, fns, Some(ty), diags);
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
                let at = operand_ty(arg, env, fns, Some(param_ty), diags);
                if !types_compatible(&at, param_ty) {
                    diags.push(type_diag(span.clone()));
                }
            }
            ret
        }
        Rhs::Operand(op) => operand_ty(op, env, fns, None, diags),
        _ => scalar(
            "i64",
            Span {
                file: String::new(),
                start: 0,
                end: 0,
            },
        ),
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
    expected: Option<&Type>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::syntax;

    #[test]
    fn call_i32_param_accepts_small_literal() {
        let src = "strake 1\n\nfn id32(a: i32) -> i32\n  ret a\nend\n\ntest t\n  r = call id32(3)\n  ret 0\nend\n";
        let m = syntax::parse(src, "t.strake").expect("parse");
        assert!(check(&m).is_ok());
    }
}
