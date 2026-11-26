#!/usr/bin/env bash
# The L4 start check: an installed or unpacked package starts on a clean system.
#
# Usage: tools/ci/l4/start.sh [--print-env] <expected-version> <command...>
#   <command...> starts the app, as tools/ci/l4/install.sh prints it, for example
#   `/usr/bin/Redactor`, `./Redactor_2.1.1_amd64.AppImage --appimage-extract-and-run` or
#   `flatpak run io.github.its30na.redactor`.
#
# One line per check, `PASS <id>` or `FAIL <id> <reason>`; exit 0 only when every line is PASS:
#   version                   `<command> --version` prints `Redactor <expected-version>` and
#                             exits 0 within 10 s, with no display and no session bus.
#   window                    a window named Redactor shows within 30 s and the app is still
#                             running 5 s later.
#   second_launch_hands_over  `<command> --scrub-clipboard` exits 0 within 10 s, the first app
#                             is still running, and there is still one Redactor window.
#   quits                     SIGTERM to the app's process group: every process of it is gone
#                             within 10 s, and nothing is left in XDG_RUNTIME_DIR.
# With ImageMagick's `import`, a screenshot goes to smoke-out/l4-<label>.png (L4_LABEL, by
# default the command's file name).
#
# It never touches the desktop it is started from (on WSLg DISPLAY=:0 and WAYLAND_DISPLAY are
# the Windows screen and clipboard): its own Xvfb on the lowest free display from :90, its own
# session bus (dbus-run-session), no WAYLAND_DISPLAY, and HOME, XDG_RUNTIME_DIR (mode 700),
# XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME, XDG_STATE_HOME and TMPDIR in a folder of its own
# (mktemp -d), removed at exit. `--print-env` prints that environment (L4_WORK is the folder)
# from inside the bus and exits without starting anything else; tools/ci/l4/selftest.sh checks it.
#
# Needs: Xvfb, xdotool, dbus-run-session, timeout, setsid (tools/ci/l4/install.sh installs them).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=tools/ci/l4/lib.sh
. "$here/lib.sh"

usage() {
  echo "usage: $0 [--print-env] <expected-version> <command...>" >&2
  exit 2
}

# ---------------------------------------------------------------------------------------------
# Inside the private bus (the script runs itself under dbus-run-session).
if [ "${1:-}" = "--inside-bus" ]; then
  shift
  mode=$1 version=$2
  shift 2
  if [ "$mode" = print-env ]; then
    for name in L4_WORK HOME TMPDIR DISPLAY WAYLAND_DISPLAY WAYLAND_SOCKET XAUTHORITY XDG_RUNTIME_DIR \
      XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME XDG_STATE_HOME DBUS_SESSION_BUS_ADDRESS GDK_BACKEND; do
      if [ -n "${!name+x}" ]; then printf '%s=%s\n' "$name" "${!name}"; fi
    done
    exit 0
  fi
  cmd=("$@")
  work=$L4_WORK

  # windows: the ids of the visible windows named Redactor, one per line.
  windows() { xdotool search --onlyvisible --name '^Redactor$' 2>/dev/null || true; }
  one_window() { [ "$(windows | grep -c .)" -ge 1 ]; }
  alive() { kill -0 "$1" 2>/dev/null; }
  # session_pids <sid>: the live processes of a session, from /proc (no procps in the containers).
  # A zombie (state Z) has exited and is skipped: in a CI job container PID 1 is `tail -f
  # /dev/null`, which never reaps the web view's helpers once the app, their parent, is gone.
  session_pids() {
    local stat rest f
    for stat in /proc/[0-9]*/stat; do
      rest=$(cat "$stat" 2>/dev/null) || continue
      # After the command name, which may hold spaces: state ppid pgrp session ...
      read -r -a f <<<"${rest##*) }"
      if [ "${f[3]:-}" = "$1" ] && [ "${f[0]:-}" != Z ]; then
        stat=${stat#/proc/}
        echo "${stat%/stat}"
      fi
    done
  }
  # shellcheck disable=SC2329 # called through wait_for
  none_left() { [ -z "$(session_pids "$1")" ]; }

  # window: the app runs in a session of its own (setsid), so its process group holds it, the
  # AppImage runtime or flatpak wrapper in front of it, and the web view's helper processes.
  setsid "${cmd[@]}" >"$work/app.log" 2>&1 </dev/null &
  app=$!
  echo "$app" >"$work/app.pid"
  if ! wait_for 30 one_window; then
    if alive "$app"; then
      fail window "no visible window named Redactor within 30 s"
    else
      fail window "the app exited before a window showed: $(tail -n 1 "$work/app.log" 2>/dev/null)"
    fi
  else
    sleep 5
    if ! alive "$app"; then
      fail window "the window showed, but the app exited within 5 s: $(tail -n 1 "$work/app.log")"
    elif ! one_window; then
      fail window "the window showed, but was gone 5 s later"
    else
      pass window
    fi
    if command -v import >/dev/null 2>&1; then
      mkdir -p "$l4_root/smoke-out"
      label=${L4_LABEL:-$(basename "${cmd[0]}")}
      import -window root "$l4_root/smoke-out/l4-$label.png" 2>/dev/null || true
    fi
  fi

  # second_launch_hands_over: a second start hands --scrub-clipboard to the first and exits.
  if ! alive "$app"; then
    fail second_launch_hands_over "the first app is not running"
  else
    set +e
    timeout 10 "${cmd[@]}" --scrub-clipboard >"$work/second.log" 2>&1 </dev/null
    code=$?
    set -e
    count=$(windows | grep -c . || true)
    if [ "$code" -eq 124 ]; then
      fail second_launch_hands_over "the second start was still running after 10 s"
    elif [ "$code" -ne 0 ]; then
      fail second_launch_hands_over "the second start exited $code: $(tail -n 1 "$work/second.log")"
    elif ! alive "$app"; then
      fail second_launch_hands_over "the first app exited"
    elif [ "$count" -ne 1 ]; then
      fail second_launch_hands_over "$count windows named Redactor, expected 1"
    else
      pass second_launch_hands_over
    fi
  fi

  # quits: SIGTERM to the whole process group, as a session's end sends it.
  if ! alive "$app"; then
    fail quits "the app was not running"
  elif ! kill -TERM -- "-$app" 2>/dev/null; then
    fail quits "SIGTERM could not be sent to process group $app"
  elif ! wait_for 10 none_left "$app"; then
    fail quits "still running 10 s after SIGTERM: $(for p in $(session_pids "$app"); do
      printf '%s(%s) ' "$p" "$(cat "/proc/$p/comm" 2>/dev/null || echo gone)"
    done)"
    kill -KILL -- "-$app" 2>/dev/null || true
  else
    # The desktop's own services, started on the bus by the libraries the window loads, keep
    # their folders there (named one by one, so anything else still counts): the document
    # portal (doc), GSettings (dconf), D-Bus activation (dbus-1) and the accessibility bus.
    # Flatpak keeps its own beside them: the instance records (.flatpak), the session helper
    # (.flatpak-helper) and the app's shared runtime folder (app/<app-id>).
    keep=(! -name doc ! -name dconf ! -name dbus-1 ! -name at-spi)
    for word in "${cmd[@]}"; do
      if [ "$(basename -- "$word")" = flatpak ]; then
        keep+=(! -name .flatpak ! -name .flatpak-helper ! -name app)
        break
      fi
    done
    left=$(find "$XDG_RUNTIME_DIR" -mindepth 1 -maxdepth 1 "${keep[@]}" -printf '%f ' 2>/dev/null || true)
    if [ -n "$left" ]; then
      fail quits "left in XDG_RUNTIME_DIR: $left"
    else
      pass quits
    fi
  fi
  exit "$l4_failed"
fi

# ---------------------------------------------------------------------------------------------
# Outside: the environment, the version check, the display, then the rest inside the bus.
print_env=0
if [ "${1:-}" = "--print-env" ]; then
  print_env=1
  shift
fi
[ "$#" -ge 2 ] || usage
version=$1
shift
cmd=("$@")

for tool in dbus-run-session timeout setsid; do
  command -v "$tool" >/dev/null || {
    echo "start.sh: $tool is missing" >&2
    exit 2
  }
done
if [ "$print_env" -eq 0 ]; then
  for tool in Xvfb xdotool; do
    command -v "$tool" >/dev/null || {
      echo "start.sh: $tool is missing" >&2
      exit 2
    }
  done
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/redactor-l4.XXXXXX")
xvfb=
# shellcheck disable=SC2329 # the EXIT trap
cleanup() {
  local pid
  if [ -s "$work/app.pid" ]; then
    pid=$(cat "$work/app.pid")
    kill -KILL -- "-$pid" 2>/dev/null || true
  fi
  if [ -n "$xvfb" ]; then kill "$xvfb" 2>/dev/null || true; fi
  remove_work "$work"
}
trap cleanup EXIT

export L4_WORK=$work
export HOME=$work/home
export XDG_RUNTIME_DIR=$work/runtime
export XDG_CONFIG_HOME=$work/config XDG_DATA_HOME=$work/data XDG_CACHE_HOME=$work/cache
export XDG_STATE_HOME=$work/state
# The AppImage runtime unpacks into TMPDIR (--appimage-extract-and-run) and leaves the copy there
# when its process group is stopped.
export TMPDIR=$work/tmp
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$TMPDIR"
mkdir -m 700 "$XDG_RUNTIME_DIR"
unset WAYLAND_DISPLAY WAYLAND_SOCKET XAUTHORITY DBUS_SESSION_BUS_ADDRESS DISPLAY
# GTK finds a Wayland compositor without WAYLAND_DISPLAY too, at the default socket in
# XDG_RUNTIME_DIR; the private folder has none, and X11 is asked for by name. No accessibility
# bus of the real session.
export GDK_BACKEND=x11 NO_AT_BRIDGE=1

# The lowest display number from 90 with no socket and no lock. Where /tmp/.X11-unix cannot be
# written (WSLg mounts it read-only) Xvfb listens on its abstract socket only.
listen=()
[ -w /tmp/.X11-unix ] || [ ! -e /tmp/.X11-unix ] || listen=(-nolisten unix)
free_display() {
  local n
  for n in $(seq "$1" 119); do
    if [ ! -e "/tmp/.X$n-lock" ] && [ ! -e "/tmp/.X11-unix/X$n" ]; then
      echo "$n"
      return 0
    fi
  done
  return 1
}

if [ "$print_env" -eq 1 ]; then
  n=$(free_display 90) || {
    echo "start.sh: no free display from :90 to :119" >&2
    exit 1
  }
  DISPLAY=":$n" dbus-run-session -- "$0" --inside-bus print-env "$version"
  exit 0
fi

# version: before any display or bus exists, so it proves the binary and its libraries load.
set +e
out=$(timeout 10 "${cmd[@]}" --version 2>"$work/version.err" </dev/null)
code=$?
set -e
first=$(printf '%s\n' "$out" | head -n 1)
if [ "$code" -eq 124 ]; then
  fail version "--version was still running after 10 s"
elif [ "$code" -ne 0 ]; then
  fail version "--version exited $code: $(tail -n 1 "$work/version.err")"
elif [ "$first" != "Redactor $version" ]; then
  fail version "--version printed '$first', expected 'Redactor $version'"
else
  pass version
fi

display=
# shellcheck disable=SC2329 # called through wait_for
xvfb_settled() { test -s "$work/display" || ! kill -0 "$xvfb" 2>/dev/null; }
start=90
while n=$(free_display "$start"); do
  Xvfb ":$n" -displayfd 3 -screen 0 1280x800x24 -noreset -nolisten tcp "${listen[@]}" \
    3>"$work/display" >"$work/xvfb.log" 2>&1 &
  xvfb=$!
  wait_for 15 xvfb_settled || true
  if [ -s "$work/display" ]; then
    display=":$n"
    break
  fi
  kill "$xvfb" 2>/dev/null || true
  xvfb=
  start=$((n + 1))
done
if [ -z "$display" ]; then
  fail window "Xvfb did not start: $(tail -n 1 "$work/xvfb.log" 2>/dev/null)"
  exit "$l4_failed"
fi

set +e
# The bus and the services it starts write to stderr: into a log, shown when a check failed.
DISPLAY=$display dbus-run-session -- "$0" --inside-bus run "$version" "${cmd[@]}" 2>"$work/bus.log"
inner=$?
set -e
if [ "$inner" -ne 0 ]; then
  l4_failed=1
  for log in bus app second; do
    if [ -s "$work/$log.log" ]; then
      echo "--- $log.log (last 20 lines)" >&2
      tail -n 20 "$work/$log.log" >&2
    fi
  done
fi
exit "$l4_failed"
