#!/usr/bin/env bash
# Runs a command inside a headless KDE Plasma 6 Wayland session in a fedora:42 container.
# Ubuntu 24.04 has no Plasma 6 and no GlobalShortcuts portal that answers without a click;
# Fedora 42 has both. Needs Docker on the host.
#
# Usage (host): tools/ci/desktop-container.sh [command [args...]]
#
# The repository is mounted read-only at /repo and the command runs there, as an unprivileged
# user in the session, with WAYLAND_DISPLAY, DISPLAY (XWayland), XDG_CURRENT_DESKTOP=KDE and
# a logind mock on DBUS_SYSTEM_BUS_ADDRESS set. Without a command it prints the Wayland
# globals and exits. The exit status is the command's. Exit 70 means the desktop did not
# come up (the log tail is printed).
#
# The same file runs three times: on the host ("host"), in the container as root ("inner")
# and in the desktop's D-Bus session as the unprivileged user ("session").
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

host() {
  local repo
  repo=$(cd "$here/../.." && pwd)
  # SYS_NICE: kwin_wayland carries that file capability, and exec fails outside the bounding set.
  exec docker run --rm --shm-size=1g --cap-add=SYS_NICE -v "$repo:/repo:ro" -w /repo fedora:42 \
    bash /repo/tools/ci/desktop-container.sh inner "$@"
}

inner() {
  local pkgs=(kwin-wayland xdg-desktop-portal xdg-desktop-portal-kde kglobalacceld
    xorg-x11-server-Xwayland mesa-dri-drivers wayland-utils wl-clipboard xclip
    python3-dbusmock dbus-daemon dbus-tools shadow-utils util-linux findutils)
  if ! dnf -y -q install --setopt=install_weak_deps=False "${pkgs[@]}" >/tmp/dnf.log 2>&1; then
    show_log dnf /tmp/dnf.log
    echo "desktop-container: dnf install failed" >&2
    exit 70
  fi
  echo "kwin: $(kwin_wayland --version 2>/dev/null)"
  echo "portal: $(rpm -q xdg-desktop-portal)"
  useradd -m -u 1000 ci
  mkdir -p /tmp/.X11-unix && chmod 1777 /tmp/.X11-unix
  install -d -m 700 -o ci -g ci /tmp/xdg-ci
  runuser -u ci -- env XDG_RUNTIME_DIR=/tmp/xdg-ci HOME=/home/ci \
    dbus-run-session -- bash /repo/tools/ci/desktop-container.sh session "$@"
}

session() {
  local work
  work=$(mktemp -d)
  pids=() # global: the EXIT trap runs after this function has returned
  # shellcheck disable=SC2317
  stop_all() { kill "${pids[@]}" 2>/dev/null; }
  trap stop_all EXIT

  # The desktop wants logind on the system bus; the container has none. A logind mock on a
  # private bus stands in for it.
  dbus-daemon --session --nofork --print-address=3 3>"$work/sysbus" >/dev/null 2>&1 &
  pids+=($!)
  wait_for 10 test -s "$work/sysbus"
  export DBUS_SYSTEM_BUS_ADDRESS
  DBUS_SYSTEM_BUS_ADDRESS=$(head -n 1 "$work/sysbus")
  python3 -m dbusmock --system --template logind >"$work/logind.log" 2>&1 &
  pids+=($!)
  if ! wait_for 15 gdbus introspect --system --dest org.freedesktop.login1 --object-path /org/freedesktop/login1; then
    show_log logind "$work/logind.log"
    echo "desktop-container: the logind mock did not start" >&2
    exit 70
  fi
  gdbus call --system --dest org.freedesktop.login1 --object-path /org/freedesktop/login1 \
    --method org.freedesktop.DBus.Mock.AddSession c1 seat0 1000 ci true >/dev/null 2>&1

  export XDG_SESSION_TYPE=wayland XDG_CURRENT_DESKTOP=KDE KWIN_COMPOSE=Q
  kwin_wayland --virtual --xwayland --width 1280 --height 800 >"$work/desktop.log" 2>&1 &
  pids+=($!)
  local kwin=$!

  wait_for 60 has_socket "$XDG_RUNTIME_DIR" 'wayland-*'
  local sock
  sock=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'wayland-*' -type s -print -quit)
  if [ -z "$sock" ]; then
    show_log kwin "$work/desktop.log"
    echo "desktop-container: no Wayland socket within 60 s" >&2
    exit 70
  fi
  export WAYLAND_DISPLAY
  WAYLAND_DISPLAY=$(basename "$sock")

  # KWin makes the XWayland socket with the Wayland one; the lowest display is the public one.
  wait_for 20 has_socket /tmp/.X11-unix 'X*'
  local xsock
  xsock=$(find /tmp/.X11-unix -maxdepth 1 -name 'X*' -type s | sort | head -n 1)
  if [ -n "$xsock" ]; then
    export DISPLAY=":${xsock##*/X}"
  fi
  dbus-update-activation-environment WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE \
    XDG_RUNTIME_DIR DBUS_SYSTEM_BUS_ADDRESS 2>/dev/null
  # Give the shell and its portal backend a moment after the socket appears.
  sleep 5
  if ! kill -0 "$kwin" 2>/dev/null; then
    show_log kwin "$work/desktop.log"
    echo "desktop-container: KWin exited within 5 s of starting" >&2
    exit 70
  fi

  if [ "$#" -eq 0 ]; then
    timeout 10 wayland-info
  else
    "$@"
  fi
}

case "$stage" in
  host) host "$@" ;;
  inner) inner "$@" ;;
  session) session "$@" ;;
esac
