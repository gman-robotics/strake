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
    let src = std::fs::read_to_string(&path).expect("read");
    let fmt_again = Command::new(bin)
        .args(["fmt", path.to_str().unwrap()])
        .output()
        .expect("fmt again");
    assert_eq!(fmt_again.status.code(), Some(0));
    let src2 = std::fs::read_to_string(&path).expect("read2");
    assert_eq!(src, src2);
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
