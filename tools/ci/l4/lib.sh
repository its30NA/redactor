#!/usr/bin/env bash
# Shared by the package checks (L4) in tools/ci/l4.
#
# Sourced: `pass <id>` and `fail <id> <reason>` print one result line each (`PASS <id>`,
# `FAIL <id> <reason>`); a FAIL sets l4_failed=1, the caller's exit status. tools/ci/lib.sh
# comes along (wait_for, remove_work).
#
# Run: `tools/ci/l4/lib.sh app-version` prints the app version from tauri.conf.json, read with
# sed as tools/release/check-version.sh reads it (the clean containers have no jq).
l4_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)

l4_app_version() {
  sed -n 's/^[[:space:]]*"version":[[:space:]]*"\([^"]*\)".*/\1/p' \
    "$l4_root/app/src-tauri/tauri.conf.json" | head -n 1
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  set -euo pipefail
  case "${1:-}" in
    app-version)
      v=$(l4_app_version)
      [ -n "$v" ] || {
        echo "lib.sh: no version in app/src-tauri/tauri.conf.json" >&2
        exit 1
      }
      printf '%s\n' "$v"
      ;;
    *)
      echo "usage: $0 app-version" >&2
      exit 2
      ;;
  esac
  exit 0
fi

# shellcheck source=tools/ci/lib.sh
. "$l4_root/tools/ci/lib.sh"

# shellcheck disable=SC2034 # the sourcing script exits with it
l4_failed=0
pass() { echo "PASS $1"; }
fail() {
  local id=$1
  shift
  echo "FAIL $id $*"
  # shellcheck disable=SC2034 # the sourcing script exits with it
  l4_failed=1
}
