//! GMA-423: shared match join name, and bool loop parameters.

use std::path::PathBuf;

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../golden")
        .join(name)
}

/// `strake test` in this process so coverage sees lower and exec.
fn run_test(name: &str) -> (i32, String) {
    let path = golden(name);
    let exit = strake::test(path.to_str().unwrap(), strake::Opts::default());
    (exit.code, exit.stderr)
}

#[test]
fn match_join_shared_returns_join_value() {
    let (code, stderr) = run_test("match_join_shared.strake");
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stderr.is_empty(), "stderr={stderr}");
}

#[test]
fn match_join_missing_arm_stays_codeless() {
    let path = golden("match_join.strake");
    let exit = strake::test(path.to_str().unwrap(), strake::Opts::default());
    assert_eq!(exit.code, 3, "stderr={}", exit.stderr);
    assert!(!exit.stderr.contains("\"code\""), "stderr={}", exit.stderr);
    assert!(
        exit.stderr.contains("match arm missing the join value"),
        "stderr={}",
        exit.stderr
    );
}

#[test]
fn loop_bool_break_exits_0() {
    let (code, stderr) = run_test("loop_bool_break.strake");
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stderr.is_empty(), "stderr={stderr}");
}

#[test]
fn loop_bool_both_arms_exits_0() {
    let (code, stderr) = run_test("loop_bool_both_arms.strake");
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stderr.is_empty(), "stderr={stderr}");
}

#[test]
fn loop_bool_swap_exits_0() {
    let (code, stderr) = run_test("loop_bool_swap.strake");
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stderr.is_empty(), "stderr={stderr}");
}

#[test]
fn loop_i64_swap_still_exits_0() {
    let (code, stderr) = run_test("loop_swap.strake");
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stderr.is_empty(), "stderr={stderr}");
}

#[test]
fn if_join_keeps_every_shared_assign() {
    let (code, stderr) = run_test("if_join_s5_waiver.strake");
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stderr.is_empty(), "stderr={stderr}");
}

#[test]
fn match_arm_nested_if_keeps_second_assign() {
    let src = "\
strake 1

fn pick(r: result i64 unit) -> i64
  c = true
  match r
    ok v ->
      if c
        x = v
        y = add i64 2 0
      else
        x = add i64 0 0
        y = add i64 4 0
      end
    err _ ->
      x = add i64 5 0
      y = add i64 6 0
  end
  ret y
end

test t
  v = ok i64 7
  n = call pick(v)
  assert.eq n 2
  ret 0
end
";
    let path = std::env::temp_dir().join("strake_gma612_match_nested_if.strake");
    std::fs::write(&path, src).expect("write");
    let exit = strake::test(path.to_str().unwrap(), strake::Opts::default());
    assert_eq!(exit.code, 0, "stderr={}", exit.stderr);
    assert!(exit.stderr.is_empty(), "stderr={}", exit.stderr);
}
