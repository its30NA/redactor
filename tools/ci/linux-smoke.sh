#!/usr/bin/env bash
# The app under test on a throwaway X display (evidence level L3): the built binary, started
# through tauri-driver (WebDriver) and directly, on Xvfb + openbox with private D-Bus buses and
# a logind mock. Nothing here reaches the desktop it is started from (on WSLg, DISPLAY is the
# Windows desktop); the app's settings go to a throwaway XDG tree. Made-up text only.
#
# Usage: tools/ci/linux-smoke.sh <app-binary>   the checks S1-S8
#        tools/ci/linux-smoke.sh --self-test    the set-up only (display, window manager,
#                                               clipboard, key grabs, mocks), no app
# Environment:
#   SMOKE_WEBDRIVER=0   the fallback without WebDriver: S1 is "window visible", S2/S5/S6 are SKIP
#   SMOKE_OUT=<dir>     screenshots, video and logs (default smoke-out, emptied first)
#   SMOKE_WD_PORT=<n>   tauri-driver's port (default 4444; WebKitWebDriver gets n+1)
#   TAURI_DRIVER=<path> the tauri-driver binary (default: from PATH)
#
# Prints one line per check, `SMOKE <id> PASS|FAIL|SKIP <detail>`, also into
# $GITHUB_STEP_SUMMARY, and takes a screenshot after every check. Exit 1 if any check failed.
set -euo pipefail
cd "$(dirname "$0")/../.."
root=$PWD

self_test=0
app=
case "${1:-}" in
  --self-test) self_test=1 ;;
  "" | -*)
    echo "usage: $0 <app-binary> | --self-test" >&2
    exit 64
    ;;
  *)
    app=$(realpath -- "$1")
    if [ ! -x "$app" ]; then
      echo "linux-smoke: $1 is not an executable file" >&2
      exit 2
    fi
    ;;
esac
webdriver=${SMOKE_WEBDRIVER:-1}
[ "$self_test" -eq 1 ] && webdriver=0
out=${SMOKE_OUT:-smoke-out}
wd_port=${SMOKE_WD_PORT:-4444}
tauri_driver=${TAURI_DRIVER:-tauri-driver}

need=(Xvfb openbox xclip xdotool xbindkeys wmctrl xprop import dbus-daemon dbus-send gdbus setsid timeout)
[ "$webdriver" = 1 ] && need+=("$tauri_driver" WebKitWebDriver)
for tool in "${need[@]}"; do
  if ! command -v "$tool" >/dev/null; then
    echo "linux-smoke: $tool is missing (see the app-linux-desktop job in .github/workflows/ci.yml)" >&2
    exit 2
  fi
done
# A pyenv or toolcache python3 does not see the system's python3-dbusmock.
python=python3
"$python" -c 'import dbusmock' 2>/dev/null || python=/usr/bin/python3
if ! "$python" -c 'import dbusmock' 2>/dev/null; then
  echo "linux-smoke: python3-dbusmock is missing" >&2
  exit 2
fi
wd=("$python" "$root/tools/ci/smoke/webdriver.py")

rm -rf "$out"
mkdir -p "$out/logs"
out=$(realpath -- "$out")
work=$(mktemp -d)
pids=()   # plain processes
groups=() # process groups (setsid): the app or tauri-driver with everything they started
video=
cleanup() {
  local p
  if [ -n "$video" ] && kill -0 "$video" 2>/dev/null; then
    # ffmpeg finishes the file on SIGTERM.
    kill -TERM "$video" 2>/dev/null || true
    wait_for 10 gone "$video" || kill -KILL "$video" 2>/dev/null || true
  fi
  for p in "${groups[@]}"; do kill -TERM -- "-$p" 2>/dev/null || true; done
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
  cp "$work"/*.log "$out/logs/" 2>/dev/null || true
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

# gone <pid>: the process has exited (a zombie that its parent has not reaped counts).
gone() {
  local stat
  stat=$(ps -o stat= -p "$1" 2>/dev/null) || return 0
  [[ $stat == Z* ]]
}

fail() {
  echo "linux-smoke: $1" >&2
  shift
  local log
  for log in "$@"; do
    [ -f "$log" ] && tail -n 20 "$log" >&2
  done
  exit 1
}

# Our own display number, from 99 up, never one that already has a socket or a lock (on WSLg
# ":0" is the Windows desktop). Where /tmp/.X11-unix cannot be written (WSLg mounts it
# read-only) Xvfb listens on its abstract socket only.
listen=()
[ -w /tmp/.X11-unix ] || [ ! -e /tmp/.X11-unix ] || listen=(-nolisten unix)
display=
xvfb=
xvfb_settled() { test -s "$work/display" || ! kill -0 "$xvfb" 2>/dev/null; }
for n in $(seq 99 119); do
  if [ -e "/tmp/.X$n-lock" ] || [ -e "/tmp/.X11-unix/X$n" ]; then continue; fi
  # -noreset: an X server starts over when its last client leaves.
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
export DISPLAY="$display"
unset WAYLAND_DISPLAY XAUTHORITY
# The app's settings and caches go to a throwaway tree, never the user's.
for d in config data cache state runtime; do mkdir -p "$work/xdg/$d"; done
chmod 700 "$work/xdg/runtime"
export XDG_CONFIG_HOME="$work/xdg/config" XDG_DATA_HOME="$work/xdg/data"
export XDG_CACHE_HOME="$work/xdg/cache" XDG_STATE_HOME="$work/xdg/state"
export XDG_RUNTIME_DIR="$work/xdg/runtime" XDG_SESSION_TYPE=x11 XDG_SESSION_ID=smoke
export GDK_BACKEND=x11 NO_AT_BRIDGE=1
# Xvfb has no GPU: with accelerated compositing WebKitGTK draws nothing into the X window, and
# every screenshot is an empty page.
export WEBKIT_DISABLE_COMPOSITING_MODE=1

openbox --sm-disable >"$work/openbox.log" 2>&1 &
pids+=($!)
wait_for 10 wmctrl -m || fail "openbox did not start" "$work/openbox.log"

start_bus() {
  dbus-daemon --session --nofork --print-address=3 3>"$work/$1.address" >"$work/$1-bus.log" 2>&1 &
  pids+=($!)
  wait_for 10 test -s "$work/$1.address" || fail "the private $1 bus did not start" "$work/$1-bus.log"
}
start_bus session
start_bus system
DBUS_SESSION_BUS_ADDRESS=$(head -n 1 "$work/session.address")
DBUS_SYSTEM_BUS_ADDRESS=$(head -n 1 "$work/system.address")
export DBUS_SESSION_BUS_ADDRESS DBUS_SYSTEM_BUS_ADDRESS

has_name() { # has_name <--system|--session> <name>
  dbus-send "$1" --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus \
    org.freedesktop.DBus.NameHasOwner "string:$2" | grep -q 'boolean true'
}

# logind: one session, "smoke", that GetSessionByPID returns for any process (the template has
# no GetSessionByPID; GetSession($XDG_SESSION_ID) returns the same path).
session=/org/freedesktop/login1/session/smoke
"$python" -m dbusmock --system --template logind >"$work/logind.log" 2>&1 &
pids+=($!)
wait_for 15 has_name --system org.freedesktop.login1 || fail "the logind mock did not appear" "$work/logind.log"
logind() { # logind <object> <method> <args...>
  local obj=$1 method=$2
  shift 2
  gdbus call --system --dest org.freedesktop.login1 --object-path "$obj" --method "$method" "$@"
}
logind /org/freedesktop/login1 org.freedesktop.DBus.Mock.AddSession smoke seat0 "$(id -u)" smoke true \
  >"$work/logind-setup.log" 2>&1 || fail "the logind mock would not add a session" "$work/logind-setup.log"
logind /org/freedesktop/login1 org.freedesktop.DBus.Mock.AddMethod org.freedesktop.login1.Manager \
  GetSessionByPID u o "ret = dbus.ObjectPath('$session')" >>"$work/logind-setup.log" 2>&1 ||
  fail "the logind mock would not take GetSessionByPID" "$work/logind-setup.log"
emit_lock() {
  logind "$session" org.freedesktop.DBus.Mock.EmitSignal org.freedesktop.login1.Session Lock '' '@av []'
}
emit_sleep() {
  logind /org/freedesktop/login1 org.freedesktop.DBus.Mock.EmitSignal \
    org.freedesktop.login1.Manager PrepareForSleep b '[<true>]'
}

# Notifications go to a mock, so the app never waits on a missing daemon.
"$python" -m dbusmock --session --template notification_daemon >"$work/notifications.log" 2>&1 &
pids+=($!)
wait_for 15 has_name --session org.freedesktop.Notifications ||
  fail "the notification mock did not appear" "$work/notifications.log"

# A tray host for S8 (and the self-test): a StatusNotifierWatcher that reports a host.
start_tray_watcher() {
  "$python" -m dbusmock --session org.kde.StatusNotifierWatcher /StatusNotifierWatcher \
    org.kde.StatusNotifierWatcher >"$work/tray-watcher.log" 2>&1 &
  pids+=($!)
  wait_for 15 has_name --session org.kde.StatusNotifierWatcher || return 1
  local mock=(gdbus call --session --dest org.kde.StatusNotifierWatcher --object-path /StatusNotifierWatcher
    --method)
  {
    "${mock[@]}" org.freedesktop.DBus.Mock.AddProperty '' IsStatusNotifierHostRegistered '<true>' &&
      "${mock[@]}" org.freedesktop.DBus.Mock.AddProperty '' ProtocolVersion '<0>' &&
      "${mock[@]}" org.freedesktop.DBus.Mock.AddProperty '' RegisteredStatusNotifierItems '<@as []>' &&
      "${mock[@]}" org.freedesktop.DBus.Mock.AddMethod '' RegisterStatusNotifierItem s '' '' &&
      "${mock[@]}" org.freedesktop.DBus.Mock.AddMethod '' RegisterStatusNotifierHost s '' ''
  } >>"$work/tray-watcher.log" 2>&1
}

# The video of the whole run; without ffmpeg the screenshots still count.
if command -v ffmpeg >/dev/null; then
  ffmpeg -nostdin -loglevel error -f x11grab -video_size 1280x800 -framerate 10 -i "$display" \
    -c:v libx264 -preset ultrafast -pix_fmt yuv420p "$out/run.mp4" >"$work/ffmpeg.log" 2>&1 &
  video=$!
else
  echo "linux-smoke: ffmpeg is missing, so there is no video of this run (the screenshots still count)"
fi

passed=0
failed=0
skipped=0
lines=()
shot() {
  import -display "$display" -window root "$out/$1.png" 2>>"$work/import.log" ||
    echo "linux-smoke: no screenshot for $1 (see logs/import.log)"
}
# result <id> <PASS|FAIL|SKIP> <detail>
result() {
  local line="SMOKE $1 $2 $3"
  echo "$line"
  lines+=("$line")
  case "$2" in
    PASS) passed=$((passed + 1)) ;;
    SKIP) skipped=$((skipped + 1)) ;;
    *) failed=$((failed + 1)) ;;
  esac
  shot "$1"
}

# The clipboard through xclip. `-quiet` keeps xclip in the foreground, so it is ours to stop;
# it exits by itself once another client owns the clipboard.
clip_set() {
  printf '%s' "$1" >"$work/clip.in"
  xclip -selection clipboard -i -quiet "$work/clip.in" >>"$work/xclip.log" 2>&1 &
  pids+=($!)
  wait_for 2 clip_is "$1"
}
clip_get() { timeout 2 xclip -selection clipboard -o 2>/dev/null; }
clip_is() { [ "$(clip_get)" = "$1" ]; }
visible() { xdotool search --onlyvisible --name '^Redactor$' >/dev/null 2>&1; }
hidden() { ! visible; }

# Made-up text, built at run time.
token=$(od -An -N9 -tx1 /dev/urandom | tr -d ' \n')
secret_value="Smk${token}Qz7"
secret="DB_PASSWORD=$secret_value"
clean="lunch at noon $$"

if [ "$self_test" -eq 1 ]; then
  # T1 the display and the window manager
  geometry=$(xdotool getdisplaygeometry 2>&1 || true)
  wm=$(wmctrl -m 2>/dev/null | sed -n 's/^Name: //p')
  if [ "$geometry" = "1280 800" ] && [ -n "$wm" ]; then
    result T1 PASS "display $display is 1280x800, window manager $wm"
  else
    result T1 FAIL "display $display: geometry '$geometry', window manager '${wm:-none}'"
  fi

  # T2 the clipboard round trip
  if clip_set "$clean"; then
    result T2 PASS "xclip wrote and read back ${#clean} chars"
  else
    result T2 FAIL "xclip read back something else"
  fi

  # T3 a key grab fires from xdotool (what S3 relies on): xbindkeys holds Ctrl+Alt+R
  printf '"touch %s"\n  control+alt + r\n' "$work/fired" >"$work/xbindkeysrc"
  xbindkeys -n -f "$work/xbindkeysrc" >"$work/xbindkeys.log" 2>&1 &
  pids+=($!)
  sleep 1
  xdotool key ctrl+alt+r || true
  if wait_for 5 test -e "$work/fired"; then
    result T3 PASS "xdotool key ctrl+alt+r reached a grab within 5 s"
  else
    result T3 FAIL "the grab did not fire within 5 s"
  fi

  # T4 a window titled Redactor: found visible, closed by wmctrl, its process exits (S7's path)
  xmessage -title Redactor "made-up smoke window $$" >"$work/xmessage.log" 2>&1 &
  win=$!
  pids+=("$win")
  if wait_for 10 visible; then
    shot T4-before
    wmctrl -F -c Redactor || true
    if wait_for 5 gone "$win"; then
      result T4 PASS "window visible, closed by wmctrl, process gone within 5 s"
    else
      result T4 FAIL "the process was still alive 5 s after wmctrl -c"
    fi
  else
    result T4 FAIL "no visible window named Redactor within 10 s"
  fi

  # T5 the logind mock: GetSessionByPID, then Lock and PrepareForSleep(true) are seen on the bus
  got=$(logind /org/freedesktop/login1 org.freedesktop.login1.Manager.GetSessionByPID "uint32 $$" 2>&1 || true)
  gdbus monitor --system --dest org.freedesktop.login1 >"$work/monitor.log" 2>&1 &
  pids+=($!)
  sleep 1
  emit_lock >>"$work/emit.log" 2>&1 || true
  emit_sleep >>"$work/emit.log" 2>&1 || true
  saw() { grep -q "$1" "$work/monitor.log"; }
  if [[ $got == *"'$session'"* ]] && wait_for 5 saw "$session: org.freedesktop.login1.Session.Lock" &&
    wait_for 5 saw 'org.freedesktop.login1.Manager.PrepareForSleep (true,)'; then
    result T5 PASS "GetSessionByPID gives $session; Lock and PrepareForSleep(true) reached the bus"
  else
    result T5 FAIL "GetSessionByPID: ${got:-nothing}; see logs/monitor.log"
  fi

  # T6 the tray watcher for S8
  if start_tray_watcher; then
    prop=$(gdbus call --session --dest org.kde.StatusNotifierWatcher --object-path /StatusNotifierWatcher \
      --method org.freedesktop.DBus.Properties.Get org.kde.StatusNotifierWatcher \
      IsStatusNotifierHostRegistered 2>&1 || true)
    reg=$(gdbus call --session --dest org.kde.StatusNotifierWatcher --object-path /StatusNotifierWatcher \
      --method org.kde.StatusNotifierWatcher.RegisterStatusNotifierItem smoke 2>&1 || echo failed)
    if [ "$prop" = "(<true>,)" ] && [ "$reg" = "()" ]; then
      result T6 PASS "StatusNotifierWatcher reports a host and takes RegisterStatusNotifierItem"
    else
      result T6 FAIL "property '$prop', register '$reg'"
    fi
  else
    result T6 FAIL "the StatusNotifierWatcher mock did not start (see logs/tray-watcher.log)"
  fi
else
  # The app under test. Each launch gets its own process group, so the clean-up reaches the
  # WebKit processes too.
  launched=
  launch() { # launch <log>: start the app directly; its pid goes to $launched
    setsid "$app" >>"$work/$1" 2>&1 &
    launched=$!
    groups+=("$launched")
  }
  app_pid=
  sid=
  window_pid() { xdotool search --name '^Redactor$' getwindowpid 2>/dev/null | head -n 1; }

  # S1
  if [ "$webdriver" = 1 ]; then
    export SMOKE_WD_PORT="$wd_port"
    setsid "$tauri_driver" --port "$wd_port" --native-port "$((wd_port + 1))" \
      --native-driver "$(command -v WebKitWebDriver)" >"$work/tauri-driver.log" 2>&1 &
    groups+=($!)
    listening() { (exec 3<>"/dev/tcp/127.0.0.1/$wd_port") 2>/dev/null; }
    if ! wait_for 10 listening; then
      result S1 FAIL "tauri-driver did not listen on port $wd_port within 10 s (see logs/tauri-driver.log)"
    elif ! sid=$("${wd[@]}" --timeout 30 session "$app" 2>>"$work/webdriver.log"); then
      result S1 FAIL "no WebDriver session within 30 s: $sid"
      sid=
    else
      title=$("${wd[@]}" title "$sid" 2>>"$work/webdriver.log" || true)
      if [ "$title" = "Redactor" ] && wait_for 10 visible; then
        app_pid=$(window_pid)
        result S1 PASS "WebDriver session $sid, title Redactor, window visible (pid ${app_pid:-unknown})"
      else
        result S1 FAIL "session $sid, title '$title', visible window: $(visible && echo yes || echo no)"
      fi
    fi
  else
    launch app-s1.log
    app_pid=$launched
    if wait_for 30 visible; then
      result S1 PASS "window Redactor visible within 30 s (WebDriver off)"
    else
      result S1 FAIL "no visible window Redactor within 30 s (see logs/app-s1.log)"
    fi
  fi
  running() { [ -n "$app_pid" ] && ! gone "$app_pid"; }

  # S2
  editor='[data-testid=input-editor] .cm-content'
  type_secret() { # type the secret into the editor and wait for the count
    "${wd[@]}" type "$sid" "$editor" "$secret" >>"$work/webdriver.log" 2>&1 &&
      "${wd[@]}" --timeout 5 text "$sid" '[data-testid=input-count]' prefix "${#secret} chars" >/dev/null
  }
  if [ "$webdriver" != 1 ]; then
    result S2 SKIP "not tested at L3 (WebDriver off); the L2 tests cover IPC"
  elif [ -z "$sid" ]; then
    result S2 FAIL "no WebDriver session (S1)"
  elif ! "${wd[@]}" type "$sid" "$editor" "$secret" >>"$work/webdriver.log" 2>&1; then
    result S2 FAIL "could not type into $editor (see logs/webdriver.log)"
  elif ! v=$("${wd[@]}" --timeout 10 visible "$sid" '[data-testid=verified]'); then
    result S2 FAIL "[data-testid=verified] not visible within 10 s ($v)"
  elif ! c=$("${wd[@]}" --timeout 5 text "$sid" '[data-testid=input-count]' prefix "${#secret} chars"); then
    result S2 FAIL "input-count shows '$c', expected ${#secret} chars"
  else
    result S2 PASS "verified badge visible, input-count '$c'"
  fi

  # S3
  if ! running; then
    result S3 FAIL "the app is not running"
  elif ! clip_set "$secret"; then
    result S3 FAIL "xclip could not put the line on the clipboard"
  else
    xdotool key ctrl+alt+r || true
    scrubbed() { local c; c=$(clip_get) && [[ $c == "DB_PASSWORD=[REDACTED"* && $c != *"$secret_value"* ]]; }
    if wait_for 5 scrubbed; then
      result S3 PASS "within 5 s the clipboard starts with DB_PASSWORD=[REDACTED and lacks the password"
    else
      c=$(clip_get || true)
      if [[ $c == *"$secret_value"* ]]; then
        result S3 FAIL "after 5 s the clipboard still holds the password"
      else
        result S3 FAIL "after 5 s the clipboard holds ${#c} chars without the expected prefix"
      fi
    fi
  fi

  # S4
  if ! running; then
    result S4 FAIL "the app is not running"
  elif ! clip_set "$clean"; then
    result S4 FAIL "xclip could not put the clean line on the clipboard"
  else
    xdotool key ctrl+alt+r || true
    sleep 2
    if clip_is "$clean"; then
      result S4 PASS "clean text identical 2 s after the hotkey"
    else
      result S4 FAIL "the clipboard changed: '$(clip_get || true)'"
    fi
  fi

  # S5 and S6: wipe on lock and on sleep
  for check in S5 S6; do
    if [ "$check" = S5 ]; then what=Lock; emit=emit_lock; else what="PrepareForSleep(true)"; emit=emit_sleep; fi
    if [ "$webdriver" != 1 ]; then
      result "$check" SKIP "not tested at L3 (WebDriver off); the L2 tests cover the $what watcher"
      continue
    fi
    if [ -z "$sid" ] || ! running; then
      result "$check" FAIL "no WebDriver session or the app is not running"
      continue
    fi
    c=$("${wd[@]}" --timeout 1 text "$sid" '[data-testid=input-count]' exact "0 chars" || true)
    if [ "$c" = "0 chars" ] && ! type_secret; then
      result "$check" FAIL "could not type the line before $what"
      continue
    fi
    if ! "$emit" >>"$work/emit.log" 2>&1; then
      result "$check" FAIL "the mock would not emit $what (see logs/emit.log)"
    elif c=$("${wd[@]}" --timeout 5 text "$sid" '[data-testid=input-count]' exact "0 chars"); then
      result "$check" PASS "$what wiped the input within 5 s"
    else
      result "$check" FAIL "5 s after $what input-count shows '$c'"
    fi
  done

  # S7: without a tray host, closing the window ends the app.
  if ! running; then
    result S7 FAIL "the app is not running"
  else
    wmctrl -F -c Redactor || true
    if wait_for 5 gone "$app_pid"; then
      result S7 PASS "no tray host: the process exited within 5 s of wmctrl -c"
    else
      result S7 FAIL "the process was still alive 5 s after wmctrl -c"
    fi
  fi
  if [ -n "$sid" ]; then "${wd[@]}" delete "$sid" >/dev/null 2>&1 || true; fi
  for p in "${groups[@]}"; do kill -TERM -- "-$p" 2>/dev/null || true; done
  wait_for 10 hidden || true

  # S8: with a tray host, closing hides the window; a second launch shows it again.
  if ! start_tray_watcher; then
    result S8 FAIL "the StatusNotifierWatcher mock did not start (see logs/tray-watcher.log)"
  else
    launch app-s8.log
    first=$launched
    if ! wait_for 30 visible; then
      result S8 FAIL "no visible window within 30 s of the direct launch (see logs/app-s8.log)"
    else
      wmctrl -F -c Redactor || true
      if ! wait_for 5 hidden; then
        result S8 FAIL "the window was still visible 5 s after wmctrl -c"
      elif gone "$first"; then
        result S8 FAIL "the process exited on close although a tray host was present"
      else
        shot S8-hidden
        launch app-s8-second.log
        second=$launched
        if ! wait_for 10 gone "$second"; then
          result S8 FAIL "the second launch did not exit within 10 s"
        elif ! wait_for 5 visible; then
          result S8 FAIL "the second launch exited, but the window stayed hidden"
        elif gone "$first"; then
          result S8 FAIL "the first process is gone after the second launch"
        else
          result S8 PASS "closed to the tray (alive, no window); the second launch exited and showed the window"
        fi
      fi
    fi
  fi
fi

total=$((passed + failed + skipped))
summary="linux-smoke: $passed passed, $failed failed, $skipped skipped on display $display"
echo "$summary"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  {
    if [ "$self_test" -eq 1 ]; then echo "### Linux smoke set-up (self-test)"; else echo "### Linux app smoke (L3)"; fi
    echo
    echo '```'
    printf '%s\n' "${lines[@]}"
    echo '```'
    echo
    echo "$summary"
    if [ "$skipped" -gt 0 ]; then
      echo
      echo "**Known limits:** the SKIP checks were not tested at L3 (WebDriver off); their L2 tests stay."
    fi
  } >>"$GITHUB_STEP_SUMMARY"
fi
[ "$total" -gt 0 ] && [ "$failed" -eq 0 ]
