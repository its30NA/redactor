//! Where the desktop app cannot run, it says why, points to the `scrub` CLI and exits 2.
#![cfg(not(windows))]

use std::process::{Command, Output};

/// The app binary with no desktop session in its environment, plus `vars`.
fn run_app(vars: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_redactor-app"));
    cmd.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
    for (k, v) in vars {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

#[test]
fn no_display_points_to_the_cli_and_exits_2() {
    let out = run_app(&[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("scrub"));
    assert!(out.stdout.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn wayland_is_refused_by_name() {
    let out = run_app(&[("WAYLAND_DISPLAY", "wayland-test")]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Wayland"));
    assert!(out.stdout.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn an_unreachable_x_server_is_refused() {
    let out = run_app(&[("DISPLAY", ":987")]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("X server"));
    assert!(out.stdout.is_empty());
}
