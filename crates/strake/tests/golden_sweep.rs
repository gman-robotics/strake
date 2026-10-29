//! Golden regression harness (Phase 2 T1).
//!
//! Every `golden/*.strake` file either has a checked-in expectation at
//! `golden/expect/<stem>.txt` or is named in `UNIT_ONLY`. The harness runs the
//! command from the expectation's `args:` line from the repo root and compares
//! the rendered exit code, stdout and stderr bytes to the file.
//!
//! Expectation format (the whole file is compared byte for byte):
//!
//! ```text
//! args: test golden/<stem>.strake
//! exit: <code>
//! --- stdout
//! <stdout bytes>
//! --- stderr
//! <stderr bytes>
//! ```
//!
//! Rule for T2–T8: add an expectation for every new golden with a CLI
//! command; add goldens with no CLI oracle to `UNIT_ONLY` instead.
//! To write missing expectations for new goldens, run with
//! `STRAKE_GOLDEN_BLESS=1`; existing files are never overwritten.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Goldens with no CLI oracle. They are covered by unit tests and must not
/// have an expectation file. `trap_user`: Phase 1 §2.7 locks only the lowered
/// shape, not a CLI exit (the golden itself lands in T6).
const UNIT_ONLY: &[&str] = &["trap_user"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn goldens(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(root.join("golden"))
        .expect("read golden/")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "strake"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

fn render(root: &Path, args_line: &str) -> String {
    let args: Vec<&str> = args_line.split_whitespace().collect();
    let out = Command::new(env!("CARGO_BIN_EXE_strake"))
        .args(&args)
        .current_dir(root)
        .output()
        .expect("run strake");
    let exit = match out.status.code() {
        Some(c) => c.to_string(),
        None => "signal".to_string(),
    };
    format!(
        "args: {}\nexit: {}\n--- stdout\n{}--- stderr\n{}",
        args_line,
        exit,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Returns one message per failing golden.
fn sweep(root: &Path, bless: bool) -> Vec<String> {
    let mut failures = Vec::new();
    let names = goldens(root);
    for name in &names {
        let expect = root.join("golden/expect").join(format!("{name}.txt"));
        let unit_only = UNIT_ONLY.contains(&name.as_str());
        match (expect.exists(), unit_only) {
            (true, true) => failures.push(format!(
                "{name}: on UNIT_ONLY but has expectation {}",
                expect.display()
            )),
            (false, true) => {}
            (false, false) => {
                if bless {
                    let line = format!("test golden/{name}.strake");
                    fs::write(&expect, render(root, &line)).expect("write expectation");
                } else {
                    failures.push(format!(
                        "{name}: no expectation at {} and not on UNIT_ONLY",
                        expect.display()
                    ));
                }
            }
            (true, false) => {
                let want = fs::read_to_string(&expect).expect("read expectation");
                let Some(args_line) = want.lines().next().and_then(|l| l.strip_prefix("args: "))
                else {
                    failures.push(format!("{name}: expectation lacks `args:` first line"));
                    continue;
                };
                let got = render(root, args_line);
                if got != want {
                    failures.push(format!(
                        "{name}: output differs\n--- want\n{want}\n--- got\n{got}"
                    ));
                }
            }
        }
    }
    // Stale expectations (no matching golden) also fail.
    for e in fs::read_dir(root.join("golden/expect")).expect("read golden/expect") {
        let p = e.expect("dir entry").path();
        let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
        if p.extension().is_some_and(|x| x == "txt") && !names.contains(&stem) {
            failures.push(format!("{stem}: expectation has no golden"));
        }
    }
    failures
}

#[test]
fn every_golden_matches_its_expectation() {
    let bless = std::env::var_os("STRAKE_GOLDEN_BLESS").is_some();
    let failures = sweep(&root(), bless);
    assert!(
        failures.is_empty(),
        "{} golden failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Copies `golden/` into a scratch root so mutations never touch the repo.
fn scratch_copy(tag: &str) -> PathBuf {
    let dst = std::env::temp_dir().join(format!("strake-sweep-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dst);
    fs::create_dir_all(dst.join("golden/expect")).unwrap();
    let src = root();
    for sub in ["golden", "golden/expect"] {
        for e in fs::read_dir(src.join(sub)).unwrap() {
            let p = e.unwrap().path();
            if p.is_file() {
                fs::copy(&p, dst.join(sub).join(p.file_name().unwrap())).unwrap();
            }
        }
    }
    dst
}

#[test]
fn deleting_an_expectation_fails() {
    let dst = scratch_copy("delete");
    fs::remove_file(dst.join("golden/expect/add2_basic.txt")).unwrap();
    let f = sweep(&dst, false);
    assert!(
        f.iter()
            .any(|m| m.starts_with("add2_basic: no expectation")),
        "{f:?}"
    );
    fs::remove_dir_all(&dst).unwrap();
}

#[test]
fn golden_without_expectation_or_unit_entry_fails() {
    let dst = scratch_copy("new");
    fs::copy(
        dst.join("golden/add2_basic.strake"),
        dst.join("golden/zz_new.strake"),
    )
    .unwrap();
    let f = sweep(&dst, false);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].starts_with("zz_new: no expectation"), "{f:?}");
    fs::remove_dir_all(&dst).unwrap();
}

#[test]
fn wrong_expectation_fails() {
    let dst = scratch_copy("wrong");
    let p = dst.join("golden/expect/div0.txt");
    let want = fs::read_to_string(&p).unwrap();
    fs::write(&p, want.replace("exit: 2\n", "exit: 0\n")).unwrap();
    let f = sweep(&dst, false);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].starts_with("div0: output differs"), "{f:?}");
    fs::remove_dir_all(&dst).unwrap();
}

#[test]
fn unit_only_golden_with_expectation_fails() {
    let dst = scratch_copy("unit");
    fs::copy(
        dst.join("golden/add2_basic.strake"),
        dst.join("golden/trap_user.strake"),
    )
    .unwrap();
    fs::copy(
        dst.join("golden/expect/add2_basic.txt"),
        dst.join("golden/expect/trap_user.txt"),
    )
    .unwrap();
    let f = sweep(&dst, false);
    assert!(
        f.iter().any(|m| m.starts_with("trap_user: on UNIT_ONLY")),
        "{f:?}"
    );
    fs::remove_file(dst.join("golden/expect/trap_user.txt")).unwrap();
    assert!(sweep(&dst, false).is_empty());
    fs::remove_dir_all(&dst).unwrap();
}
