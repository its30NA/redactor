//! Where the desktop app cannot run, it says why, points to the `scrub` CLI and exits 2.
//! Linux only: on macOS the binary starts the app, and these tests would wait for it.
#![cfg(target_os = "linux")]

use std::process::{Command, Output};

/// The app binary with no desktop session in its environment, plus `vars`.
fn run_app(vars: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_redactor-app"));
    cmd.env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET");
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

#[test]
fn an_unreachable_wayland_compositor_is_refused() {
    // An empty runtime folder of its own: no socket by that name can exist, so the app never
    // reaches the desktop this runs on.
    let runtime = std::env::temp_dir().join(format!("redactor-no-wayland-{}", std::process::id()));
    std::fs::create_dir_all(&runtime).unwrap();
    let out = run_app(&[
        ("XDG_RUNTIME_DIR", runtime.to_str().unwrap()),
        ("WAYLAND_DISPLAY", "wayland-test"),
    ]);
    let _ = std::fs::remove_dir(&runtime);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Wayland compositor"));
    assert!(out.stdout.is_empty());
}

#[test]
fn an_unreachable_x_server_is_refused() {
    let out = run_app(&[("DISPLAY", ":987")]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("X server"));
    assert!(out.stdout.is_empty());
}
