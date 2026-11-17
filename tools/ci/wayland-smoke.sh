#!/usr/bin/env bash
# The app under test on a throwaway Wayland compositor (evidence level L3): the built binary on
# headless sway, inside tools/ci/wayland-tests.sh --run (its own XDG_RUNTIME_DIR, a private
# session bus with no services and no portal on it). Nothing here reaches the desktop it is
# started from (on WSLg, WAYLAND_DISPLAY is the Windows clipboard); the app's settings go to a
# throwaway XDG tree. Made-up text only, built at run time; no clipboard text is printed.
#
# Usage: tools/ci/wayland-smoke.sh <app-binary>   the checks W1-W7
# Environment:
#   SMOKE_OUT=<dir>     screenshots and logs (default smoke-out, emptied first)
#   SMOKE_WD_PORT=<n>   tauri-driver's port (default 4444; WebKitWebDriver gets n+1)
#   TAURI_DRIVER=<path> the tauri-driver binary (default: from PATH)
#
# The script builds two development helpers first (app/src-tauri/examples): `wayland_offer`, a
# password manager's copy (text plus x-kde-passwordManagerHint = secret, which wl-copy cannot
# offer), and `wayland_keyboard`, a keyboard for the seat (headless sway has none, and without
# one no window ever gets focus). Then it runs itself inside tools/ci/wayland-tests.sh --run,
# with MOCK_PORTAL=none: no portal on the bus, so the app's hotkey is the command
# `redactor --scrub-clipboard`.
#
# The checks, in the order they run:
#   W1 the app starts as a native Wayland window (xdg_shell, an app_id; never Xwayland)
#   W2 a secret copied by wl-copy, then `--scrub-clipboard`: the clipboard is cleaned in 5 s
#   W3 a password manager's copy (the hint) is left alone by `--scrub-clipboard`
#   W6 without a portal, Settings shows the command to bind (WebDriver text check, screenshot)
#   W4 the window clipboard (REDACTOR_WAYLAND_FOCUSED=1, a second start of the app): the copy is
#      made while the window is on another workspace; once sway focuses it again,
#      `--scrub-clipboard` cleans in 5 s
#   W5 the same with the window on another workspace when `--scrub-clipboard` comes: the scrub
#      waits; `swaymsg workspace 1` brings the window back and it is cleaned, once
#   W7 quit (the window's close request; no tray here) leaves no process, window, socket or
#      bus name of ours
#
# Prints one line per check, `SMOKE <id> PASS|FAIL|SKIP <detail>`, also into
# $GITHUB_STEP_SUMMARY, and takes a screenshot (grim) after every check. Exit 1 if any check
# failed or did not report.
set -euo pipefail
cd "$(dirname "$0")/../.."
root=$PWD
# shellcheck source=tools/ci/lib.sh
. tools/ci/lib.sh

usage() {
  echo "usage: $0 <app-binary>" >&2
  exit 64
}

inside=0
if [ "${1:-}" = "--inside" ]; then
  inside=1
  shift
fi
case "${1:-}" in
  "" | -*) usage ;;
esac
[ "$#" -eq 1 ] || usage
app=$(realpath -- "$1")
if [ ! -x "$app" ] || [ -d "$app" ]; then
  echo "wayland-smoke: $1 is not an executable file" >&2
  exit 2
fi
tauri_driver=${TAURI_DRIVER:-tauri-driver}

if [ "$inside" -eq 0 ]; then
  for tool in sway swaymsg grim wl-copy wl-paste jq dbus-send setsid timeout pgrep cargo \
    "$tauri_driver" WebKitWebDriver; do
    if ! command -v "$tool" >/dev/null; then
      echo "wayland-smoke: $tool is missing (see the app-linux-wayland job in .github/workflows/ci.yml)" >&2
      exit 2
    fi
  done
  echo "wayland-smoke: building the helpers (app/src-tauri/examples)"
  cargo build -p redactor-app --locked --quiet --example wayland_offer --example wayland_keyboard
  out=${SMOKE_OUT:-smoke-out}
  rm -rf "$out"
  mkdir -p "$out/logs"
  SMOKE_OUT=$(realpath -- "$out")
  SMOKE_HELPERS=$(realpath -- "${CARGO_TARGET_DIR:-target}/debug/examples")
  export SMOKE_OUT SMOKE_HELPERS
  # No portal on the bus: the hotkey is the command, as on a desktop without one.
  MOCK_PORTAL=none exec tools/ci/wayland-tests.sh --run "$root/tools/ci/wayland-smoke.sh" --inside "$app"
fi

# From here on: inside tools/ci/wayland-tests.sh --run. Never on another compositor.
runtime=${XDG_RUNTIME_DIR:-}
socket=${REDACTOR_TEST_WAYLAND:-}
if [ -z "$socket" ] || [ "${WAYLAND_DISPLAY:-}" != "$socket" ] || [ -z "$runtime" ] ||
  [ "$(cat "$runtime/redactor-test-runtime" 2>/dev/null)" != "$socket" ]; then
  echo "wayland-smoke: --inside runs only under tools/ci/wayland-tests.sh --run; start $0 <app-binary>" >&2
  exit 2
fi
out=${SMOKE_OUT:?}
helpers=${SMOKE_HELPERS:?}
wd_port=${SMOKE_WD_PORT:-4444}
python=/usr/bin/python3
wd=("$python" "$root/tools/ci/smoke/webdriver.py")
SWAYSOCK=$(find "$runtime" -maxdepth 1 -name 'sway-ipc.*.sock' -type s -print -quit)
[ -n "$SWAYSOCK" ] || {
  echo "wayland-smoke: no sway IPC socket in the runtime folder" >&2
  exit 2
}
export SWAYSOCK

work=$(mktemp -d)
pids=()   # plain processes
groups=() # process groups (setsid): an app or tauri-driver with everything they started
cleanup() {
  local p
  for p in "${groups[@]}"; do kill -TERM -- "-$p" 2>/dev/null || true; done
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
  cp "$work"/*.log "$out/logs/" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

# The app's settings and caches go to a throwaway tree, never the user's.
for d in config data cache state; do mkdir -p "$work/xdg/$d"; done
export XDG_CONFIG_HOME="$work/xdg/config" XDG_DATA_HOME="$work/xdg/data"
export XDG_CACHE_HOME="$work/xdg/cache" XDG_STATE_HOME="$work/xdg/state"
export XDG_SESSION_TYPE=wayland
# Headless sway has no GPU: with accelerated compositing WebKitGTK shows its first frames only,
# and later screenshots miss every change (seen on the Settings sheet).
export WEBKIT_DISABLE_COMPOSITING_MODE=1

# gone <pid>: the process has exited (a zombie that its parent has not reaped counts).
gone() {
  local stat
  stat=$(ps -o stat= -p "$1" 2>/dev/null) || return 0
  [[ $stat == Z* ]]
}

# The windows of the app under test, as sway sees them: "pid app_id shell focused visible"
# for each window whose process runs the app binary.
windows() {
  local pid rest
  swaymsg -t get_tree 2>/dev/null |
    jq -r '.. | objects | select((.type? == "con" or .type? == "floating_con") and (.pid? // 0) > 0)
      | "\(.pid) \(.app_id // "-") \(.shell // "-") \(.focused) \(.visible)"' |
    while read -r pid rest; do
      if [ "$(readlink "/proc/$pid/exe" 2>/dev/null)" = "$app" ]; then echo "$pid $rest"; fi
    done
}
window() { windows | head -n 1; }
has_window() { [ -n "$(window)" ]; }
no_window() { [ -z "$(window)" ]; }
# field <n>: the n-th field of the window line (1 pid, 2 app_id, 3 shell, 4 focused, 5 visible).
field() { window | cut -d ' ' -f "$1"; }
focused() { [ "$(field 4)" = true ]; }
unfocused() { [ "$(field 4)" = false ]; }
workspace() { swaymsg "workspace $1" >>"$work/swaymsg.log" 2>&1; }

passed=0
failed=0
skipped=0
lines=()
shot() {
  grim "$out/$1.png" 2>>"$work/grim.log" ||
    echo "wayland-smoke: no screenshot for $1 (see logs/grim.log)"
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

# The clipboard through wl-copy and wl-paste (data-control on sway). `--foreground` keeps
# wl-copy ours to stop; it exits by itself once another client owns the clipboard.
clip_set() {
  printf '%s' "$1" >"$work/clip.in"
  wl-copy --foreground <"$work/clip.in" >>"$work/wl-copy.log" 2>&1 &
  pids+=($!)
  wait_for 2 clip_is "$1"
}
clip_get() { timeout 2 wl-paste --no-newline 2>/dev/null; }
clip_is() { [ "$(clip_get)" = "$1" ]; }

# Made-up text, built at run time: a password line, new for each check.
secret_for() { # secret_for <tag>: sets $secret_value and $secret
  secret_value="Smk$(od -An -N9 -tx1 /dev/urandom | tr -d ' \n')$1"
  secret="DB_PASSWORD=$secret_value"
}
scrubbed() {
  local c
  c=$(clip_get) && [[ $c == "DB_PASSWORD=[REDACTED"* && $c != *"$secret_value"* ]]
}
# What the clipboard holds instead of the cleaned line, without printing it.
clip_state() {
  local c
  if ! c=$(clip_get); then
    echo "an empty clipboard"
  elif [[ $c == *"$secret_value"* ]]; then
    echo "the password still"
  else
    echo "${#c} chars without the expected prefix"
  fi
}

# A second start with --scrub-clipboard, as a desktop shortcut runs it; its pid goes to
# $request. It hands the flag to the running app over the session bus and exits.
request=
scrub_request() {
  "$app" --scrub-clipboard >>"$work/scrub-request.log" 2>&1 &
  request=$!
}
# request_ended: the second start exited 0 within 10 s.
request_ended() {
  local status=0
  wait_for 10 gone "$request" || return 1
  wait "$request" || status=$?
  [ "$status" -eq 0 ]
}

# The app under test, started directly in a process group of its own; its pid goes to $app_pid.
app_pid=
launch() { # launch <log> [VAR=value...]
  local log=$1
  shift
  setsid env "$@" "$app" >>"$work/$log" 2>&1 &
  app_pid=$!
  groups+=("$app_pid")
}
running() { [ -n "$app_pid" ] && ! gone "$app_pid"; }

# W1: through tauri-driver (WebDriver, for W6), or directly when WebDriver has no session.
sid=
wd_error=
export SMOKE_WD_PORT="$wd_port"
setsid "$tauri_driver" --port "$wd_port" --native-port "$((wd_port + 1))" \
  --native-driver "$(command -v WebKitWebDriver)" >"$work/tauri-driver.log" 2>&1 &
driver=$!
groups+=("$driver")
listening() { (exec 3<>"/dev/tcp/127.0.0.1/$wd_port") 2>/dev/null; }
if ! wait_for 10 listening; then
  wd_error="tauri-driver did not listen on port $wd_port within 10 s (see logs/tauri-driver.log)"
elif ! sid=$("${wd[@]}" --timeout 30 session "$app" 2>>"$work/webdriver.log"); then
  wd_error="no WebDriver session: $sid"
  sid=
fi
how="through WebDriver (session $sid)"
if [ -z "$sid" ]; then
  # What WebDriver started must be gone before the direct start, which would hand over to it.
  kill -TERM -- "-$driver" 2>/dev/null || true
  wait_for 10 no_window || true
  launch app-a.log
  how="directly (WebDriver had no session)"
fi
if ! wait_for 30 has_window; then
  result W1 FAIL "no window of the app within 30 s (see logs/app-a.log, logs/tauri-driver.log)"
else
  read -r app_pid app_id shell _ <<<"$(window)"
  if [ "$shell" = xdg_shell ] && [ "$app_id" != - ]; then
    result W1 PASS "native Wayland window: app_id $app_id, shell xdg_shell, pid $app_pid, started $how"
  else
    result W1 FAIL "window of pid $app_pid: app_id '$app_id', shell '$shell' (want xdg_shell)"
  fi
fi

# W2
secret_for W2
if ! running; then
  result W2 FAIL "the app is not running"
elif ! clip_set "$secret"; then
  result W2 FAIL "wl-copy could not put the line on the clipboard"
else
  scrub_request
  if wait_for 5 scrubbed; then
    if request_ended; then
      result W2 PASS "within 5 s of --scrub-clipboard the clipboard starts with DB_PASSWORD=[REDACTED and lacks the password"
    else
      result W2 FAIL "cleaned, but the --scrub-clipboard start did not exit 0 within 10 s"
    fi
  else
    result W2 FAIL "5 s after --scrub-clipboard the clipboard holds $(clip_state)"
  fi
fi

# W3
secret_for W3
if ! running; then
  result W3 FAIL "the app is not running"
else
  printf '%s' "$secret" >"$work/protected.in"
  "$helpers/wayland_offer" "$work/protected.in" >"$work/wayland-offer.log" 2>&1 &
  offer=$!
  pids+=("$offer")
  if ! wait_for 10 grep -qx ready "$work/wayland-offer.log" || ! clip_is "$secret"; then
    result W3 FAIL "the protected copy did not become the clipboard (see logs/wayland-offer.log)"
  else
    scrub_request
    if ! request_ended; then
      result W3 FAIL "the --scrub-clipboard start did not exit 0 within 10 s"
    else
      sleep 2
      if clip_is "$secret" && ! gone "$offer"; then
        result W3 PASS "the copy with x-kde-passwordManagerHint=secret is unchanged 2 s after --scrub-clipboard"
      else
        result W3 FAIL "after --scrub-clipboard the clipboard holds $(clip_state)"
      fi
    fi
  fi
  kill "$offer" 2>/dev/null || true
fi

# W6: no portal on this bus, so Settings shows the command to bind.
note='This desktop does not let apps set a hotkey. Add a shortcut in its keyboard settings that runs: redactor --scrub-clipboard'
if [ -z "$sid" ]; then
  result W6 FAIL "not checked: $wd_error"
elif ! running; then
  result W6 FAIL "the app is not running"
# The settings tree is new, so the first-run card is up: "Get started" closes it.
elif ! c=$("${wd[@]}" click "$sid" '[data-testid=first-run] .start' 2>>"$work/webdriver.log"); then
  result W6 FAIL "could not close the first-run card: $c"
elif ! c=$("${wd[@]}" click "$sid" '[data-testid=settings-open]' 2>>"$work/webdriver.log"); then
  result W6 FAIL "could not open Settings: $c"
elif ! c=$("${wd[@]}" --timeout 5 text "$sid" '[data-testid=hotkey-note]' prefix "$note"); then
  result W6 FAIL "the hotkey note reads '$c'"
elif ! c=$("${wd[@]}" --timeout 5 visible "$sid" '[data-testid=copy-command]'); then
  result W6 FAIL "no Copy button for the command ($c)"
elif ! c=$("${wd[@]}" show "$sid" '[data-testid=hotkey-note]' 2>>"$work/webdriver.log"); then
  result W6 FAIL "could not scroll the note into view for the screenshot: $c"
else
  # For the screenshot only (the check above is done): the sheet's opening motion ends.
  sleep 1
  result W6 PASS "Settings shows the command note, 'redactor --scrub-clipboard' and its Copy button"
fi

# End the first start: what follows needs a second one with the window clipboard.
if [ -n "$sid" ]; then "${wd[@]}" delete "$sid" >/dev/null 2>&1 || true; fi
if running; then swaymsg "[pid=$app_pid] kill" >>"$work/swaymsg.log" 2>&1 || true; fi
wait_for 10 no_window || true
for p in "${groups[@]}"; do kill -TERM -- "-$p" 2>/dev/null || true; done
groups=()
first_ended=yes
wait_for 10 no_window || first_ended=no

# W4 and W5 need focus, so the seat gets a keyboard first.
"$helpers/wayland_keyboard" >"$work/wayland-keyboard.log" 2>&1 &
pids+=($!)
keyboard=yes
wait_for 10 grep -qx ready "$work/wayland-keyboard.log" || keyboard=no
app_pid=
if [ "$first_ended" = yes ] && [ "$keyboard" = yes ]; then
  launch app-b.log REDACTOR_WAYLAND_FOCUSED=1
  wait_for 30 focused || true
fi

# W4: the copy is made while the window is away; sway focuses it again, then the press.
secret_for W4
if [ "$first_ended" = no ]; then
  result W4 FAIL "the first start of the app did not end within 10 s"
elif [ "$keyboard" = no ]; then
  result W4 FAIL "the seat got no keyboard (see logs/wayland-keyboard.log)"
elif ! running || ! focused; then
  result W4 FAIL "no focused window of the app with REDACTOR_WAYLAND_FOCUSED=1 within 30 s (see logs/app-b.log)"
else
  workspace 2
  if ! wait_for 5 unfocused; then
    result W4 FAIL "the window kept the focus after 'workspace 2'"
  elif ! clip_set "$secret"; then
    result W4 FAIL "wl-copy could not put the line on the clipboard"
  else
    workspace 1
    if ! wait_for 5 focused; then
      result W4 FAIL "sway did not focus the window within 5 s of 'workspace 1'"
    else
      scrub_request
      if wait_for 5 scrubbed; then
        if request_ended; then
          result W4 PASS "window clipboard: within 5 s of --scrub-clipboard (window focused) the clipboard is cleaned"
        else
          result W4 FAIL "cleaned, but the --scrub-clipboard start did not exit 0 within 10 s"
        fi
      else
        result W4 FAIL "window clipboard: 5 s after --scrub-clipboard the clipboard holds $(clip_state)"
      fi
    fi
  fi
fi

# W5: the press comes while the window is away; the scrub waits for it.
secret_for W5
if ! running || ! focused; then
  result W5 FAIL "no focused window of the app with REDACTOR_WAYLAND_FOCUSED=1"
else
  workspace 2
  if ! wait_for 5 unfocused; then
    result W5 FAIL "the window kept the focus after 'workspace 2'"
  elif ! clip_set "$secret"; then
    result W5 FAIL "wl-copy could not put the line on the clipboard"
  else
    scrub_request
    if ! request_ended; then
      result W5 FAIL "the --scrub-clipboard start did not exit 0 within 10 s"
    else
      sleep 2
      if ! clip_is "$secret"; then
        result W5 FAIL "the window was away, yet 2 s after --scrub-clipboard the clipboard holds $(clip_state)"
      elif ! unfocused; then
        result W5 FAIL "sway brought the window back by itself: the scrub did not have to wait"
      else
        shot W5-waiting
        workspace 1
        if ! wait_for 5 scrubbed; then
          result W5 FAIL "5 s after 'workspace 1' the clipboard holds $(clip_state)"
        else
          # Once: the next return of the window leaves a new copy alone.
          secret_for W5-again
          workspace 2
          if ! wait_for 5 unfocused || ! clip_set "$secret"; then
            result W5 FAIL "cleaned, but the check for a second scrub could not be set up"
          else
            workspace 1
            if ! wait_for 5 focused; then
              result W5 FAIL "cleaned, but the window did not come back for the second look"
            else
              sleep 2
              if clip_is "$secret"; then
                result W5 PASS "the scrub waited 2 s for the window; 'workspace 1' brought it back and it cleaned, once (the next return left a new copy alone)"
              else
                result W5 FAIL "a second return of the window scrubbed again: the clipboard holds $(clip_state)"
              fi
            fi
          fi
        fi
      fi
    fi
  fi
fi

# W7: quit through the window's close request (no tray host on this bus, so it quits).
bus_name=io.github.its30na.redactor.SingleInstance
has_bus_name() {
  dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus \
    org.freedesktop.DBus.NameHasOwner "string:$bus_name" 2>/dev/null | grep -q 'boolean true'
}
# Sockets in the runtime folder besides the compositor's own two.
our_sockets() {
  find "$runtime" -type s ! -name "$socket" ! -name 'sway-ipc.*.sock' -print 2>/dev/null
}
if ! running; then
  result W7 FAIL "the app is not running"
else
  group=$app_pid
  had_name=no
  has_bus_name && had_name=yes
  swaymsg "[pid=$app_pid] kill" >>"$work/swaymsg.log" 2>&1 || true
  if ! wait_for 5 gone "$app_pid"; then
    result W7 FAIL "the process was still alive 5 s after the close request"
  else
    # The WebKit helpers end shortly after the app.
    group_empty() { ! pgrep -g "$group" >/dev/null; }
    wait_for 5 group_empty || true
    left=$({ pgrep -g "$group" || true; } | wc -l)
    sockets=$(our_sockets)
    if [ "$left" -ne 0 ]; then
      result W7 FAIL "$left processes of the app's group are left 5 s after it ended: $(pgrep -d ' ' -l -g "$group" || true)"
    elif ! no_window; then
      result W7 FAIL "a window of the app is left"
    elif [ -n "$sockets" ]; then
      result W7 FAIL "sockets left in the runtime folder: $sockets"
    elif [ "$had_name" = no ]; then
      result W7 FAIL "the app never held $bus_name, so its release says nothing"
    elif has_bus_name; then
      result W7 FAIL "$bus_name is still owned"
    else
      result W7 PASS "the app ended within 5 s of the close request, its WebKit helpers within 5 s more: no process of its group, no window, no socket in the runtime folder, $bus_name released"
    fi
  fi
fi

total=$((passed + failed + skipped))
summary="wayland-smoke: $passed passed, $failed failed, $skipped skipped on sway ($socket)"
echo "$summary"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  {
    echo "### Wayland app smoke (L3, sway)"
    echo
    echo '```'
    printf '%s\n' "${lines[@]}"
    echo '```'
    echo
    echo "$summary"
  } >>"$GITHUB_STEP_SUMMARY"
fi
# Every check reports exactly once: a run with fewer lines failed somewhere unseen.
[ "$total" -eq 7 ] && [ "$failed" -eq 0 ]
