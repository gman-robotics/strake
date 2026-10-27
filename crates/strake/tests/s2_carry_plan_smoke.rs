//! Check + lower smoke: goldens and r1 blocker shapes must not exit 70 except S5 waiver.

use std::path::{Path, PathBuf};
use std::process::Command;

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../golden")
        .join(name)
}

fn run(args: &[&str]) -> (std::process::ExitStatus, String) {
    let bin = env!("CARGO_BIN_EXE_strake");
    let out = Command::new(bin).args(args).output().expect("run strake");
    (
        out.status,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn assert_test_ok(path: &Path) {
    let (status, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0), "stderr={stderr}");
    assert!(stderr.is_empty(), "stderr={stderr}");
}

fn write_temp(name: &str, src: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("strake_carry_smoke_{name}.strake"));
    std::fs::write(&p, src).expect("write temp");
    p
}

#[test]
fn executing_loop_goldens_no_exit_70() {
    let ok: &[&str] = &[
        "loop.strake",
        "loop_swap.strake",
        "loop_swap_ba.strake",
        "loop_cross_param.strake",
        "loop_cross_param_ba.strake",
        "loop_cycle_three.strake",
        "loop_init_ba.strake",
        "loop_both_arms_loop_true.strake",
        "loop_both_arms_loop_false.strake",
        "loop_both_arms_nested_if.strake",
        "loop_mirror_true.strake",
        "loop_mirror_false.strake",
        "loop_arm_join_true.strake",
        "loop_arm_join_false.strake",
        "loop_dual_loop_join.strake",
        "loop_fn_and_test_loops.strake",
        "loop_join_arm_init_n.strake",
        "nested_loop.strake",
        "loop_break_carry.strake",
        "loop_continue_carry.strake",
        "loop_continue_if_else.strake",
        "loop_continue_nested_if.strake",
        "loop_break_if_continue_else.strake",
        "loop_stmt_gap.strake",
        "if_arm_nested_loop.strake",
    ];
    for name in ok {
        assert_test_ok(&golden(name));
    }
}

#[test]
fn blocker_shapes_no_exit_70() {
    let init_ba = write_temp(
        "init_ba",
        "strake 1\n\ntest t\n  loop a = 1 b = a\n    break\n  end\n  ret 0\nend\n",
    );
    assert_test_ok(&init_ba);

    let fn_plus_test = write_temp(
        "fn_test_loops",
        "strake 1\n\nfn f() -> i64\n  loop i = 0\n    break\n  end\n  ret i\nend\n\ntest t\n  loop j = 2\n    break\n  end\n  ret 0\nend\n",
    );
    assert_test_ok(&fn_plus_test);

    let nested_if = write_temp(
        "both_arms_nested_if",
        "strake 1\n\ntest t\n  c = true\n  c2 = false\n  if c\n    loop x = 7\n      if c2\n        break\n      end\n      break\n    end\n  else\n    loop x = 3\n      break\n    end\n  end\n  ret 0\nend\n",
    );
    assert_test_ok(&nested_if);

    assert_test_ok(&golden("loop_dual_loop_join.strake"));
    assert_test_ok(&golden("loop_mirror_true.strake"));
    assert_test_ok(&golden("loop_arm_join_true.strake"));
}

#[test]
fn s5_waiver_still_exits_70() {
    let (status, stderr) = run(&["test", golden("if_join_s5_waiver.strake").to_str().unwrap()]);
    assert_eq!(status.code(), Some(70), "stderr={stderr}");
    assert!(stderr.contains("unbound name in lower"), "stderr={stderr}");
}

#[test]
fn nested_if_one_arm_check_e_if_phi_not_test() {
    let p = golden("if_nested_then_else_direct_e_if_phi.strake");
    let (status, stderr) = run(&["check", p.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1), "stderr={stderr}");
    assert!(stderr.contains("E_IF_PHI"), "stderr={stderr}");
    let (tstatus, terr) = run(&["test", p.to_str().unwrap()]);
    assert_eq!(tstatus.code(), Some(1), "stderr={terr}");
    assert!(!terr.contains("lower:"), "must not lower: {terr}");
}
