#!/usr/bin/env bash
# The guard of the L4 start check (tools/ci/l4/start.sh): it must never reach the desktop it is
# started from. On WSLg DISPLAY=:0 and WAYLAND_DISPLAY=wayland-0 are the Windows screen and
# clipboard, and HOME is a real home with real settings.
#
# Runs `start.sh --print-env` with a real-looking environment and fails unless the environment
# start.sh would give the app has no WAYLAND_DISPLAY, a DISPLAY of its own from :90, a session
# bus of its own (not the one passed in), and HOME and every XDG folder under one fresh temporary
# folder, which is gone afterwards.
#
# Usage: tools/ci/l4/selftest.sh       (no display, no app and no package needed)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)

status=0
pass() { echo "PASS $1"; }
fail() {
  echo "FAIL $1 $2"
  status=1
}

real=/nonexistent-real-home
set +e
out=$(env DISPLAY=:0 WAYLAND_DISPLAY=wayland-0 WAYLAND_SOCKET=3 XAUTHORITY="$real/.Xauthority" \
  HOME="$real" XDG_RUNTIME_DIR=/run/user/1000 XDG_CONFIG_HOME="$real/.config" \
  XDG_DATA_HOME="$real/.local/share" XDG_CACHE_HOME="$real/.cache" XDG_STATE_HOME="$real/.local/state" \
  DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
  "$here/start.sh" --print-env 0.0.0 true 2>&1)
code=$?
set -e
if [ "$code" -ne 0 ]; then
  fail print-env "start.sh --print-env exited $code: $(printf '%s\n' "$out" | tail -n 1)"
  exit 1
fi

# value <name>: the value start.sh printed for <name>, or "<unset>".
value() {
  local line
  line=$(printf '%s\n' "$out" | grep -m1 "^$1=" || true)
  if [ -z "$line" ]; then echo "<unset>"; else echo "${line#*=}"; fi
}

work=$(value L4_WORK)
case "$work" in
  "<unset>" | "" | / | "$real"*) fail work "no temporary folder (L4_WORK=$work)" ;;
  "${TMPDIR:-/tmp}"/*) pass work ;;
  *) fail work "L4_WORK=$work is not under ${TMPDIR:-/tmp}" ;;
esac

for name in WAYLAND_DISPLAY WAYLAND_SOCKET XAUTHORITY; do
  if [ "$(value "$name")" = "<unset>" ]; then pass "no_$name"; else fail "no_$name" "$name=$(value "$name")"; fi
done

display=$(value DISPLAY)
if [[ "$display" =~ ^:[0-9]+$ ]] && [ "${display#:}" -ge 90 ]; then
  pass display
else
  fail display "DISPLAY=$display, expected a number of its own from :90"
fi

for name in HOME TMPDIR XDG_RUNTIME_DIR XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME XDG_STATE_HOME; do
  v=$(value "$name")
  case "$v" in
    "$work"/*) pass "$name" ;;
    *) fail "$name" "$name=$v is not under L4_WORK" ;;
  esac
done

bus=$(value DBUS_SESSION_BUS_ADDRESS)
case "$bus" in
  "<unset>" | "" | *"/run/user/"*) fail session_bus "DBUS_SESSION_BUS_ADDRESS=$bus is not a bus of its own" ;;
  *) pass session_bus ;;
esac

if [ -n "$work" ] && [ -e "$work" ]; then
  fail cleaned_up "$work is still there"
else
  pass cleaned_up
fi

exit "$status"
