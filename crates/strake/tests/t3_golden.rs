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
fn add2_basic_test_exits_zero_empty_stderr() {
    let path = golden("add2_basic.strake");
    let p = path.to_str().unwrap();
    let out = run(&["test", p]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
}
