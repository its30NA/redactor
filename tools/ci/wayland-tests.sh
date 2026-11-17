#!/usr/bin/env bash
# The Wayland desktop tests (evidence level L2) on a throwaway compositor and a private D-Bus
# session bus.
#
# The tests are `#[ignore]`d and refuse to run unless REDACTOR_TEST_WAYLAND names the socket of
# the compositor this script started, WAYLAND_DISPLAY is that socket, and XDG_RUNTIME_DIR is the
# script's own folder (marked with a file the tests look for). Nothing here reaches the desktop
# it is started from: on WSLg the inherited WAYLAND_DISPLAY is the Windows clipboard.
#
# The compositor is headless sway (tools/ci/sway.conf); REDACTOR_WAYLAND_COMPOSITOR=kwin starts
# `kwin_wayland --virtual` instead (inside the Plasma container).
#
# Fails unless every listed test binary reports "0 failed" and at least one test passed.
# Lines the tests print as "OBSERVED ..." (what this compositor does where that is not a pass or
# fail, such as the measured write race) are collected at the end, and in the job summary on
# GitHub.
#
# `wayland-tests.sh --run <command...>` runs the command in the same environment instead of the
# tests (from the repository root, with the same compositor, bus and clean-up) and exits with its
# status: for starting the app by hand or a smoke test on the throwaway compositor.
set -euo pipefail
cd "$(dirname "$0")/../.."
# shellcheck source=tools/ci/lib.sh
. tools/ci/lib.sh

# One line per test binary.
tests=(
  wayland_clipboard
)

run=()
if [ "${1:-}" = "--run" ]; then
  shift
  if [ "$#" -eq 0 ]; then
    echo "wayland-tests: --run needs a command" >&2
    exit 2
  fi
  run=("$@")
elif [ "$#" -ne 0 ]; then
  echo "usage: wayland-tests.sh [--run <command...>]" >&2
  exit 2
fi

compositor=${REDACTOR_WAYLAND_COMPOSITOR:-sway}
case "$compositor" in
  sway) compositor_tool=sway ;;
  kwin) compositor_tool=kwin_wayland ;;
  *)
    echo "wayland-tests: REDACTOR_WAYLAND_COMPOSITOR must be sway or kwin" >&2
    exit 2
    ;;
esac

# Everything the script and the tests start, so a missing package is named here and not as a
# test failure half way through.
for tool in "$compositor_tool" wl-copy wl-paste dbus-daemon timeout; do
  if ! command -v "$tool" >/dev/null; then
    echo "wayland-tests: $tool is missing (see CONTRIBUTING.md for the apt line)" >&2
    exit 2
  fi
done

work=$(mktemp -d)
pids=()
cleanup() {
  local p
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

fail() {
  echo "wayland-tests: $1" >&2
  shift
  local log
  for log in "$@"; do show_log "$(basename "$log")" "$log" >&2; done
  exit 1
}

# Nothing of the session this script is started from: no compositor, no X display, no bus.
unset WAYLAND_DISPLAY WAYLAND_SOCKET DISPLAY XAUTHORITY SWAYSOCK DBUS_SESSION_BUS_ADDRESS
mkdir -m 700 "$work/runtime"
export XDG_RUNTIME_DIR="$work/runtime"

dbus-daemon --session --nofork --print-address=3 3>"$work/session.address" >"$work/session.log" 2>&1 &
pids+=($!)
wait_for 10 test -s "$work/session.address" || fail "the private session bus did not start" "$work/session.log"
DBUS_SESSION_BUS_ADDRESS=$(head -n 1 "$work/session.address")
export DBUS_SESSION_BUS_ADDRESS

# The compositor makes its socket in our runtime folder: the only one there.
if [ "$compositor" = sway ]; then
  WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
    sway --config tools/ci/sway.conf >"$work/compositor.log" 2>&1 &
else
  kwin_wayland --virtual --no-lockscreen --socket wayland-redactor >"$work/compositor.log" 2>&1 &
fi
pids+=($!)
wait_for 60 has_socket "$XDG_RUNTIME_DIR" 'wayland-*' ||
  fail "$compositor_tool did not make its socket in 60 s" "$work/compositor.log"
socket=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'wayland-*' -type s -printf '%f\n' | head -n 1)

# The tests check this mark: it says XDG_RUNTIME_DIR is this script's folder.
printf '%s\n' "$socket" >"$XDG_RUNTIME_DIR/redactor-test-runtime"
export REDACTOR_TEST_WAYLAND="$socket"
export WAYLAND_DISPLAY="$socket"
export REDACTOR_TEST_BUS=1
export REDACTOR_WAYLAND_COMPOSITOR="$compositor"
# GTK and the accessibility bus never reach for the real session.
export GDK_BACKEND=wayland NO_AT_BRIDGE=1

if [ "${#run[@]}" -ne 0 ]; then
  set +e
  "${run[@]}"
  status=$?
  set -e
  exit "$status"
fi

args=()
for t in "${tests[@]}"; do args+=(--test "$t"); done
set +e
# --show-output: what a passing test printed (the OBSERVED lines) is shown after the run.
cargo test -p redactor-app --locked "${args[@]}" -- --ignored --test-threads=1 --show-output 2>&1 |
  tee "$work/out.log"
status=${PIPESTATUS[0]}
set -e

observed=$(grep -E '^OBSERVED ' "$work/out.log" | sort | uniq -c || true)
if [ -n "$observed" ]; then
  echo "wayland-tests: observed on $compositor:"
  echo "$observed"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
      echo "### Wayland desktop tests: observed on $compositor"
      echo '```'
      echo "$observed"
      echo '```'
    } >>"$GITHUB_STEP_SUMMARY"
  fi
fi

binaries=$(grep -c '^test result: ' "$work/out.log" || true)
green=$(grep -cE '^test result: ok\. [1-9][0-9]* passed; 0 failed' "$work/out.log" || true)
passed=$(sed -nE 's/^test result: ok\. ([0-9]+) passed; 0 failed.*/\1/p' "$work/out.log" | awk '{ n += $1 } END { print n + 0 }')
if [ "$status" -ne 0 ] || [ "$binaries" -ne "${#tests[@]}" ] || [ "$green" -ne "${#tests[@]}" ]; then
  echo "wayland-tests: FAILED (cargo exit $status, $green of ${#tests[@]} test binaries green)" >&2
  show_log compositor "$work/compositor.log" >&2
  exit 1
fi
echo "wayland-tests: ok, $passed tests in ${#tests[@]} test binaries on $compositor ($socket)"
