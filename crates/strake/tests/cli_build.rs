use std::process::Command;

#[test]
fn strake_build_black_box() {
    let bin = env!("CARGO_BIN_EXE_strake");
    let output = Command::new(bin)
        .arg("build")
        .output()
        .expect("run strake build");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "build is post-MVP\n"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("code"));
}
