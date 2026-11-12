#!/usr/bin/env bash
# P2: X11 on Xvfb + openbox. Clipboard round trip, a password-manager hint target read
# by name, and a global key grab (XGrabKey through xbindkeys) reached by a synthetic key.
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

missing=$(apt_install xvfb openbox xclip xdotool xbindkeys x11-utils)
if [ -n "$missing" ]; then
  probe_broke P2 "apt could not install: $missing"
  probe_exit
fi

export DISPLAY=:99
Xvfb :99 -screen 0 1280x800x24 -nolisten tcp >"$work/xvfb.log" 2>&1 &
pids+=($!)
if ! wait_for 15 xdpyinfo; then
  show_log Xvfb "$work/xvfb.log"
  probe_broke P2 "Xvfb did not start"
  probe_exit
fi
openbox >"$work/openbox.log" 2>&1 &
pids+=($!)
sleep 1

# P2a: one client owns the clipboard, another reads it.
text="redactor-probe-$RANDOM-$RANDOM"
printf '%s' "$text" | xclip -selection clipboard -i
got=$(timeout 5 xclip -selection clipboard -o 2>/dev/null)
if [ "$got" = "$text" ]; then
  probe_result P2a PASS "xclip round trip on Xvfb + openbox"
else
  probe_result P2a FAIL "xclip round trip: read ${#got} chars, wrote ${#text}"
fi

# P2b: a password-manager hint offered as its own target is listed and readable by name.
printf 'secret' | xclip -selection clipboard -t x-kde-passwordManagerHint -i
targets=$(timeout 5 xclip -selection clipboard -t TARGETS -o 2>/dev/null | tr '\n' ' ')
hint=$(timeout 5 xclip -selection clipboard -t x-kde-passwordManagerHint -o 2>/dev/null)
if printf '%s' "$targets" | grep -q 'x-kde-passwordManagerHint' && [ "$hint" = "secret" ]; then
  probe_result P2b PASS "targets: $targets"
else
  probe_result P2b FAIL "targets: ${targets:-none}; hint read: ${hint:-nothing}"
fi

# P2c: xbindkeys grabs Ctrl+Alt+R with XGrabKey; a synthetic key press reaches the grab.
marker="$work/hotkey-fired"
cat >"$work/xbindkeysrc" <<EOF
"touch $marker"
  control+alt + r
EOF
xbindkeys -n -f "$work/xbindkeysrc" >"$work/xbindkeys.log" 2>&1 &
pids+=($!)
sleep 1
xdotool key ctrl+alt+r
if wait_for 2 test -f "$marker"; then
  probe_result P2c PASS "xdotool ctrl+alt+r reached the XGrabKey grab"
else
  show_log xbindkeys "$work/xbindkeys.log"
  probe_result P2c FAIL "no grab callback within 2 s"
fi

probe_exit
