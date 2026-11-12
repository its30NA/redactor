#!/usr/bin/env bash
# P4: light Wayland compositors, headless (weston, sway, KWin of Plasma 5.27 on Ubuntu 24.04).
# Per compositor: starts within 60 s; data-control globals; wl-copy / wl-paste round trip
# from a client with no focused window.
set -uo pipefail
. "$(dirname "$0")/lib.sh"

# One session bus for every compositor (KWin wants one).
if [ -z "${PROBE_IN_SESSION:-}" ]; then
  missing=$(apt_install dbus weston sway kwin-wayland wayland-utils wl-clipboard)
  [ -n "$missing" ] && echo "apt could not install: $missing"
  PROBE_IN_SESSION=1 exec dbus-run-session -- bash "$0" "$@"
fi

work=$(mktemp -d)
pid=
cleanup() {
  [ -n "$pid" ] && kill "$pid" 2>/dev/null
  rm -rf "$work"
}
trap cleanup EXIT

# probe_compositor <id> <binary> <command...>
probe_compositor() {
  local id=$1 bin=$2
  shift 2
  if ! command -v "$bin" >/dev/null; then
    probe_result "$id" FAIL "not installed: $bin"
    return
  fi
  local run="$work/$id"
  mkdir -m 700 "$run"
  local started=$SECONDS
  XDG_RUNTIME_DIR="$run" "$@" >"$work/$id.log" 2>&1 &
  pid=$!
  if ! wait_for 60 has_socket "$run" 'wayland-*'; then
    show_log "$id" "$work/$id.log"
    probe_result "$id" FAIL "no Wayland socket within 60 s"
    kill "$pid" 2>/dev/null
    pid=
    return
  fi
  local took=$((SECONDS - started))
  local display
  display=$(basename "$(find "$run" -maxdepth 1 -name 'wayland-*' -type s -print -quit)")
  sleep 1

  local globals control text got roundtrip
  globals=$(XDG_RUNTIME_DIR="$run" WAYLAND_DISPLAY="$display" timeout 10 wayland-info 2>/dev/null)
  control=$(printf '%s\n' "$globals" | grep -oE '(ext|zwlr)_data_control_manager_v1' | sort -u | tr '\n' ' ')
  text="redactor-probe-$RANDOM-$RANDOM"
  printf '%s' "$text" | XDG_RUNTIME_DIR="$run" WAYLAND_DISPLAY="$display" timeout 10 wl-copy
  got=$(XDG_RUNTIME_DIR="$run" WAYLAND_DISPLAY="$display" timeout 10 wl-paste -n 2>/dev/null)
  XDG_RUNTIME_DIR="$run" WAYLAND_DISPLAY="$display" timeout 5 wl-copy --clear 2>/dev/null
  if [ "$got" = "$text" ]; then roundtrip=ok; else roundtrip=failed; fi

  local detail="started in ${took} s; data-control: ${control:-none}; wl-copy/wl-paste: $roundtrip"
  if [ -n "$control" ] && [ "$roundtrip" = ok ]; then
    probe_result "$id" PASS "$detail"
  else
    show_log "$id" "$work/$id.log"
    probe_result "$id" FAIL "$detail"
  fi
  kill "$pid" 2>/dev/null
  wait "$pid" 2>/dev/null
  pid=
}

printf '' >"$work/sway.config"

probe_compositor P4-weston weston weston --backend=headless --idle-time=0
probe_compositor P4-sway sway env WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
  sway -c "$work/sway.config"
probe_compositor P4-kwin kwin_wayland env KWIN_COMPOSE=Q \
  kwin_wayland --virtual --width 1280 --height 800

probe_exit
