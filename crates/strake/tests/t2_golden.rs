use std::path::PathBuf;
use std::process::Command;

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../golden")
        .join(name)
}

fn run(args: &[&str]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_strake");
    Command::new(bin).args(args).output().expect("run strake")
}

#[test]
fn fmt_add2_worked_example_idempotent() {
    const ADD2: &str = include_str!("../../../golden/add2_worked.strake");
    let tmp = std::env::temp_dir().join("strake_add2_worked.strake");
    std::fs::write(&tmp, ADD2).expect("write temp");
    let path = tmp.to_str().unwrap();
    let out = run(&["fmt", path]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), ADD2);
    assert!(out.stderr.is_empty());
}

#[test]
fn fuel_twice_check_test_fmt() {
    let path = golden("fuel_twice.strake");
    let expected = std::fs::read_to_string(&path).expect("read golden");
    let p = path.to_str().unwrap();

    let check = run(&["check", p]);
    assert_eq!(check.status.code(), Some(3));
    assert!(!String::from_utf8_lossy(&check.stderr).contains("code"));

    let test = run(&["test", p]);
    assert_eq!(test.status.code(), Some(3));
    assert!(!String::from_utf8_lossy(&test.stderr).contains("code"));

    let fmt = run(&["fmt", p]);
    assert_eq!(fmt.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&fmt.stdout), expected);
}

#[test]
fn pages_twice_check_test_fmt() {
    let path = golden("pages_twice.strake");
    let expected = std::fs::read_to_string(&path).expect("read golden");
    let p = path.to_str().unwrap();

    let check = run(&["check", p]);
    assert_eq!(check.status.code(), Some(3));
    assert!(!String::from_utf8_lossy(&check.stderr).contains("code"));

    let test = run(&["test", p]);
    assert_eq!(test.status.code(), Some(3));
    assert!(!String::from_utf8_lossy(&test.stderr).contains("code"));

    let fmt = run(&["fmt", p]);
    assert_eq!(fmt.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&fmt.stdout), expected);
}
