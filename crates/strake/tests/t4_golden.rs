use std::path::PathBuf;
use std::process::Command;

use strake::{check, Opts};

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
fn shadow_exits_1_e_shadow() {
    let path = golden("shadow.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_SHADOW"));
}

#[test]
fn if_join_exits_0_empty_stderr() {
    let path = golden("if_join.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());
}

#[test]
fn if_phi_exits_1_e_if_phi() {
    let path = golden("if_phi.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_IF_PHI"));
}

#[test]
fn hole_ship_exits_1_e_hole_check() {
    let path = golden("hole_ship.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("E_HOLE"));
    assert!(stderr.contains("\"kind\":\"check\""));
}

#[test]
fn priv_check_exits_0() {
    let path = golden("priv.strake");
    let (status, _stdout, _stderr) = run(&["check", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
}

#[test]
fn priv_call_exits_3_plain() {
    let path = golden("priv_call.strake");
    let (status, _stdout, stderr) = run(&["test", path.to_str().unwrap()]);
    assert_eq!(status.code(), Some(3));
    assert!(!stderr.contains("\"code\""));
}

#[test]
fn check_draft_hole_exits_0_no_e_hole_check() {
    let path = golden("hole_ship.strake");
    let exit = check(path.to_str().unwrap(), Opts { draft: true, ..Opts::default() });
    assert_eq!(exit.code, 0);
    assert!(!exit.stderr.contains("E_HOLE"));
}

#[test]
fn check_dir_reads_only_src_strake() {
    let tmp = std::env::temp_dir().join("strake_t4_check_dir");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src/nested")).expect("mkdir");
    std::fs::create_dir_all(tmp.join("other")).expect("mkdir");
    let good = "strake 1\n\nfn ok() -> i64\n  ret 0\nend\n";
    let bad = "strake 1\n\nfn bad() -> i64\n  x = add i64 1 2\n  x = add i64 x 1\n  ret x\nend\n";
    std::fs::write(tmp.join("src/nested/good.strake"), good).expect("write");
    std::fs::write(tmp.join("other/ignored.strake"), bad).expect("write");
    let (status, _stdout, _stderr) = run(&["check", tmp.to_str().unwrap()]);
    assert_eq!(status.code(), Some(0));
}
