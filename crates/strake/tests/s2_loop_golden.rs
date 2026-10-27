use std::path::{Path, PathBuf};
use std::process::Command;

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../golden")
        .join(name)
}

fn check_e_type(path: &Path) {
    let (status, _stdout, stderr) = run(&["check", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_TYPE"));
}

fn run(args: &[&str]) -> (std::process::ExitStatus, String, String) {
    let bin = env!("CARGO_BIN_EXE_strake");
    let out = Command::new(bin).args(args).output().expect("run strake");
    (
        out.status,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn loop_l7_exits_0() {
    let path = golden("loop.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn loop_arity_fmt_fixed_point() {
    let path = golden("loop_arity.strake");
    let bin = env!("CARGO_BIN_EXE_strake");
    let on_disk = std::fs::read(&path).expect("read golden");
    let fmt_out = Command::new(bin)
        .args(["fmt", path.to_str().unwrap()])
        .output()
        .expect("fmt");
    assert_eq!(fmt_out.status.code(), Some(0));
    assert_eq!(fmt_out.stdout, on_disk);
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn nested_loop_exits_0() {
    let path = golden("nested_loop.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn loop_break_carry_exits_0() {
    let path = golden("loop_break_carry.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn loop_continue_carry_exits_0() {
    let path = golden("loop_continue_carry.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn if_arm_nested_loop_exits_0() {
    let path = golden("if_arm_nested_loop.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn loop_continue_if_else_exits_0() {
    let path = golden("loop_continue_if_else.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn loop_stmt_gap_exits_0() {
    let path = golden("loop_stmt_gap.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn loop_continue_arity_miss_check_e_type() {
    check_e_type(&golden("loop_continue_arity_miss.strake"));
}

#[test]
fn loop_continue_arity_if_then_check_e_type() {
    check_e_type(&golden("loop_continue_arity_if_then.strake"));
}

#[test]
fn loop_continue_arity_if_else_check_e_type() {
    check_e_type(&golden("loop_continue_arity_if_else.strake"));
}

#[test]
fn loop_continue_arity_nested_if_check_e_type() {
    check_e_type(&golden("loop_continue_arity_nested_if.strake"));
}

#[test]
fn loop_continue_arity_inner_loop_if_check_e_type() {
    check_e_type(&golden("loop_continue_arity_inner_loop_if.strake"));
}

#[test]
fn loop_continue_arity_dup_if_check_e_type() {
    check_e_type(&golden("loop_continue_arity_dup_if.strake"));
}

#[test]
fn loop_continue_bool_i64_check_e_type() {
    check_e_type(&golden("loop_continue_bool_i64.strake"));
}

#[test]
fn loop_break_outside_check_e_type() {
    check_e_type(&golden("loop_break_outside.strake"));
}

#[test]
fn loop_continue_outside_check_e_type() {
    check_e_type(&golden("loop_continue_outside.strake"));
}
