//! If check accepts a program, lowering must finish and SSA verify must pass.

use std::fs;
use std::path::PathBuf;

use super::check::{self, CheckFailure};
use super::compile_module;
use super::diag::Code;
use super::host::EmptyHost;
use super::interp::{self, ir::verify_function_ssa_public};
use super::lower;
use super::syntax;

/// Check-pass fixtures whose lower abort is locked (exit 70).
const LOWER_ABORT_GOLDENS: &[&str] = &["add2_worked.strake", "if_join_s5_waiver.strake"];

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../golden")
}

fn assert_verified(file: &str, src: &str) {
    let module = syntax::parse(src, file).unwrap_or_else(|_| panic!("{file}: parse"));
    let sketch = check::check(&module, false).unwrap_or_else(|_| panic!("{file}: check"));
    let program = lower::lower(&sketch).unwrap_or_else(|e| panic!("{file}: {}", e.message()));
    for func in &program.functions {
        verify_function_ssa_public(func)
            .unwrap_or_else(|e| panic!("{file} {}: {}", func.name, e.message()));
    }
}

fn assert_check_implies_verified(file: &str, src: &str) {
    let module = syntax::parse(src, file).unwrap_or_else(|_| panic!("{file}: parse"));
    let Ok(sketch) = check::check(&module, false) else {
        return;
    };
    let program = lower::lower(&sketch).unwrap_or_else(|e| panic!("{file}: {}", e.message()));
    for func in &program.functions {
        verify_function_ssa_public(func)
            .unwrap_or_else(|e| panic!("{file} {}: {}", func.name, e.message()));
    }
}

fn assert_etype(file: &str, src: &str) {
    let module = syntax::parse(src, file).unwrap_or_else(|_| panic!("{file}: parse"));
    match check::check(&module, false) {
        Err(CheckFailure::Diags(diags)) => {
            assert!(diags.iter().any(|d| d.code == Code::EType), "{file}");
        }
        Ok(_) => panic!("{file}: check passed"),
        Err(CheckFailure::Codeless(msg)) => panic!("{file}: codeless {msg}"),
    }
}

fn compile_run(src: &str) -> i64 {
    let module = syntax::parse(src, "t.strake").expect("parse");
    let program = compile_module(&module, false).expect("compile");
    interp::call_function_i64(&program, "test.t", &[], &EmptyHost).expect("exec")
}

fn cmp_i64(op: &str, lhs: &str, rhs: &str, expect: &str) -> String {
    format!(
        "strake 1\n\ntest t\n  c = {op} i64 {lhs} {rhs}\n  assert.eq c {expect}\n  ret 0\nend\n"
    )
}

fn cmp_bytes(op: &str, left: &str, right: &str, expect: &str) -> String {
    format!(
        "strake 1\n\ntest t\n  a = bytes.new x\"{left}\"\n  b = bytes.new x\"{right}\"\n  c = {op} bytes a b\n  assert.eq c {expect}\n  ret 0\nend\n"
    )
}

fn pass_probes() -> Vec<(String, String)> {
    let mut out = vec![
        ("eq-bytes-true".into(), cmp_bytes("eq", "ff00", "ff00", "true")),
        (
            "eq-bytes-false".into(),
            cmp_bytes("eq", "ff00", "00ff", "false"),
        ),
        (
            "ne-bytes-true".into(),
            cmp_bytes("ne", "ff00", "00ff", "true"),
        ),
        (
            "copy-of-compare".into(),
            "strake 1\n\ntest t\n  a = bytes.new x\"ff00\"\n  b = bytes.new x\"ff00\"\n  c = eq bytes a b\n  d = c\n  assert.eq d true\n  ret 0\nend\n".into(),
        ),
        (
            "call-bool".into(),
            "strake 1\n\nfn pred() -> bool\n  r = eq i64 1 1\n  ret r\nend\n\ntest t\n  c = call pred()\n  assert.eq c true\n  ret 0\nend\n".into(),
        ),
        (
            "if-join-eq-i64".into(),
            "strake 1\n\ntest t\n  c = true\n  if c\n    b = eq i64 1 1\n  else\n    b = eq i64 2 3\n  end\n  assert.eq b true\n  ret 0\nend\n".into(),
        ),
        (
            "same-record-if".into(),
            "strake 1\n\ntype Point { x: i64, y: i64 }\n\nfn f(c: bool) -> i64\n  if c\n    p = { x: 1, y: 2 }\n  else\n    p = { x: 3, y: 4 }\n  end\n  v = p.x\n  ret v\nend\n\ntest t\n  n = call f(true)\n  assert.eq n 1\n  ret 0\nend\n".into(),
        ),
    ];
    for (op, lhs, rhs, expect) in [
        ("eq", "1", "1", "true"),
        ("eq", "1", "2", "false"),
        ("ne", "1", "2", "true"),
        ("ne", "1", "1", "false"),
    ] {
        out.push((
            format!("cmp-i64-{op}-{lhs}-{rhs}"),
            cmp_i64(op, lhs, rhs, expect),
        ));
    }
    out
}

fn etype_probes() -> Vec<(&'static str, String)> {
    vec![
        (
            "mismatch-records",
            "strake 1\n\ntype A { x: i64, y: i64 }\n\ntype B { x: bool, y: i64 }\n\nfn f(c: bool) -> i64\n  if c\n    r = { x: 1, y: 2 }\n  else\n    r = { x: true, y: 2 }\n  end\n  ret r\nend\n\ntest t\n  ret 0\nend\n".into(),
        ),
        (
            "mismatch-i64-bool",
            "strake 1\n\nfn f(c: bool) -> i64\n  if c\n    n = add i64 1 2\n  else\n    n = true\n  end\n  ret n\nend\n\ntest t\n  ret 0\nend\n".into(),
        ),
    ]
}

#[test]
fn golden_check_pass_lowers_and_verifies_ssa() {
    let dir = golden_dir();
    let mut names: Vec<String> = fs::read_dir(&dir)
        .expect("golden dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "strake"))
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(!names.is_empty());
    for name in names {
        if LOWER_ABORT_GOLDENS.contains(&name.as_str()) {
            continue;
        }
        let src = fs::read_to_string(dir.join(&name)).expect("read golden");
        assert_check_implies_verified(&name, &src);
    }
}

#[test]
fn probes_check_pass_lowers_and_verifies_ssa() {
    for (name, src) in pass_probes() {
        assert_verified(&name, &src);
    }
    for (name, src) in etype_probes() {
        assert_etype(name, &src);
    }
}

#[test]
fn assert_eq_compare_repros_run() {
    for (name, src) in pass_probes() {
        assert_eq!(compile_run(&src), 0, "{name}");
    }
}
