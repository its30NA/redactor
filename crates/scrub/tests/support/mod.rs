//! Shared test helpers.
#![allow(dead_code)]

pub mod gen;

use assert_cmd::Command;
use std::path::Path;

/// A `scrub` invocation in `dir` (never inside the repo, so no stray config is discovered).
pub fn scrub_in(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("scrub").unwrap();
    cmd.current_dir(dir);
    cmd
}

/// Runs `scrub` with stdin and returns (stdout, stderr, exit code).
pub fn run(dir: &Path, args: &[&str], stdin: &str) -> (String, String, i32) {
    let out = scrub_in(dir)
        .args(args)
        .write_stdin(stdin)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}
