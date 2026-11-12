#!/usr/bin/env bash
# P3: a logind mock (python3-dbusmock) on a private bus that stands in for the system bus.
# It emits PrepareForSleep(true) and a session Lock; gdbus monitor must see both.
set -uo pipefail
. "$(dirname "$0")/lib.sh"

work=$(mktemp -d)
pids=()
cleanup() {
  local p
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done
  rm -rf "$work"
}
trap cleanup EXIT

missing=$(apt_install python3-dbusmock dbus libglib2.0-bin)
if [ -n "$missing" ]; then
  probe_broke P3 "apt could not install: $missing"
  probe_exit
fi

dbus-daemon --session --nofork --print-address=3 3>"$work/address" >"$work/bus.log" 2>&1 &
pids+=($!)
if ! wait_for 10 test -s "$work/address"; then
  probe_broke P3 "private bus did not start"
  probe_exit
fi
DBUS_SYSTEM_BUS_ADDRESS=$(head -n 1 "$work/address")
export DBUS_SYSTEM_BUS_ADDRESS

python3 -m dbusmock --system --template logind >"$work/mock.log" 2>&1 &
pids+=($!)
if ! wait_for 15 gdbus introspect --system --dest org.freedesktop.login1 --object-path /org/freedesktop/login1; then
  show_log dbusmock "$work/mock.log"
  probe_broke P3 "logind mock did not appear on the bus"
  probe_exit
fi

session=$(gdbus call --system --dest org.freedesktop.login1 --object-path /org/freedesktop/login1 \
  --method org.freedesktop.DBus.Mock.AddSession probe seat0 1000 probe true 2>"$work/add.log" |
  grep -o "/org/freedesktop/login1/session/[A-Za-z0-9_]*")
if [ -z "$session" ]; then
  show_log AddSession "$work/add.log"
  probe_result P3 FAIL "the mock would not add a session"
  probe_exit
fi

gdbus monitor --system --dest org.freedesktop.login1 >"$work/monitor.log" 2>&1 &
pids+=($!)
sleep 1

emit() {
  gdbus call --system --dest org.freedesktop.login1 --object-path "$1" \
    --method org.freedesktop.DBus.Mock.EmitSignal "$2" "$3" "$4" "$5" >>"$work/emit.log" 2>&1
}
emit /org/freedesktop/login1 org.freedesktop.login1.Manager PrepareForSleep b '[<true>]'
emit "$session" org.freedesktop.login1.Session Lock '' '@av []'
sleep 1

seen=()
grep -q 'org.freedesktop.login1.Manager.PrepareForSleep (true,)' "$work/monitor.log" && seen+=(PrepareForSleep)
grep -q "$session: org.freedesktop.login1.Session.Lock" "$work/monitor.log" && seen+=(Lock)
if [ "${#seen[@]}" -eq 2 ]; then
  probe_result P3 PASS "gdbus monitor saw PrepareForSleep(true) and Lock on $session"
else
  show_log "gdbus monitor" "$work/monitor.log"
  show_log EmitSignal "$work/emit.log"
  probe_result P3 FAIL "seen: ${seen[*]:-nothing}"
fi

probe_exit
