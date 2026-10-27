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

fn write_temp(src: &str, name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, src).expect("write temp");
    path
}

#[test]
fn for_empty_list_body_exits_0() {
    let src = "strake 1\n\ntest t\n  t0 = list.empty i64\n  for x in t0\n    ret 1\n  end\n  ret 0\nend\n";
    let path = write_temp(src, "strake_for_empty.strake");
    let (status, _, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn for_one_element_returns_element() {
    let src = "strake 1\n\ntest t\n  t0 = list.empty i64\n  xs = list.cons 7 t0\n  for x in xs\n    assert.eq x 7\n  end\n  ret 0\nend\n";
    let path = write_temp(src, "strake_for_one.strake");
    let (status, _, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn module_list_forall_assert_exits_0() {
    let src = "strake 1\n\nt0 = list.empty i64\nxs = list.cons 1 t0\n\nproperty p\n  forall n: i64 in xs\n    assert.eq n 1\n  end\nend\n";
    let path = write_temp(src, "strake_forall_list_ok.strake");
    let (status, _, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn loop_continue_after_if_exits_0() {
    let src = "strake 1\n\ntest t\n  loop acc = 0\n    next = add i64 acc 1\n    stop = eq i64 next 2\n    if stop\n      break\n    end\n    continue acc = next\n  end\n  ret 0\nend\n";
    let path = write_temp(src, "strake_loop_continue_if.strake");
    let (status, _, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn assert_in_loop_fails_e_test() {
    let src =
        "strake 1\n\ntest t\n  loop acc = 0\n    assert.eq acc 1\n    break\n  end\n  ret 0\nend\n";
    let path = write_temp(src, "strake_loop_assert_fail.strake");
    let (status, _, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_TEST"));
}

#[test]
fn match_err_before_ok_on_ok_sum() {
    let src = "strake 1\n\nfn pick(r: result i64 unit) -> i64\n  match r\n    err _ ->\n      out = 0\n    ok v ->\n      out = v\n  end\n  ret out\nend\n\ntest t\n  r = ok i64 7\n  x = call pick(r)\n  assert.eq x 7\n  ret 0\nend\n";
    let path = write_temp(src, "strake_match_order.strake");
    let (status, _, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn nested_loop_golden_exits_0() {
    let path = golden("nested_loop.strake");
    let (status, _, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn forall_name_range_lo_dotdot_hi_fmt_roundtrip() {
    let src = "strake 1\n\nlo = 0\nhi = 2\n\nproperty p\n  forall n: i64 in lo..hi\n    assert.eq n n\n  end\nend\n";
    let path = write_temp(src, "strake_forall_bind_range.strake");
    let (status, _, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0), "stderr: {stderr}");
    let bin = env!("CARGO_BIN_EXE_strake");
    let fmt_out = std::process::Command::new(bin)
        .args(["fmt", path.to_str().unwrap()])
        .output()
        .expect("fmt");
    assert_eq!(fmt_out.status.code(), Some(0));
    let formatted = String::from_utf8_lossy(&fmt_out.stdout);
    assert!(formatted.contains("lo..hi") || formatted.contains("lo .. hi"));
    let (status2, _, stderr2) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status2.code(), Some(0), "stderr: {stderr2}");
}
