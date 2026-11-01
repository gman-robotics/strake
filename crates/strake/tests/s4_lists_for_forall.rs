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
fn for_list_exits_0_no_e_oob() {
    let path = golden("for_list.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(!stderr.contains("E_OOB"));
}

#[test]
fn forall_rev_exits_1_e_forall() {
    let path = golden("forall_rev.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_FORALL"));
}

#[test]
fn forall_ok_exits_0() {
    let path = golden("forall_ok.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn forall_list_bad_exits_3_no_e_forall() {
    let path = golden("forall_list_bad.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(3));
    assert!(!stderr.contains("E_FORALL"));
    assert!(!stderr.contains("\"code\""));
}

#[test]
fn forall_rev_in_match_arm_exits_1_e_forall() {
    let path = golden("forall_rev_match_arm.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_FORALL"));
}

#[test]
fn if_arm_for_exits_0() {
    let path = golden("if_arm_for.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn if_arm_forall_exits_0() {
    let path = golden("if_arm_forall.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn match_arm_for_exits_0() {
    let path = golden("match_arm_for.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn forall_rev_exec_counter_stays_zero() {
    let path = golden("forall_rev.strake");
    let bin = env!("CARGO_BIN_EXE_strake");
    let out = Command::new(bin)
        .args(["test", path.to_str().unwrap()])
        .env("STRAKE_EXEC_COUNTER", "1")
        .output()
        .expect("run strake");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E_FORALL"));
    assert!(!stderr.contains("exec="));
}
