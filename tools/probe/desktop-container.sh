#!/usr/bin/env bash
# P5: GNOME 48+ and Plasma 6, headless, in a fedora:42 container (Ubuntu 24.04's GNOME 46
# has no GlobalShortcuts portal). Records the portal version, whether BindShortcuts answers
# without a click, the data-control globals, and an unfocused X11 client reading the
# clipboard through XWayland.
#
# Usage (host): tools/probe/desktop-container.sh gnome|kde
# The same file runs inside the container ("inner", as root) and in the desktop's
# D-Bus session ("session", as an unprivileged user).
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/lib.sh"

stage=host
case "${1:-}" in
  inner | session)
    stage=$1
    shift
    ;;
esac
desktop=${1:?gnome|kde}
id="P5-$desktop"

host() {
  local out
  out=$(mktemp)
  # SYS_NICE: kwin_wayland carries that file capability, and exec fails outside the bounding set.
  docker run --rm --shm-size=1g --cap-add=SYS_NICE -v "$here:/probe:ro" fedora:42 \
    bash /probe/desktop-container.sh inner "$desktop" 2>&1 | tee "$out"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    grep '^PROBE ' "$out" | sed 's/.*/- `&`/' >>"$GITHUB_STEP_SUMMARY"
  fi
  if ! grep -q '^PROBE ' "$out"; then
    probe_broke "$id" "the container gave no result"
  elif grep -q '^PROBE .* script broke' "$out"; then
    PROBE_BROKE=1
  fi
  rm -f "$out"
  probe_exit
}

inner() {
  local common=(xdg-desktop-portal xorg-x11-server-Xwayland mesa-dri-drivers wayland-utils wl-clipboard xclip
    python3-gobject python3-dbusmock dbus-daemon dbus-tools shadow-utils util-linux findutils)
  local extra
  case "$desktop" in
    gnome) extra=(gnome-shell xdg-desktop-portal-gnome) ;;
    kde) extra=(kwin-wayland xdg-desktop-portal-kde kglobalacceld) ;;
    *)
      probe_broke "$id" "unknown desktop $desktop"
      probe_exit
      ;;
  esac
  if ! dnf -y -q install --setopt=install_weak_deps=False "${common[@]}" "${extra[@]}" >/tmp/dnf.log 2>&1; then
    show_log dnf /tmp/dnf.log
    probe_broke "$id" "dnf install failed"
    probe_exit
  fi
  case "$desktop" in
    gnome) echo "gnome-shell: $(gnome-shell --version 2>/dev/null)" ;;
    kde) echo "kwin: $(kwin_wayland --version 2>/dev/null)" ;;
  esac
  echo "portal: $(rpm -q xdg-desktop-portal)"
  useradd -m -u 1000 probe
  mkdir -p /tmp/.X11-unix && chmod 1777 /tmp/.X11-unix
  install -d -m 700 -o probe -g probe /tmp/xdg-probe
  # The portal's host registry only takes an app id that has a desktop file.
  install -d -o probe -g probe /home/probe/.local/share/applications
  printf '[Desktop Entry]\nType=Application\nName=Redactor probe\nExec=true\n' \
    >/home/probe/.local/share/applications/io.github.redactor.Probe.desktop
  runuser -u probe -- env XDG_RUNTIME_DIR=/tmp/xdg-probe HOME=/home/probe \
    dbus-run-session -- bash /probe/desktop-container.sh session "$desktop"
}

session() {
  local work
  work=$(mktemp -d)
  pids=() # global: the EXIT trap runs after this function has returned
  # shellcheck disable=SC2317
  stop_all() { kill "${pids[@]}" 2>/dev/null; }
  trap stop_all EXIT

  # GNOME Shell needs logind on the system bus; the container has none. A logind mock on a
  # private bus stands in (the same as probe P3).
  dbus-daemon --session --nofork --print-address=3 3>"$work/sysbus" >/dev/null 2>&1 &
  pids+=($!)
  wait_for 10 test -s "$work/sysbus"
  export DBUS_SYSTEM_BUS_ADDRESS
  DBUS_SYSTEM_BUS_ADDRESS=$(head -n 1 "$work/sysbus")
  python3 -m dbusmock --system --template logind >"$work/logind.log" 2>&1 &
  pids+=($!)
  if ! wait_for 15 gdbus introspect --system --dest org.freedesktop.login1 --object-path /org/freedesktop/login1; then
    show_log logind "$work/logind.log"
    probe_broke "$id" "logind mock did not start"
    return
  fi
  gdbus call --system --dest org.freedesktop.login1 --object-path /org/freedesktop/login1 \
    --method org.freedesktop.DBus.Mock.AddSession c1 seat0 1000 probe true >/dev/null 2>&1

  export XDG_SESSION_TYPE=wayland
  case "$desktop" in
    gnome)
      export XDG_CURRENT_DESKTOP=GNOME
      gnome-shell --headless --wayland --virtual-monitor 1280x800 >"$work/desktop.log" 2>&1 &
      ;;
    kde)
      export XDG_CURRENT_DESKTOP=KDE KWIN_COMPOSE=Q
      kwin_wayland --virtual --xwayland --width 1280 --height 800 >"$work/desktop.log" 2>&1 &
      ;;
  esac
  pids+=($!)

  local started=$SECONDS
  wait_for 60 has_socket "$XDG_RUNTIME_DIR" 'wayland-*'
  local sock
  sock=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'wayland-*' -type s -print -quit)
  if [ -z "$sock" ]; then
    show_log "$desktop" "$work/desktop.log"
    probe_result "$id" FAIL "no Wayland socket within 60 s (does not run headless)"
    return
  fi
  export WAYLAND_DISPLAY
  WAYLAND_DISPLAY=$(basename "$sock")
  probe_result "$id-start" PASS "Wayland socket after $((SECONDS - started)) s"

  # XWayland: GNOME starts it on demand, KWin with --xwayland; both make the socket first.
  local xsock=
  wait_for 20 has_socket /tmp/.X11-unix 'X*'
  # The lowest display is the public one (mutter keeps :1 for its own services).
  xsock=$(find /tmp/.X11-unix -maxdepth 1 -name 'X*' -type s | sort | head -n 1)
  if [ -n "$xsock" ]; then
    export DISPLAY=":${xsock##*/X}"
  fi
  dbus-update-activation-environment WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE XDG_RUNTIME_DIR \
    DBUS_SYSTEM_BUS_ADDRESS 2>/dev/null
  # Give the shell and its portal backend a moment after the socket appears.
  sleep 5
  if ! kill -0 "${pids[-1]}" 2>/dev/null; then
    show_log "$desktop" "$work/desktop.log"
    probe_result "$id" FAIL "the desktop exited within 5 s of starting"
    return
  fi

  # Data-control globals (GNOME is expected to have none).
  local globals control
  globals=$(timeout 10 wayland-info 2>/dev/null)
  if [ -n "$globals" ]; then
    control=$(printf '%s\n' "$globals" | grep -oE '(ext|zwlr)_data_control_manager_v1' | sort -u | tr '\n' ' ')
    probe_result "$id-datacontrol" PASS "data-control: ${control:-none}"
  else
    probe_result "$id-datacontrol" FAIL "wayland-info gave nothing"
  fi

  # GlobalShortcuts portal: version, and whether BindShortcuts answers with no click.
  local portal
  portal=$(timeout 60 python3 /probe/portal_shortcuts.py 2>"$work/portal.log")
  if printf '%s' "$portal" | grep -q 'bind=0'; then
    probe_result "$id-shortcuts" PASS "$portal"
  else
    show_log portal "$work/portal.log"
    probe_result "$id-shortcuts" FAIL "${portal:-the helper printed nothing}"
  fi

  # An X11 client under XWayland, never focused, reads what a Wayland client copied.
  if [ -z "${DISPLAY:-}" ]; then
    probe_result "$id-xwayland" FAIL "no XWayland socket within 20 s"
  else
    local text got back
    text="redactor-probe-$RANDOM-$RANDOM"
    printf '%s' "$text" | timeout 10 wl-copy
    sleep 1
    got=$(timeout 10 xclip -selection clipboard -o 2>/dev/null)
    printf '%s' "$text-x" | timeout 10 xclip -selection clipboard -i
    sleep 1
    back=$(timeout 10 wl-paste -n 2>/dev/null)
    local detail
    detail="Wayland->X11: $([ "$got" = "$text" ] && echo ok || echo failed); X11->Wayland: $([ "$back" = "$text-x" ] && echo ok || echo failed)"
    if [ "$got" = "$text" ]; then
      probe_result "$id-xwayland" PASS "$detail"
    else
      probe_result "$id-xwayland" FAIL "$detail"
    fi
  fi
  if grep -qi 'error\|fail' "$work/desktop.log"; then
    show_log "$desktop" "$work/desktop.log"
  fi
}

case "$stage" in
  host) host ;;
  inner) inner ;;
  session) session ;;
esac
probe_exit
