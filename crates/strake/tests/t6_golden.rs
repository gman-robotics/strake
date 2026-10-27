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
fn wrap_exits_0() {
    let path = golden("wrap.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn div0_exits_2_two_ediv0_trap_lines() {
    let path = golden("div0.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(2));
    assert_eq!(stderr.matches("E_DIV0").count(), 2);
    assert_eq!(stderr.matches("\"kind\":\"trap\"").count(), 2);
}

#[test]
fn fuel_exits_2_efuel_trap() {
    let path = golden("fuel.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(2));
    assert!(stderr.contains("E_FUEL"));
    assert!(stderr.contains("\"kind\":\"trap\""));
}

#[test]
fn stack_exits_2_estack_trap() {
    let path = golden("stack.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(2));
    assert!(stderr.contains("E_STACK"));
    assert!(stderr.contains("\"kind\":\"trap\""));
}

#[test]
fn not_bool_exits_0() {
    let path = golden("not_bool.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn not_bool_fail_exits_1_e_test() {
    let path = golden("not_bool_fail.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_TEST"));
    assert!(stderr.contains("\"want\":\"false\""));
    assert!(stderr.contains("\"got\":\"true\""));
}

#[test]
fn multi_fn_calls_target_correct_bodies() {
    let path = golden("multi_fn.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn fuel_efuel_span_names_source_file() {
    let path = golden("fuel.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(2));
    assert!(stderr.contains("E_FUEL"));
    assert!(stderr.contains("fuel.strake"));
    assert!(!stderr.contains("\"file\":\"\""));
}
