//! `scrub install-hook`: a pre-commit hook that blocks commits containing secrets.
//!
//! The hook shells back into `scrub check`, which scans the staged blobs and exits non-zero if
//! anything sensitive is found.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::cli::HookArgs;
use crate::CliError;

pub const HOOK_SCRIPT: &str = "\
#!/bin/sh
# Installed by `scrub install-hook`. Blocks commits that contain secrets.
# Bypass in an emergency with `git commit --no-verify`.
scrub check || {
    echo \"redactor: commit blocked — sensitive data found in staged files, or a staged file could not be checked.\" >&2
    echo \"Review the findings above, or bypass with 'git commit --no-verify'.\" >&2
    exit 1
}
";

fn hooks_dir() -> Result<PathBuf, String> {
    let out = Command::new("git")
        .args(["rev-parse", "--git-path", "hooks"])
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

pub fn run(args: &HookArgs) -> Result<i32, CliError> {
    let fail = |why: String| CliError::new(2, format!("could not install hook: {why}"));
    let hooks = hooks_dir().map_err(fail)?;
    fs::create_dir_all(&hooks).map_err(|e| fail(e.to_string()))?;
    let path = hooks.join("pre-commit");
    if path.exists() && !args.force {
        return Err(CliError::new(
            1,
            format!(
                "{} already exists; re-run with --force to overwrite",
                path.display()
            ),
        ));
    }
    fs::write(&path, HOOK_SCRIPT).map_err(|e| fail(e.to_string()))?;
    make_executable(&path).map_err(|e| fail(e.to_string()))?;
    eprintln!("Installed pre-commit hook at {}", path.display());
    Ok(0)
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}
