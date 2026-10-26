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
fn test_fail_exits_1_e_test_want_got() {
    let path = golden("test_fail.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_TEST"));
    assert!(stderr.contains("\"kind\":\"result\""));
    assert!(stderr.contains("\"want\":\"14\""));
    assert!(stderr.contains("\"got\":\"16\""));
    assert!(!stderr.contains('%'));
}

#[test]
fn refused_exits_77_e_refused() {
    let path = golden("refused.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(77));
    assert!(stderr.contains("E_REFUSED"));
    assert!(stderr.contains("\"kind\":\"result\""));
}

#[test]
fn refused_computed_seventy_exits_77_e_refused() {
    let path = golden("refused_computed.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(77));
    assert!(stderr.contains("E_REFUSED"));
    assert!(stderr.contains("\"kind\":\"result\""));
}

#[test]
fn callee_77_exits_0_no_e_refused() {
    let path = golden("callee_77.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(!stderr.contains("E_REFUSED"));
    assert!(stderr.is_empty());
}
