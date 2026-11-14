use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_strake"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run strake");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn blob_pkg_hash() -> String {
    let bytes = fs::read(root().join("golden/blob/pkg")).expect("read golden/blob/pkg");
    blake3::hash(&bytes).to_hex().to_string()
}

#[test]
fn use_ok_exit_0_empty_stderr() {
    let path = root().join("golden/use_ok.strake");
    let (code, stdout, stderr) = run(&[
        "check",
        "--blob-root",
        "golden/blob",
        path.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

#[test]
fn use_bad_missing_blob_exit_1_e_use() {
    let path = root().join("golden/use_bad.strake");
    let (code, _stdout, stderr) = run(&[
        "check",
        "--blob-root",
        "golden/blob-empty",
        path.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert!(stderr.contains("E_USE"));
    assert!(stderr.contains("\"kind\":\"check\""));
}

#[test]
fn use_mismatch_blob_bad_exit_1_e_use() {
    let path = root().join("golden/use_ok.strake");
    let (code, _stdout, stderr) = run(&[
        "check",
        "--blob-root",
        "golden/blob-bad",
        path.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert!(stderr.contains("E_USE"));
    assert!(stderr.contains("\"kind\":\"check\""));
}

#[test]
fn use_path_escape_exit_1_e_use() {
    let src = concat!(
        "strake 1\n\n",
        "use pkg.sub@blake3:deadbeef\n\n",
        "fn f() -> unit\n  ret unit\nend\n"
    );
    let path = std::env::temp_dir().join("strake_use_path_escape.strake");
    fs::write(&path, src).expect("write escape fixture");
    let (code, _stdout, stderr) = run(&[
        "check",
        "--blob-root",
        "golden/blob",
        path.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert!(stderr.contains("E_USE"));
    assert!(stderr.contains("\"kind\":\"check\""));
}

#[test]
fn no_use_regression_add2_basic_exit_0() {
    let path = root().join("golden/add2_basic.strake");
    let (code, stdout, stderr) = run(&["check", path.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

#[test]
fn blob_pkg_hash_matches_fixture() {
    assert_eq!(
        blob_pkg_hash(),
        "48c37268d0995bdaee1c79d7da54a7c9015408fa14088d3958a458e666d44eeb"
    );
}
