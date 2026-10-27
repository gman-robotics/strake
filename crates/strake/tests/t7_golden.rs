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
fn loop_exits_0() {
    let path = golden("loop.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn loop_arity_exits_1_e_type() {
    let path = golden("loop_arity.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_TYPE"));
}

#[test]
fn match_join_exits_3_plain_text() {
    let path = golden("match_join.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(3));
    assert!(!stderr.contains("\"code\""));
    assert!(!stderr.contains("E_MATCH"));
    assert!(!stderr.contains("E_IF_PHI"));
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
fn bool_ret_exits_0() {
    let path = golden("bool_ret.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn stack_call_span_ends_at_rparen() {
    let path = golden("stack.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(2));
    assert!(stderr.contains("E_STACK"));
    assert!(stderr.contains("stack.strake"));
    let src = std::fs::read_to_string(&path).expect("read");
    let call_line = src.lines().nth(3).expect("call line");
    let close = call_line.find(')').expect("paren");
    let file_offset = src.find(call_line).expect("line in file") + close + 1;
    assert!(
        stderr.contains(&format!("\"end\":{file_offset}")),
        "expected E_STACK span end at closing paren byte {file_offset}, stderr: {stderr}"
    );
}
