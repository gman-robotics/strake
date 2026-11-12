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

#[test]
fn forall_module_list_name_is_legal_and_runs() {
    let path = golden("forall_list_ok.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0), "{stderr}");
    assert!(stderr.is_empty());
}

/// Runs forall_list_ok ([2, 3]) with the body asserting `x == want`.
fn forall_list_ok_asserting(want: &str) -> (std::process::ExitStatus, String) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join(format!("forall_list_each_{want}.strake"));
    let src = std::fs::read_to_string(golden("forall_list_ok.strake"))
        .expect("read golden")
        .replace("assert.eq x x", &format!("assert.eq x {want}"));
    std::fs::write(&path, src).expect("write");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    (status, stderr)
}

#[test]
fn forall_module_list_body_sees_every_element() {
    // Asserting 2 fails on 3 and asserting 3 fails on 2, so both were visited.
    for (want, got) in [("2", "3"), ("3", "2")] {
        let (status, stderr) = forall_list_ok_asserting(want);
        assert_eq!(status.code(), Some(1), "{stderr}");
        assert!(stderr.contains("\"E_TEST\""));
        assert!(stderr.contains(&format!("\"got\":\"{got}\"")), "{stderr}");
    }
}

fn run_src(name: &str, src: &str) -> (i32, String) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join(name);
    std::fs::write(&path, src).expect("write");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    (status.code().unwrap_or(-1), stderr)
}

/// Module list `[2, 3]`, same shape as `golden/forall_list_ok.strake`, driven with `for`.
fn for_module_list_src(body_assert: &str) -> String {
    format!(
        "strake 1\n\nt0 = list.empty i64\n\nt1 = list.cons 3 t0\n\nxs = list.cons 2 t1\n\ntest each\n  for x in xs\n    {body_assert}\n  end\n  ret 0\nend\n"
    )
}

#[test]
fn for_over_module_list_runs() {
    let (code, stderr) = run_src(
        "for_mod_list.strake",
        &for_module_list_src("assert.eq x x"),
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn for_over_module_list_sees_every_element() {
    for (want, got) in [("2", "3"), ("3", "2")] {
        let (code, stderr) = run_src(
            &format!("for_mod_each_{want}.strake"),
            &for_module_list_src(&format!("assert.eq x {want}")),
        );
        assert_eq!(code, 1, "{stderr}");
        assert!(stderr.contains("\"E_TEST\""), "{stderr}");
        assert!(stderr.contains(&format!("\"got\":\"{got}\"")), "{stderr}");
    }
}

fn forall_list_binder_src(ty: &str) -> String {
    format!(
        "strake 1\n\nxs = list.empty i64\n\nproperty p\n  forall n: {ty} in xs\n  end\nend\n"
    )
}

fn forall_range_binder_src(ty: &str) -> String {
    format!("strake 1\n\nproperty p\n  forall n: {ty} in 0..1\n  end\nend\n")
}

fn assert_binder_rejected(name: &str, src: &str) {
    let (code, stderr) = run_src(name, src);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("\"E_TYPE\""), "{stderr}");
    assert!(!stderr.contains("\"E_TEST\""), "{stderr}");
}

#[test]
fn forall_bool_binder_on_i64_list_is_etype() {
    assert_binder_rejected(
        "forall_bool_list.strake",
        &forall_list_binder_src("bool"),
    );
}

#[test]
fn forall_bool_binder_on_range_is_etype() {
    assert_binder_rejected(
        "forall_bool_range.strake",
        &forall_range_binder_src("bool"),
    );
}

#[test]
fn forall_i64_binder_on_list_and_range_runs() {
    for (name, src) in [
        ("forall_i64_list.strake", forall_list_binder_src("i64")),
        ("forall_i64_range.strake", forall_range_binder_src("i64")),
    ] {
        let (code, stderr) = run_src(name, &src);
        assert_eq!(code, 0, "{stderr}");
        assert!(stderr.is_empty(), "{stderr}");
    }
}

#[test]
fn empty_for_then_ret_binder_is_etype() {
    let src = "strake 1\n\ntest t\n  xs = list.empty i64\n  for x in xs\n  end\n  ret x\nend\n";
    let (code, stderr) = run_src("for_binder_leak.strake", src);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("\"E_TYPE\""), "{stderr}");
    assert!(!stderr.is_empty(), "{stderr}");
}

#[test]
fn empty_for_in_if_then_ret_binder_is_etype() {
    let src = "strake 1\n\ntest t\n  if true\n    xs = list.empty i64\n    for x in xs\n    end\n    ret x\n  end\n  ret 0\nend\n";
    let (code, stderr) = run_src("for_binder_leak_if.strake", src);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("\"E_TYPE\""), "{stderr}");
    assert!(!stderr.is_empty(), "{stderr}");
}

#[test]
fn forall_binder_is_not_visible_after() {
    let src = "strake 1\n\nproperty p\n  forall n: i64 in 0..1\n  end\n  assert.eq n 0\nend\n";
    let (code, stderr) = run_src("forall_binder_after.strake", src);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("\"E_TYPE\""), "{stderr}");
    assert!(!stderr.contains("\"E_TEST\""), "{stderr}");
}

#[test]
fn nested_forall_binder_is_not_visible_after() {
    let src = "strake 1\n\nproperty p\n  forall n: i64 in 0..1\n    forall m: i64 in 0..1\n    end\n    assert.eq m 0\n  end\nend\n";
    let (code, stderr) = run_src("forall_binder_nested.strake", src);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("\"E_TYPE\""), "{stderr}");
    assert!(!stderr.contains("\"E_TEST\""), "{stderr}");
}
