#!/usr/bin/env bash
# The desktop tests (evidence level L2) on a throwaway X display and private D-Bus buses.
#
# The tests are `#[ignore]`d and refuse to run unless REDACTOR_TEST_DISPLAY (and for D-Bus
# tests REDACTOR_TEST_BUS) is set. Only this script sets them: it starts its own Xvfb, a
# private session bus, and a private bus with a logind mock that stands in for the system bus.
# Nothing here reaches the desktop it is started from (on WSLg, DISPLAY is the Windows
# clipboard).
#
# Fails unless every listed test binary reports "0 failed" and at least one test passed.
set -euo pipefail
cd "$(dirname "$0")/../.."

tests=(x11_clipboard linux_webview)

for tool in Xvfb xclip dbus-daemon dbus-send timeout; do
  if ! command -v "$tool" >/dev/null; then
    echo "x11-tests: $tool is missing (see CONTRIBUTING.md for the apt line)" >&2
    exit 2
  fi
done
# A pyenv or toolcache python3 does not see the system's python3-dbusmock.
python=python3
"$python" -c 'import dbusmock' 2>/dev/null || python=/usr/bin/python3
if ! "$python" -c 'import dbusmock' 2>/dev/null; then
  echo "x11-tests: python3-dbusmock is missing" >&2
  exit 2
fi

work=$(mktemp -d)
pids=()
cleanup() {
  local p
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

# wait_for <seconds> <command...>: poll every 100 ms until the command succeeds.
wait_for() {
  local tries=$(($1 * 10))
  shift
  while [ "$tries" -gt 0 ]; do
    if "$@" >/dev/null 2>&1; then return 0; fi
    sleep 0.1
    tries=$((tries - 1))
  done
  return 1
}

fail() {
  echo "x11-tests: $1" >&2
  shift
  local log
  for log in "$@"; do
    [ -f "$log" ] && tail -n 20 "$log" >&2
  done
  exit 1
}

# Our own display number, from 99 up, never one that already has a socket or a lock (on WSLg
# ":0" is the Windows desktop). Where /tmp/.X11-unix cannot be written (WSLg mounts it
# read-only) Xvfb listens on its abstract socket only; Xlib, xcb and x11rb try that one first,
# and a client that does not finds no socket file for this number and fails.
listen=()
[ -w /tmp/.X11-unix ] || [ ! -e /tmp/.X11-unix ] || listen=(-nolisten unix)
display=
xvfb=
xvfb_settled() { test -s "$work/display" || ! kill -0 "$xvfb" 2>/dev/null; }
for n in $(seq 99 119); do
  if [ -e "/tmp/.X$n-lock" ] || [ -e "/tmp/.X11-unix/X$n" ]; then continue; fi
  # Xvfb writes the number to fd 3 once it accepts clients. -noreset: an X server starts over
  # when its last client leaves, and resets connections made in that moment (between two tests).
  Xvfb ":$n" -displayfd 3 -screen 0 1280x800x24 -noreset -nolisten tcp "${listen[@]}" \
    3>"$work/display" >"$work/xvfb.log" 2>&1 &
  xvfb=$!
  wait_for 15 xvfb_settled || true
  if [ -s "$work/display" ]; then
    display=":$n"
    pids+=("$xvfb")
    break
  fi
  kill "$xvfb" 2>/dev/null || true
done
[ -n "$display" ] || fail "Xvfb did not start" "$work/xvfb.log"

start_bus() {
  dbus-daemon --session --nofork --print-address=3 3>"$work/$1.address" >"$work/$1.log" 2>&1 &
  pids+=($!)
  wait_for 10 test -s "$work/$1.address" || fail "the private $1 bus did not start" "$work/$1.log"
  head -n 1 "$work/$1.address"
}
start_bus session >/dev/null
start_bus system >/dev/null
DBUS_SESSION_BUS_ADDRESS=$(head -n 1 "$work/session.address")
DBUS_SYSTEM_BUS_ADDRESS=$(head -n 1 "$work/system.address")
export DBUS_SESSION_BUS_ADDRESS DBUS_SYSTEM_BUS_ADDRESS

"$python" -m dbusmock --system --template logind >"$work/logind.log" 2>&1 &
pids+=($!)
has_logind() {
  dbus-send --system --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus \
    org.freedesktop.DBus.NameHasOwner string:org.freedesktop.login1 | grep -q 'boolean true'
}
wait_for 15 has_logind || fail "the logind mock did not appear on the bus" "$work/logind.log"

export REDACTOR_TEST_DISPLAY="$display"
export REDACTOR_TEST_BUS=1
# Tools that fall back to the environment get the throwaway display too, never the real one.
export DISPLAY="$display"
unset WAYLAND_DISPLAY XAUTHORITY

args=()
for t in "${tests[@]}"; do args+=(--test "$t"); done
set +e
cargo test -p redactor-app --locked "${args[@]}" -- --ignored --test-threads=1 2>&1 | tee "$work/out.log"
status=${PIPESTATUS[0]}
set -e

binaries=$(grep -c '^test result: ' "$work/out.log" || true)
green=$(grep -cE '^test result: ok\. [1-9][0-9]* passed; 0 failed' "$work/out.log" || true)
passed=$(sed -nE 's/^test result: ok\. ([0-9]+) passed; 0 failed.*/\1/p' "$work/out.log" | awk '{ n += $1 } END { print n + 0 }')
if [ "$status" -ne 0 ] || [ "$binaries" -ne "${#tests[@]}" ] || [ "$green" -ne "${#tests[@]}" ]; then
  echo "x11-tests: FAILED (cargo exit $status, $green of ${#tests[@]} test binaries green)" >&2
  exit 1
fi
echo "x11-tests: ok, $passed tests in ${#tests[@]} test binaries on display $display"
