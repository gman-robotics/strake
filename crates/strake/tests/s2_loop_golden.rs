use std::path::PathBuf;
use std::process::Command;

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../golden")
        .join(name)
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
    let fmt_out = Command::new(bin)
        .args(["fmt", path.to_str().unwrap()])
        .output()
        .expect("fmt");
    assert_eq!(fmt_out.status.code(), Some(0));
    let tmp = std::env::temp_dir().join("strake_loop_arity_fmt.strake");
    std::fs::write(&tmp, &fmt_out.stdout).expect("write fmt stdout");
    let fmt_again = Command::new(bin)
        .args(["fmt", tmp.to_str().unwrap()])
        .output()
        .expect("fmt again");
    assert_eq!(fmt_again.status.code(), Some(0));
    assert_eq!(fmt_again.stdout, fmt_out.stdout);
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
    let path = golden("loop_continue_arity_miss.strake");
    let (status, _stdout, stderr) = run(&["check", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_TYPE"));
}
