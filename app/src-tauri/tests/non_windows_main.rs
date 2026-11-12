//! Until its platform code lands, the app refuses to start outside Windows.
#![cfg(not(windows))]

#[test]
fn the_binary_points_to_the_cli_and_exits_2() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_redactor-app"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("scrub"));
    assert!(out.stdout.is_empty());
}
