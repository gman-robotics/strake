//! GMA-481 (P2-T5): region `load`/`store`, page count, and the §2.17 bool-load stop.
use std::path::PathBuf;
use std::process::Command;

fn run_src(name: &str, src: &str) -> (Option<i32>, String) {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.strake"));
    std::fs::write(&path, src).expect("write");
    let out = Command::new(env!("CARGO_BIN_EXE_strake"))
        .args(["test", path.to_str().unwrap()])
        .output()
        .expect("run strake");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn region_test(name: &str, header: &str, body: &str) -> (Option<i32>, String) {
    let src = format!(
        "strake 1\n{header}\nimport region mem : region\n\ntest {name}\n{body}  ret 0\nend\n"
    );
    run_src(name, &src)
}

#[test]
fn bool_load_of_byte_2_stops_exit_3_plain_text() {
    // diag::status only yields 0, 1, or 2, so exit 3 means it was not called.
    let (code, stderr) = region_test("bool_two", "", "  s = store i32 0 2\n  b = load bool 0\n");
    assert_eq!(code, Some(3), "{stderr}");
    assert_eq!(stderr, "bool load of a byte other than 0 or 1\n");
    assert!(!stderr.contains("\"code\""));
}

#[test]
fn bool_load_of_bytes_0_and_1() {
    let body = "  f = load bool 5\n  assert.eq f false\n  s = store i32 5 1\n  t = load bool 5\n  assert.eq t true\n  u = store bool 6 true\n  w = load i32 5\n  assert.eq w 257\n";
    let (code, stderr) = region_test("bool_ok", "", body);
    assert_eq!(code, Some(0), "{stderr}");
}

#[test]
fn i64_store_i32_loads_little_endian() {
    let body = "  c = store i64 40 258\n  lo = load i32 40\n  assert.eq lo 258\n  hi = load i32 41\n  assert.eq hi 1\n";
    let (code, stderr) = region_test("float_rt", "", body);
    assert_eq!(code, Some(0), "{stderr}");
}

#[test]
fn page_directive_sizes_the_region() {
    let last = "  v = load i64 65528\n";
    let (code, stderr) = region_test("pages_one_ok", "pages 1\n", last);
    assert_eq!(code, Some(0), "{stderr}");
    let past = "  v = load i64 65529\n";
    let (code, stderr) = region_test("pages_one_oob", "pages 1\n", past);
    assert_eq!(code, Some(2), "{stderr}");
    assert!(stderr.contains("\"E_OOB\""));
}

#[test]
fn default_region_is_16_pages() {
    let (code, stderr) = region_test("pages_default", "", "  v = load i64 1048568\n");
    assert_eq!(code, Some(0), "{stderr}");
    let (code, stderr) = region_test("pages_default_oob", "", "  v = load bool 1048576\n");
    assert_eq!(code, Some(2), "{stderr}");
    assert!(stderr.contains("\"E_OOB\""));
}

#[test]
fn region_check_errors_are_e_type() {
    for (name, body) in [
        ("store_mismatch", "  s = store bool 0 7\n"),
        ("load_unit", "  v = load unit 0\n"),
        ("addr_bool", "  v = load i64 true\n"),
    ] {
        let (code, stderr) = region_test(name, "", body);
        assert_eq!(code, Some(1), "{name}: {stderr}");
        assert!(stderr.contains("\"E_TYPE\""), "{name}: {stderr}");
    }
}
