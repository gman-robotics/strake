//! S3 match / sum golden CLI tests and required-fix rows.

use std::path::{Path, PathBuf};
use std::process::Command;

use strake::{check as strake_check, test as strake_test, Opts};

const MATCH_JOIN_CODELESS: &str = "match arm missing the join value";

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../golden")
        .join(name)
}

fn run(args: &[&str]) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_strake");
    let out = Command::new(bin).args(args).output().expect("run");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn assert_test_ok(path: &Path) {
    let (code, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr={stderr}");
}

fn assert_check_codeless(path: &Path) {
    let (code, stderr) = run(&["check", path.to_str().unwrap()]);
    assert_eq!(code, 3, "stderr={stderr}");
    assert!(!stderr.contains("\"code\""), "stderr={stderr}");
    assert!(stderr.contains(MATCH_JOIN_CODELESS), "stderr={stderr}");
}

fn assert_check_codeless_draft(path: &Path) {
    let (code, stderr) = run(&["check", "--draft", path.to_str().unwrap()]);
    assert_eq!(code, 3, "stderr={stderr}");
    assert!(!stderr.contains("\"code\""), "stderr={stderr}");
    assert!(stderr.contains(MATCH_JOIN_CODELESS), "stderr={stderr}");
    assert!(!stderr.contains("lower:"), "stderr={stderr}");
}

fn assert_test_codeless_draft(path: &Path) {
    let (code, stderr) = run(&["test", "--draft", path.to_str().unwrap()]);
    assert_eq!(code, 3, "stderr={stderr}");
    assert!(!stderr.contains("\"code\""), "stderr={stderr}");
    assert!(stderr.contains(MATCH_JOIN_CODELESS), "stderr={stderr}");
    assert!(!stderr.contains("lower:"), "stderr={stderr}");
}

#[test]
fn accept_goldens_exit_0() {
    for name in [
        "match_result.strake",
        "match_two_names.strake",
        "match_in_if.strake",
        "match_loop_fallthrough.strake",
        "match_both_ret.strake",
        "match_trap_arm.strake",
    ] {
        assert_test_ok(&golden(name));
    }
}

#[test]
fn match_join_codeless_check_and_test() {
    let p = golden("match_join.strake");
    assert_check_codeless(&p);
    let (code, stderr) = run(&["test", p.to_str().unwrap()]);
    assert_eq!(code, 3, "stderr={stderr}");
    assert!(!stderr.contains("\"code\""));
}

#[test]
fn match_join_codeless_draft_check_and_test() {
    let p = golden("match_join.strake");
    assert_check_codeless_draft(&p);
    assert_test_codeless_draft(&p);
}

#[test]
fn match_join_assign_codeless() {
    assert_check_codeless(&golden("match_join_assign.strake"));
}

#[test]
fn reject_catalog_rows() {
    let cases: &[(&str, &str)] = &[
        ("match_miss.strake", "E_MATCH"),
        ("match_nonsum.strake", "E_TYPE"),
        ("match_unknown_tag.strake", "E_TYPE"),
        ("match_dup_tag.strake", "E_TYPE"),
        ("match_shadow.strake", "E_SHADOW"),
        ("match_payload.strake", "E_TYPE"),
        ("match_eq.strake", "E_TYPE"),
        ("match_binder_after.strake", "E_TYPE"),
        ("match_seq.strake", "E_SEQ"),
    ];
    for (file, code) in cases {
        let (c, stderr) = run(&["check", golden(file).to_str().unwrap()]);
        assert_eq!(c, 1, "file={file} stderr={stderr}");
        assert!(stderr.contains(code), "file={file} stderr={stderr}");
    }
}

#[test]
fn partial_use_in_if_arm_exits_3() {
    let src = "strake 1\n\nfn f(c: bool, r: result i64 unit) -> i64\n  if c\n    match r\n      ok v ->\n        out = v\n      err _ ->\n        skip = 0\n    end\n    ret out\n  else\n    ret 1\n  end\nend\n\ntest t\n  ret 0\nend\n";
    let tmp = std::env::temp_dir().join("strake_s3_partial_if.strake");
    std::fs::write(&tmp, src).expect("write");
    let exit = strake_check(tmp.to_str().unwrap(), Opts::default());
    assert_eq!(exit.code, 3);
    assert!(exit.stderr.contains(MATCH_JOIN_CODELESS));
}

#[test]
fn partial_use_in_loop_body_exits_3() {
    let src = "strake 1\n\nfn f(r: result i64 unit) -> i64\n  loop i = 0\n    match r\n      ok v ->\n        out = v\n      err _ ->\n        skip = 0\n    end\n    ret out\n  end\nend\n\ntest t\n  ret 0\nend\n";
    let tmp = std::env::temp_dir().join("strake_s3_partial_loop.strake");
    std::fs::write(&tmp, src).expect("write");
    let exit = strake_check(tmp.to_str().unwrap(), Opts::default());
    assert_eq!(exit.code, 3);
    assert!(exit.stderr.contains(MATCH_JOIN_CODELESS));
}

#[test]
fn ret_arm_assign_then_ret_out_is_e_type_not_70() {
    let src = "strake 1\n\nfn f(r: result i64 unit) -> i64\n  match r\n    ok v ->\n      out = v\n      ret out\n    err _ ->\n      skip = 0\n  end\n  ret out\nend\n\ntest t\n  ret 0\nend\n";
    let tmp = std::env::temp_dir().join("strake_s3_ret_arm_e_type.strake");
    std::fs::write(&tmp, src).expect("write");
    let exit = strake_check(tmp.to_str().unwrap(), Opts::default());
    assert_eq!(exit.code, 1, "stderr={}", exit.stderr);
    assert!(exit.stderr.contains("E_TYPE"), "stderr={}", exit.stderr);
    assert!(!exit.stderr.contains("lower:"), "stderr={}", exit.stderr);
    let exit_test = strake_test(tmp.to_str().unwrap(), Opts::default());
    assert_eq!(exit_test.code, 1);
    assert!(!exit_test.stderr.contains("lower:"));
}

#[test]
fn partial_then_assign_out_exits_0() {
    let src = "strake 1\n\nfn f(r: result i64 unit) -> i64\n  match r\n    ok v ->\n      out = v\n    err _ ->\n      skip = 0\n  end\n  out = 0\n  ret out\nend\n\ntest t\n  v = ok i64 1\n  n = call f(v)\n  assert.eq n 0\n  ret 0\nend\n";
    let tmp = std::env::temp_dir().join("strake_s3_assign_after_partial.strake");
    std::fs::write(&tmp, src).expect("write");
    let exit = strake_test(tmp.to_str().unwrap(), Opts::default());
    assert_eq!(exit.code, 0);
}

#[test]
fn later_if_assign_publish_out_exits_0() {
    let src = "strake 1\n\nfn f(c: bool, r: result i64 unit) -> i64\n  match r\n    ok v ->\n      out = v\n    err _ ->\n      skip = 0\n  end\n  if c\n    out = 1\n  else\n    out = 2\n  end\n  ret out\nend\n\ntest t\n  v = ok i64 9\n  n = call f(true, v)\n  assert.eq n 1\n  ret 0\nend\n";
    let tmp = std::env::temp_dir().join("strake_s3_if_after_partial.strake");
    std::fs::write(&tmp, src).expect("write");
    let exit = strake_test(tmp.to_str().unwrap(), Opts::default());
    assert_eq!(exit.code, 0);
}

#[test]
fn match_in_test_body_exits_0() {
    let src = "strake 1\n\nfn pick(r: result i64 unit) -> i64\n  match r\n    ok v ->\n      out = v\n    err _ ->\n      out = 0\n  end\n  ret out\nend\n\ntest t\n  v = ok i64 7\n  n = call pick(v)\n  assert.eq n 7\n  ret 0\nend\n";
    let tmp = std::env::temp_dir().join("strake_s3_match_in_test.strake");
    std::fs::write(&tmp, src).expect("write");
    let exit = strake_test(tmp.to_str().unwrap(), Opts::default());
    assert_eq!(exit.code, 0, "stderr={}", exit.stderr);
}

#[test]
fn if_loop_exit_and_match_publish_two_names() {
    let src = "strake 1\n\nfn f(c: bool, r: result i64 unit) -> i64\n  if c\n    loop x = 1\n      break\n    end\n    match r\n      ok v ->\n        y = v\n      err _ ->\n        y = 0\n    end\n  else\n    x = 2\n    y = 3\n  end\n  n = add i64 x y\n  ret n\nend\n\ntest t1\n  v = ok i64 7\n  n = call f(true, v)\n  assert.eq n 8\n  ret 0\nend\n\ntest t2\n  v = ok i64 2\n  n = call f(false, v)\n  assert.eq n 5\n  ret 0\nend\n";
    let tmp = std::env::temp_dir().join("strake_s3_if_loop_match.strake");
    std::fs::write(&tmp, src).expect("write");
    let exit = strake_test(tmp.to_str().unwrap(), Opts::default());
    assert_eq!(exit.code, 0, "stderr={}", exit.stderr);
}
