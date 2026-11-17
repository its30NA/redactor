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
# The portal tests (`wayland_portal`) talk to tools/ci/mock_portal.py, a stand-in for the
# desktop's GlobalShortcuts portal on the private bus. They run in two passes: first
# `no_portal_means_command_mode` alone with MOCK_PORTAL=none, before the mock is started (it must
# report exactly 1 passed), then every test with the mock running. Under `--run` the mock runs
# unless MOCK_PORTAL=none. The mock needs python3-gi for /usr/bin/python3; the Plasma run starts
# no mock.
#
# Fails unless every listed test binary reports "0 failed" and at least one test passed. With
# WAYLAND_TESTS_EXPECT=<n> (the CI jobs) it also fails unless exactly n tests passed in them (the
# no-portal pass not counted), so a test that went missing fails the run.
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
  wayland_focused
  wayland_portal
  wayland_webview
)

# The test that needs a bus without a portal; it runs alone, before the mock starts.
no_portal_test=no_portal_means_command_mode

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

# The Plasma run proves the data-control path only: it runs `wayland_clipboard` and nothing
# else. The skips are said in the log and in the job summary.
if [ "$compositor" = kwin ] && [ "${#run[@]}" -eq 0 ]; then
  for t in "${tests[@]}"; do
    case "$t" in
      wayland_clipboard) continue ;;
      # KWin offers data-control, so the app never uses the focused-window clipboard there, and
      # it keeps the virtual keyboard those tests need from ordinary clients.
      wayland_focused) why="data-control is used there, not the focused-window clipboard" ;;
      wayland_portal) why="the mock portal does not depend on the compositor" ;;
      wayland_webview) why="the WebKit settings do not depend on the compositor" ;;
      *) why="the Plasma run lists wayland_clipboard only" ;;
    esac
    line="wayland-tests: skipping $t on kwin ($why)"
    echo "$line"
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then echo "- $line" >>"$GITHUB_STEP_SUMMARY"; fi
  done
  tests=(wayland_clipboard)
fi

# Everything the script and the tests start, so a missing package is named here and not as a
# test failure half way through.
for tool in "$compositor_tool" wl-copy wl-paste dbus-daemon timeout; do
  if ! command -v "$tool" >/dev/null; then
    echo "wayland-tests: $tool is missing (see CONTRIBUTING.md for the apt line)" >&2
    exit 2
  fi
done

# The mock portal: for the portal tests, and under --run unless MOCK_PORTAL=none. Never on kwin.
portal=no
if [ "$compositor" = sway ]; then
  if [ "${#run[@]}" -ne 0 ]; then
    [ "${MOCK_PORTAL:-}" = none ] || portal=yes
  else
    for t in "${tests[@]}"; do [ "$t" = wayland_portal ] && portal=yes; done
  fi
fi
python=/usr/bin/python3
if [ "$portal" = yes ] && ! "$python" -c 'import gi; gi.require_version("Gio", "2.0")' 2>/dev/null; then
  echo "wayland-tests: $python has no GObject bindings (python3-gi), the mock portal needs them" >&2
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

# A session bus that starts no services: without this it would D-Bus-activate the machine's own
# xdg-desktop-portal (and its document portal, a FUSE mount in our runtime folder) on the first
# call to the portal's name. The only portal here is the mock, and only when the script starts it.
cat >"$work/session.conf" <<'CONF'
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
CONF
dbus-daemon --config-file="$work/session.conf" --nofork --print-address=3 3>"$work/session.address" >"$work/session.log" 2>&1 &
pids+=($!)
wait_for 10 test -s "$work/session.address" || fail "the private session bus did not start" "$work/session.log"
DBUS_SESSION_BUS_ADDRESS=$(head -n 1 "$work/session.address")
export DBUS_SESSION_BUS_ADDRESS
# The portal tests and the mock check this mark: it says the session bus is this script's own.
printf '%s\n' "$DBUS_SESSION_BUS_ADDRESS" >"$XDG_RUNTIME_DIR/redactor-test-bus"

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

# start_portal: the mock portal on the private bus, until it owns its name.
start_portal() {
  "$python" tools/ci/mock_portal.py >"$work/portal.log" 2>&1 &
  pids+=($!)
  wait_for 10 grep -qx ready "$work/portal.log" ||
    fail "the mock portal did not start in 10 s" "$work/portal.log"
}

if [ "${#run[@]}" -ne 0 ]; then
  if [ "$portal" = yes ]; then
    start_portal
  fi
  set +e
  "${run[@]}"
  status=$?
  set -e
  exit "$status"
fi

# The script says which pass has no portal; a MOCK_PORTAL from outside never reaches the tests.
unset MOCK_PORTAL
# No portal on the bus: one test, before the mock starts.
skip=()
if [ "$portal" = yes ]; then
  set +e
  MOCK_PORTAL=none cargo test -p redactor-app --locked --test wayland_portal -- \
    --ignored --exact "$no_portal_test" --test-threads=1 --show-output 2>&1 | tee "$work/none.log"
  status=${PIPESTATUS[0]}
  set -e
  if [ "$status" -ne 0 ] || [ "$(grep -c '^test result: ' "$work/none.log" || true)" -ne 1 ] ||
    ! grep -qE '^test result: ok\. 1 passed; 0 failed' "$work/none.log"; then
    echo "wayland-tests: FAILED ($no_portal_test without a portal: cargo exit $status, want exactly 1 passed)" >&2
    exit 1
  fi
  start_portal
  skip=(--skip "$no_portal_test")
fi

args=()
for t in "${tests[@]}"; do args+=(--test "$t"); done
set +e
# --show-output: what a passing test printed (the OBSERVED lines) is shown after the run.
cargo test -p redactor-app --locked "${args[@]}" -- --ignored "${skip[@]}" --test-threads=1 --show-output 2>&1 |
  tee "$work/out.log"
status=${PIPESTATUS[0]}
set -e

observed=$(cat "$work/out.log" "$work/none.log" 2>/dev/null | grep -E '^OBSERVED ' | sort | uniq -c || true)
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
  show_log portal "$work/portal.log" >&2
  exit 1
fi
expect=${WAYLAND_TESTS_EXPECT:-}
if [ -n "$expect" ] && [ "$passed" != "$expect" ]; then
  echo "wayland-tests: FAILED ($passed tests passed, $expect expected: a test went missing or was added without updating the count)" >&2
  exit 1
fi
extra=
if [ "$portal" = yes ]; then
  extra=" (and 1 without a portal)"
fi
echo "wayland-tests: ok, $passed tests$extra in ${#tests[@]} test binaries on $compositor ($socket)"
