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
fn if_arm_assert_pass_exits_0() {
    let path = golden("if_arm_assert_pass.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn if_arm_assert_fail_exits_1_e_test() {
    let path = golden("if_arm_assert_fail.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_TEST"));
}

#[test]
fn if_arm_nested_if_exits_0() {
    let path = golden("if_arm_nested_if.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn if_arm_call_exits_0() {
    let path = golden("if_arm_call.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn if_nonlast_fn_exits_0() {
    let path = golden("if_nonlast_fn.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn if_fn_nested_outer_join_exits_0() {
    let path = golden("if_fn_nested_outer_join.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn if_fn_nested_outer_join_first_exits_0() {
    let path = golden("if_fn_nested_outer_join_first.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn if_test_outer_join_after_split_exits_0() {
    let path = golden("if_test_outer_join_after_split.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn bool_phi_assert_mismatch_exits_1_e_test() {
    let path = golden("bool_phi_assert_mismatch.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_TEST"));
}

#[test]
fn fn_no_ret_fuel5_exits_2_empty_stderr() {
    let path = golden("fn_no_ret_fuel5.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(2));
    assert!(stderr.is_empty());
}
