use std::path::{Path, PathBuf};
use std::process::Command;

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../golden")
        .join(name)
}

const E_SEQ_MSG: &str = "statement follows ret, break, or continue in the same block";
const E_IF_PHI_MSG: &str = "a name used after if is missing from one arm";

fn run(args: &[&str]) -> (std::process::ExitStatus, String, String) {
    let bin = env!("CARGO_BIN_EXE_strake");
    let out = Command::new(bin).args(args).output().expect("run strake");
    (
        out.status,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn test_exits_0(path: &Path) {
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0), "stderr={stderr}");
    assert!(stderr.is_empty(), "stderr={stderr}");
}

fn check_e_type(path: &Path) {
    let (status, _stdout, stderr) = run(&["check", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_TYPE"), "stderr={stderr}");
}

fn check_e_seq(path: &Path) {
    let (status, _stdout, stderr) = run(&["check", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_SEQ"), "stderr={stderr}");
    assert!(stderr.contains(E_SEQ_MSG), "stderr={stderr}");
}

fn check_e_if_phi(path: &Path) {
    let (status, _stdout, stderr) = run(&["check", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_IF_PHI"), "stderr={stderr}");
    assert!(stderr.contains(E_IF_PHI_MSG), "stderr={stderr}");
}

fn test_exits_70_contains(path: &Path, needle: &str) {
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(70), "stderr={stderr}");
    assert!(stderr.contains(needle), "stderr={stderr}");
}

#[test]
fn loop_swap_exits_0() {
    test_exits_0(&golden("loop_swap.strake"));
}

#[test]
fn loop_swap_ba_exits_0() {
    test_exits_0(&golden("loop_swap_ba.strake"));
}

#[test]
fn loop_cross_param_exits_0() {
    test_exits_0(&golden("loop_cross_param.strake"));
}

#[test]
fn loop_cross_param_ba_exits_0() {
    test_exits_0(&golden("loop_cross_param_ba.strake"));
}

#[test]
fn loop_cycle_three_exits_0() {
    test_exits_0(&golden("loop_cycle_three.strake"));
}

#[test]
fn loop_both_arms_loop_true_exits_0() {
    test_exits_0(&golden("loop_both_arms_loop_true.strake"));
}

#[test]
fn loop_both_arms_loop_false_exits_0() {
    test_exits_0(&golden("loop_both_arms_loop_false.strake"));
}

#[test]
fn loop_mirror_true_exits_0() {
    test_exits_0(&golden("loop_mirror_true.strake"));
}

#[test]
fn loop_mirror_false_exits_0() {
    test_exits_0(&golden("loop_mirror_false.strake"));
}

#[test]
fn loop_arm_join_true_exits_0() {
    test_exits_0(&golden("loop_arm_join_true.strake"));
}

#[test]
fn loop_arm_join_false_exits_0() {
    test_exits_0(&golden("loop_arm_join_false.strake"));
}

#[test]
fn loop_break_if_continue_else_exits_0() {
    test_exits_0(&golden("loop_break_if_continue_else.strake"));
}

#[test]
fn loop_continue_nested_if_exits_0() {
    test_exits_0(&golden("loop_continue_nested_if.strake"));
}

#[test]
fn if_join_s5_waiver_exits_70() {
    test_exits_70_contains(&golden("if_join_s5_waiver.strake"), "unbound name in lower");
}

#[test]
fn loop_l7_exits_0() {
    test_exits_0(&golden("loop.strake"));
}

#[test]
fn loop_arity_fmt_fixed_point() {
    let path = golden("loop_arity.strake");
    let bin = env!("CARGO_BIN_EXE_strake");
    let on_disk = std::fs::read(&path).expect("read golden");
    let fmt_out = Command::new(bin)
        .args(["fmt", path.to_str().unwrap()])
        .output()
        .expect("fmt");
    assert_eq!(fmt_out.status.code(), Some(0));
    assert_eq!(fmt_out.stdout, on_disk);
    test_exits_0(&path);
}

#[test]
fn fn_no_ret_fuel5_fmt_fixed_point() {
    let path = golden("fn_no_ret_fuel5.strake");
    let bin = env!("CARGO_BIN_EXE_strake");
    let on_disk = std::fs::read(&path).expect("read golden");
    let fmt_out = Command::new(bin)
        .args(["fmt", path.to_str().unwrap()])
        .output()
        .expect("fmt");
    assert_eq!(fmt_out.status.code(), Some(0));
    assert_eq!(fmt_out.stdout, on_disk);
}

#[test]
fn nested_loop_exits_0() {
    test_exits_0(&golden("nested_loop.strake"));
}

#[test]
fn loop_break_carry_exits_0() {
    test_exits_0(&golden("loop_break_carry.strake"));
}

#[test]
fn loop_continue_carry_exits_0() {
    test_exits_0(&golden("loop_continue_carry.strake"));
}

#[test]
fn if_arm_nested_loop_exits_0() {
    test_exits_0(&golden("if_arm_nested_loop.strake"));
}

#[test]
fn loop_continue_if_else_exits_0() {
    test_exits_0(&golden("loop_continue_if_else.strake"));
}

#[test]
fn loop_stmt_gap_exits_0() {
    test_exits_0(&golden("loop_stmt_gap.strake"));
}

#[test]
fn loop_continue_arity_miss_check_e_type() {
    check_e_type(&golden("loop_continue_arity_miss.strake"));
}

#[test]
fn loop_continue_arity_if_then_check_e_type() {
    check_e_type(&golden("loop_continue_arity_if_then.strake"));
}

#[test]
fn loop_continue_arity_if_else_check_e_type() {
    check_e_type(&golden("loop_continue_arity_if_else.strake"));
}

#[test]
fn loop_continue_arity_nested_if_check_e_type() {
    check_e_type(&golden("loop_continue_arity_nested_if.strake"));
}

#[test]
fn loop_continue_arity_inner_loop_if_check_e_type() {
    check_e_type(&golden("loop_continue_arity_inner_loop_if.strake"));
}

#[test]
fn loop_continue_arity_dup_if_check_e_type() {
    check_e_type(&golden("loop_continue_arity_dup_if.strake"));
}

#[test]
fn loop_continue_dup_check_e_type() {
    check_e_type(&golden("loop_continue_dup.strake"));
}

#[test]
fn loop_continue_bool_i64_check_e_type() {
    check_e_type(&golden("loop_continue_bool_i64.strake"));
}

#[test]
fn loop_break_outside_check_e_type() {
    check_e_type(&golden("loop_break_outside.strake"));
}

#[test]
fn loop_continue_outside_check_e_type() {
    check_e_type(&golden("loop_continue_outside.strake"));
}

#[test]
fn loop_break_outside_if_check_e_type() {
    check_e_type(&golden("loop_break_outside_if.strake"));
}

#[test]
fn loop_continue_outside_if_check_e_type() {
    check_e_type(&golden("loop_continue_outside_if.strake"));
}

#[test]
fn loop_break_outside_nested_if_check_e_type() {
    check_e_type(&golden("loop_break_outside_nested_if.strake"));
}

#[test]
fn loop_continue_outside_nested_if_check_e_type() {
    check_e_type(&golden("loop_continue_outside_nested_if.strake"));
}

#[test]
fn loop_break_in_fn_check_e_type() {
    check_e_type(&golden("loop_break_in_fn.strake"));
}

#[test]
fn loop_seq_after_ret_check_e_seq() {
    check_e_seq(&golden("loop_seq_after_ret.strake"));
}

#[test]
fn loop_seq_after_break_check_e_seq() {
    check_e_seq(&golden("loop_seq_after_break.strake"));
}

#[test]
fn loop_seq_after_continue_check_e_seq() {
    check_e_seq(&golden("loop_seq_after_continue.strake"));
}

#[test]
fn loop_seq_continue_in_if_check_e_seq() {
    check_e_seq(&golden("loop_seq_continue_in_if.strake"));
}

#[test]
fn loop_seq_both_arms_terminate_check_e_seq() {
    check_e_seq(&golden("loop_seq_both_arms_terminate.strake"));
}

#[test]
fn loop_init_ba_exits_0() {
    test_exits_0(&golden("loop_init_ba.strake"));
}

#[test]
fn loop_both_arms_nested_if_exits_0() {
    test_exits_0(&golden("loop_both_arms_nested_if.strake"));
}

#[test]
fn loop_join_arm_init_n_exits_0() {
    test_exits_0(&golden("loop_join_arm_init_n.strake"));
}

#[test]
fn loop_fn_and_test_loops_exits_0() {
    test_exits_0(&golden("loop_fn_and_test_loops.strake"));
}

#[test]
fn loop_dual_loop_join_exits_0() {
    test_exits_0(&golden("loop_dual_loop_join.strake"));
}

#[test]
fn if_nested_then_else_direct_check_e_if_phi() {
    check_e_if_phi(&golden("if_nested_then_else_direct_e_if_phi.strake"));
}

fn check_e_if_phi_no_lower_70(path: &Path) {
    check_e_if_phi(path);
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1), "stderr={stderr}");
    assert!(
        !stderr.contains("lower:") && !stderr.contains("unbound name in lower"),
        "stderr={stderr}"
    );
}

#[test]
fn if_nested_inner_x_else_x3_check_e_if_phi_no_lower_70() {
    check_e_if_phi_no_lower_70(&golden("if_nested_inner_x_else_x3_e_if_phi.strake"));
}

#[test]
fn if_nested_inner_loop_x_else_x3_check_e_if_phi_no_lower_70() {
    check_e_if_phi_no_lower_70(&golden("if_nested_inner_loop_x_else_x3_e_if_phi.strake"));
}

#[test]
fn loop_zero_param_break_check_e_type() {
    check_e_type(&golden("loop_zero_param_break_e_type.strake"));
}

#[test]
fn loop_zero_param_continue_check_e_type() {
    check_e_type(&golden("loop_zero_param_continue_e_type.strake"));
}

#[test]
fn nested_loop_no_break_use_check_e_type() {
    check_e_type(&golden("nested_loop_no_break_use_e_type.strake"));
}
