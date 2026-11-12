#!/usr/bin/env bash
# P7: can Linux UI flows run through WebDriver? webkit2gtk-driver from apt and tauri-driver
# from crates.io, with their versions.
set -uo pipefail
. "$(dirname "$0")/lib.sh"

missing=$(apt_install webkit2gtk-driver)
if [ -n "$missing" ] || ! command -v WebKitWebDriver >/dev/null; then
  probe_result P7 FAIL "webkit2gtk-driver did not install"
  probe_exit
fi
driver=$(WebKitWebDriver --version 2>&1 | head -n 1)

# Outside the repository, so the runner's own toolchain builds it, not the pinned one.
cd "$(mktemp -d)" || exit 1
if cargo install tauri-driver --locked >install.log 2>&1; then
  tauri=$(cargo install --list | sed -n 's/^tauri-driver \(v[^:]*\):$/\1/p')
  probe_result P7 PASS "WebKitWebDriver: $driver; tauri-driver $tauri"
else
  show_log "cargo install tauri-driver" install.log
  probe_result P7 FAIL "WebKitWebDriver: $driver; tauri-driver did not build"
fi

probe_exit
