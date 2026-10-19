use std::process::Command;

#[test]
fn main_and_lib_share_check_usage() {
    let main_src = include_str!("../src/main.rs");
    assert!(
        main_src.contains("use strake::") && main_src.contains("CHECK_USAGE"),
        "main.rs must import CHECK_USAGE from the library"
    );
    assert_eq!(
        strake::CHECK_USAGE,
        "usage: strake check [--draft] [--fn NAME] [--blob-root DIR] <file|dir>"
    );
    assert_eq!(
        strake::check_usage_stderr(),
        format!("{}\n", strake::CHECK_USAGE)
    );

    let bin = env!("CARGO_BIN_EXE_strake");
    let out = Command::new(bin).arg("check").output().expect("run strake");
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        strake::check_usage_stderr()
    );
}
