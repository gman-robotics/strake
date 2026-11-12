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
