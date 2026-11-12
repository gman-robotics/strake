//! `strake check` is exit 70 only for an exact known-fail set.
//!
//! Exit 70 is the lower/SSA abort (`lower: div type`). Check diagnostics stay
//! exit 1 or 3. A known-fail id that starts to pass no longer belongs in the list.

use std::fs;
use std::path::{Path, PathBuf};

use strake::{check, Opts};

const OPS: &[&str] = &[
    "add", "sub", "mul", "div", "eq", "ne", "lt", "le", "gt", "ge",
];

const LOWER_ABORT_STDERR: &str = "lower: div type\n";

/// Exact ids whose check exits 70. Any other id with that exit fails the test.
const KNOWN_LOWER_ABORTS: &[&str] = &[
    // GMA-617 (https://linear.app/gman-personal-projects/issue/GMA-617)
    "lit-div-bool",
    // GMA-617 (https://linear.app/gman-personal-projects/issue/GMA-617)
    "named-div-bool",
    // GMA-617 (https://linear.app/gman-personal-projects/issue/GMA-617)
    "lit-div-unit",
    // GMA-617 (https://linear.app/gman-personal-projects/issue/GMA-617)
    "named-div-unit",
    // GMA-617 (https://linear.app/gman-personal-projects/issue/GMA-617)
    "bytes-div",
    // GMA-617 (https://linear.app/gman-personal-projects/issue/GMA-617)
    "record-div",
];

fn golden_paths() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../golden");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("golden dir")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "strake"))
        .collect();
    paths.sort();
    paths
}

fn check_path(path: &Path) -> strake::Exit {
    check(path.to_str().expect("utf8 path"), Opts::default())
}

#[test]
fn golden_check_exit_is_not_lower_abort() {
    let paths = golden_paths();
    // 130 on GMA-489, plus 9 GMA-497 goldens. None of those nine exit 70:
    // dup_record, record_project, result_unit_join, result_i64_join,
    // loop_exit_record, loop_exit_bytes, match_arm_records, match_arm_project,
    // match_arm_agree.
    assert_eq!(paths.len(), 139, "golden corpus");
    for path in &paths {
        let exit = check_path(path);
        assert_ne!(
            exit.code,
            70,
            "{} exited 70\n{}",
            path.display(),
            exit.stderr
        );
    }
}

fn lit_src(op: &str, ty: &str, lhs: &str, rhs: &str) -> String {
    format!("strake 1\n\ntest t\n  c = {op} {ty} {lhs} {rhs}\n  ret 0\nend\n")
}

fn named_src(op: &str, ty: &str) -> String {
    format!(
        "strake 1\n\nfn f(a: {ty}, b: {ty}) -> i64\n  c = {op} {ty} a b\n  ret 0\nend\n\ntest t\n  ret 0\nend\n"
    )
}

fn bytes_src(op: &str) -> String {
    format!(
        "strake 1\n\ntest t\n  a = bytes.new x\"00\"\n  b = bytes.new x\"01\"\n  c = {op} bytes a b\n  ret 0\nend\n"
    )
}

fn record_src(op: &str) -> String {
    format!(
        "strake 1\n\ntype A {{ n: i64 }}\n\nfn f(a: A, b: A) -> i64\n  c = {op} A a b\n  ret 0\nend\n\ntest t\n  ret 0\nend\n"
    )
}

fn scalar_pairs() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("i64", "1", "2"),
        ("i32", "1", "2"),
        ("f64", "1.0", "2.0"),
        ("f32", "0.25", "0.5"),
        ("bool", "true", "false"),
        ("unit", "unit", "unit"),
    ]
}

fn probes() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for op in OPS {
        for (ty, lhs, rhs) in scalar_pairs() {
            out.push((format!("lit-{op}-{ty}"), lit_src(op, ty, lhs, rhs)));
            out.push((format!("named-{op}-{ty}"), named_src(op, ty)));
        }
        out.push((format!("bytes-{op}"), bytes_src(op)));
        out.push((format!("record-{op}"), record_src(op)));
    }
    out
}

fn write_probe(dir: &Path, id: &str, src: &str) -> PathBuf {
    let path = dir.join(format!("{id}.strake"));
    fs::write(&path, src).expect("write probe");
    path
}

fn known_ids() -> Vec<String> {
    KNOWN_LOWER_ABORTS
        .iter()
        .map(|id| (*id).to_string())
        .collect()
}

#[test]
fn probe_lower_abort_set_matches_known_list() {
    let dir = std::env::temp_dir().join("strake_gma489_probes");
    fs::create_dir_all(&dir).expect("probe dir");
    let mut aborts = Vec::new();
    for (id, src) in probes() {
        let path = write_probe(&dir, &id, &src);
        let exit = check_path(&path);
        if exit.code == 70 {
            assert_eq!(exit.stderr, LOWER_ABORT_STDERR, "{id}");
            aborts.push(id);
        }
    }
    aborts.sort();
    let mut expect = known_ids();
    expect.sort();
    assert_eq!(aborts, expect);
}
