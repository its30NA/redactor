#!/usr/bin/env bash
# Windows build loop for the desktop app, run from WSL.
#
# Copies the repo (without target/, node_modules/ and .git/) to C:\dev\redactor and runs the
# Windows toolchain there through powershell.exe. npm's PowerShell shim is blocked by the
# execution policy, so the .cmd forms are used; the policy is never changed.
#
# Usage: tools/win-build.sh [sync|check|test|build|all]   (default: all)
#   sync   copy the repo only
#   check  clippy (app, scrub, wire) and the tests that leave the desktop alone (scrub: its unit
#          tests; its CLI tests need git on PATH): the clipboard
#          tests are #[ignore]d and not run, so the user's clipboard is never touched
#   test   clippy and tests of the app crate on Windows, including the clipboard tests
#          (they use the desktop clipboard and save/restore its text)
#   build  UI build plus `tauri build`: the per-user NSIS installer
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST_WSL="/mnt/c/dev/redactor"
DEST_WIN='C:\dev\redactor'
STEP="${1:-all}"

sync_repo() {
  mkdir -p "$DEST_WSL"
  rsync -a --delete \
    --exclude '/.*/' --exclude 'target/' --exclude 'node_modules/' \
    --exclude 'dist/' --exclude 'dist-mock/' --exclude 'test-results/' \
    --exclude 'playwright-report/' --exclude 'app/src-tauri/gen/' \
    "$ROOT/" "$DEST_WSL/"
}

# Runs PowerShell commands in C:\dev\redactor with a fresh PATH and the repo's Rust version.
# Each command must succeed; the first failure ends the script with its exit code.
win() {
  local body=""
  local cmd
  for cmd in "$@"; do
    body+="$cmd; if (\$LASTEXITCODE -ne 0) { exit \$LASTEXITCODE }; "
  done
  # rust-toolchain.toml pins 1.99; the installed stable toolchain is used when it is 1.99.x, so
  # the build never installs a toolchain on its own.
  (cd "$DEST_WSL" && powershell.exe -NoProfile -NonInteractive -Command "
    \$ErrorActionPreference = 'Stop'
    \$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User')
    Set-Location '$DEST_WIN'
    if ((rustup toolchain list) -notmatch '^1\.99') {
      \$v = (rustc +stable --version)
      if (\$v -notmatch '^rustc 1\.99\.') { Write-Error \"need Rust 1.99, found \$v\"; exit 1 }
      \$env:RUSTUP_TOOLCHAIN = 'stable'
    }
    $body
    exit 0")
}

run_tests() {
  win 'cargo clippy -p redactor-app --all-targets -- -D warnings' \
      'cargo test -p redactor-app' \
      'cargo test -p redactor-app --test win_clipboard -- --ignored --test-threads=1 --nocapture'
}

run_check() {
  win 'cargo clippy -p redactor-app -p scrub -p redactor-wire --all-targets -- -D warnings' \
      'cargo test -p redactor-app -p redactor-wire' \
      'cargo test -p scrub --lib'
}

run_build() {
  win 'Set-Location app\ui' \
      'npm.cmd ci --no-audit --no-fund' \
      'npm.cmd run build' \
      'Set-Location ..\src-tauri' \
      '..\ui\node_modules\.bin\tauri.cmd build --ci --bundles nsis'
  local out="$DEST_WSL/target/release/bundle/nsis"
  ls -l "$out"/*.exe
  sha256sum "$out"/*.exe
}

case "$STEP" in
  sync) sync_repo ;;
  check) sync_repo; run_check ;;
  test) sync_repo; run_tests ;;
  build) sync_repo; run_build ;;
  all) sync_repo; run_tests; run_build ;;
  *) echo "usage: $0 [sync|check|test|build|all]" >&2; exit 2 ;;
esac
