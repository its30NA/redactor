#!/usr/bin/env bash
# The built app under test on macOS (evidence level L3): the .app bundle, started with a
# throwaway home, driven by posted keys and clicks (tools/ci/macos/*.swift), checked through
# the pasteboard and the window list. Made-up text only.
#
# It takes over the desktop it runs on: the pasteboard, the keyboard and the mouse, and it posts
# "screen locked" to the apps of the session. So it runs only on a CI runner (CI=true) or when
# told that this is fine:
#
#   tools/ci/macos-smoke.sh <Redactor.app> [--i-know-this-takes-my-desktop]   the checks S1-S11
#   tools/ci/macos-smoke.sh --self-test [--i-know-this-takes-my-desktop]      the set-up only
#                                                                            (helpers, pasteboard,
#                                                                            window list, posted keys,
#                                                                            screenshots), no app
# Environment:
#   SMOKE_OUT=<dir>   screenshots and logs (default smoke-out, emptied first)
#
# Every run starts with the key probe K0: a Carbon hotkey in a helper (the API the app's hotkey
# uses) must fire on a posted Ctrl+Option+J. Where it does not, this machine does not hand posted
# keys on, and a check that needs a posted key and fails is reported SKIP, not FAIL ("not tested
# at L3"); its L2 tests (tools/ci/macos-tests.sh) stay. A check that passes passes either way.
#
# Prints one line per check, `SMOKE <id> PASS|FAIL|SKIP <detail>`, also into
# $GITHUB_STEP_SUMMARY, and takes a screenshot after every check (screencapture; without the
# Screen Recording permission it may show only the wallpaper). Exit 1 if any check failed.
set -euo pipefail
cd "$(dirname "$0")/../.."
root=$PWD

self_test=0
bundle=
allowed=0
for arg in "$@"; do
  case "$arg" in
    --self-test) self_test=1 ;;
    --i-know-this-takes-my-desktop) allowed=1 ;;
    -*)
      echo "usage: $0 <Redactor.app> | --self-test" >&2
      exit 64
      ;;
    *) bundle=$arg ;;
  esac
done
if [ "$self_test" -eq 0 ] && [ -z "$bundle" ]; then
  echo "usage: $0 <Redactor.app> | --self-test" >&2
  exit 64
fi
if [ "$(uname -s)" != "Darwin" ]; then
  echo "macos-smoke: this needs macOS" >&2
  exit 2
fi
if [ "${CI:-}" != "true" ] && [ "$allowed" -ne 1 ]; then
  echo "macos-smoke: this takes over the pasteboard, keyboard and mouse. On your own Mac run:" >&2
  echo "  $0 ${bundle:---self-test} --i-know-this-takes-my-desktop" >&2
  exit 2
fi
for tool in swiftc pbcopy pbpaste screencapture open pgrep /usr/libexec/PlistBuddy; do
  if ! command -v "$tool" >/dev/null; then
    echo "macos-smoke: $tool is missing" >&2
    exit 2
  fi
done
exe=
if [ "$self_test" -eq 0 ]; then
  bundle=$(cd "$bundle" 2>/dev/null && pwd) || {
    echo "macos-smoke: $bundle is not a directory" >&2
    exit 2
  }
  name=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$bundle/Contents/Info.plist" 2>/dev/null || true)
  bundle_id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$bundle/Contents/Info.plist" 2>/dev/null || true)
  exe="$bundle/Contents/MacOS/$name"
  if [ -z "$name" ] || [ -z "$bundle_id" ] || [ ! -x "$exe" ]; then
    echo "macos-smoke: $bundle is not an app bundle with an executable" >&2
    exit 2
  fi
fi
# pbcopy and pbpaste take their text encoding from the locale.
export LC_ALL=en_US.UTF-8

out=${SMOKE_OUT:-smoke-out}
rm -rf "$out"
mkdir -p "$out/logs"
out=$(cd "$out" && pwd)
work=$(mktemp -d)
pids=()
cleanup() {
  local p
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  if [ -n "$exe" ]; then pkill -f -- "$exe" 2>/dev/null || true; fi
  wait 2>/dev/null || true
  cp "$work"/*.log "$out/logs/" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

fail() {
  echo "macos-smoke: $1" >&2
  shift
  local log
  for log in "$@"; do
    [ -f "$log" ] && tail -n 20 "$log" >&2
  done
  exit 1
}

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

# The helpers, built once (swift <file> would compile them again on every call).
bin="$work/bin"
mkdir -p "$bin"
for helper in keys windows notify concealed hotkey; do
  swiftc -O -o "$bin/$helper" "$root/tools/ci/macos/$helper.swift" >"$work/swiftc-$helper.log" 2>&1 ||
    fail "the helper $helper.swift did not build" "$work/swiftc-$helper.log"
done
key() { "$bin/keys" key "$@" >>"$work/keys.log" 2>&1; }

lines=()
passed=0
failed=0
skipped=0
shot() {
  screencapture -x "$out/$1.png" 2>>"$work/screencapture.log" ||
    echo "macos-smoke: no screenshot for $1 (see logs/screencapture.log)"
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
# keyed <id> <ok> <pass detail> <fail detail>: a check that needs posted keys. A failure is SKIP
# where the key probe K0 showed that posted keys do not arrive on this machine.
keys_arrive=0
keyed() {
  if [ "$2" -eq 1 ]; then
    result "$1" PASS "$3"
  elif [ "$keys_arrive" -eq 0 ]; then
    result "$1" SKIP "not tested at L3: posted keys do not arrive here (K0); it failed: $4"
  else
    result "$1" FAIL "$4"
  fi
}

# The pasteboard. `count` is its change count: it rises with every copy, by anyone.
clip_get() { pbpaste 2>/dev/null; }
clip_is() { [ "$(clip_get)" = "$1" ]; }
clip_set() {
  printf '%s' "$1" | pbcopy
  clip_is "$1"
}
count() { "$bin/concealed" count; }

# Made-up text, built at run time.
token=$(od -An -N9 -tx1 /dev/urandom | tr -d ' \n')
secret_value="Smk${token}Qz7"
secret="DB_PASSWORD=$secret_value"
clean="lunch at noon $$"
sentinel="smoke sentinel $$"
scrubbed() {
  local c
  c=$(clip_get) && [[ $c == "DB_PASSWORD=[REDACTED"* && $c != *"$secret_value"* ]]
}
what_is_there() { # a short description of the pasteboard for a FAIL line, never the secret
  local c
  c=$(clip_get || true)
  if [[ $c == *"$secret_value"* ]]; then echo "the pasteboard still holds the password"; else echo "the pasteboard holds ${#c} chars"; fi
}

# K0: posted keys reach a Carbon hotkey.
"$bin/hotkey" 10 >"$work/hotkey.out" 2>&1 &
probe=$!
pids+=("$probe")
probe_said() { grep -q "^$1" "$work/hotkey.out"; }
if ! wait_for 5 probe_said ready; then
  if [ "$self_test" -eq 1 ]; then k0=FAIL; else k0=SKIP; fi
  result K0 "$k0" "the probe did not register Ctrl+Option+J: $(head -c 200 "$work/hotkey.out")"
else
  key j ctrl alt || true
  if wait_for 5 probe_said fired; then
    keys_arrive=1
    result K0 PASS "a posted Ctrl+Option+J fired a Carbon hotkey within 5 s"
  else
    if [ "$self_test" -eq 1 ]; then k0=FAIL; else k0=SKIP; fi
    result K0 "$k0" "a posted Ctrl+Option+J did not fire the hotkey within 5 s: checks that need posted keys report SKIP when they fail"
  fi
fi
kill "$probe" 2>/dev/null || true

if [ "$self_test" -eq 1 ]; then
  # T1 the pasteboard round trip, and its change count rises
  before=$(count)
  if clip_set "$clean" && [ "$(count)" -gt "$before" ]; then
    result T1 PASS "pbcopy and pbpaste agree on ${#clean} chars; the change count rose"
  else
    result T1 FAIL "pbpaste read back something else, or the change count did not rise"
  fi

  # T2 a concealed copy reads back as text
  if c=$("$bin/concealed" copy "$clean" 2>>"$work/concealed.log") && clip_is "$clean"; then
    result T2 PASS "concealed copy made (change count $c), pbpaste reads its text"
  else
    result T2 FAIL "the concealed copy was refused or does not read back (see logs/concealed.log)"
  fi

  # T3 the window list
  n=$("$bin/windows" list 2>>"$work/windows.log" | grep -c . || true)
  if [ "$n" -gt 0 ]; then
    result T3 PASS "$n windows listed; frontmost app pid $("$bin/windows" frontmost)"
  else
    result T3 FAIL "the window list is empty (see logs/windows.log)"
  fi

  # T4 a distributed notification can be posted
  if "$bin/notify" org.example.redactor.smoke >>"$work/notify.log" 2>&1; then
    result T4 PASS "a distributed notification was posted"
  else
    result T4 FAIL "notify failed (see logs/notify.log)"
  fi

  # T5 screenshots
  screencapture -x "$work/t5.png" 2>>"$work/screencapture.log" || true
  if [ -s "$work/t5.png" ]; then
    result T5 PASS "screencapture wrote $(wc -c <"$work/t5.png" | tr -d ' ') bytes"
  else
    result T5 FAIL "screencapture wrote nothing (see logs/screencapture.log)"
  fi
else
  # The app's settings go to a throwaway home: tray mode on (closing hides the window), wipe on
  # lock on, and the first-run card already seen (an open card holds the keys back).
  home="$work/home"
  mkdir -p "$home/Library/Application Support/Redactor"
  printf '%s\n' '{"trayMode":true,"wipeOnLock":true,"firstRunDone":true}' \
    >"$home/Library/Application Support/Redactor/settings.json"

  launched=
  launch() { # launch <log>: start the bundle's executable; its pid goes to $launched
    HOME="$home" CFFIXED_USER_HOME="$home" "$exe" >>"$work/$1" 2>&1 &
    launched=$!
    pids+=("$launched")
  }
  # Windows of <pid> at layer 0 (ordinary windows; the menu bar item is a different layer).
  windows_of() { "$bin/windows" list 2>>"$work/windows.log" | awk -F'\t' -v pid="$1" '$5 == pid && $2 == 0'; }
  shown() { windows_of "$1" | awk -F'\t' '$4 == "true" { split($3, s, "x"); if (s[1] >= 200 && s[2] >= 200) found = 1 } END { exit !found }'; }
  shown_full() { windows_of "$1" | awk -F'\t' '$4 == "true" { split($3, s, "x"); if (s[1] >= 960 && s[2] >= 640) found = 1 } END { exit !found }'; }
  not_shown() { ! shown "$1"; }
  front() { "$bin/windows" activate "$1" >>"$work/windows.log" 2>&1; }
  instances() { pgrep -f -- "$exe" | grep -c . || true; }

  # S1
  launch app-s1.log
  app=$launched
  if wait_for 30 shown_full "$app"; then
    result S1 PASS "a window of $name (pid $app), layer 0, $(windows_of "$app" | awk -F'\t' '$4 == "true" { print $3; exit }'), on screen within 30 s"
  elif gone "$app"; then
    result S1 FAIL "the app exited (see logs/app-s1.log)"
  else
    result S1 FAIL "no window of at least 960x640 on screen within 30 s; its windows: $(windows_of "$app" | tr '\t\n' ' ;')"
  fi
  running() { ! gone "$app"; }
  # The page loads its settings and app_info after the window shows.
  sleep 3

  # S2 the hotkey end to end
  s2=0
  if ! running; then
    result S2 FAIL "the app is not running"
  elif ! clip_set "$secret"; then
    result S2 FAIL "pbcopy did not take the line"
  else
    key r ctrl alt || true
    if wait_for 5 scrubbed; then
      s2=1
      keyed S2 1 "within 5 s of Ctrl+Option+R the pasteboard starts with DB_PASSWORD=[REDACTED and lacks the password" ""
    else
      keyed S2 0 "" "5 s after Ctrl+Option+R $(what_is_there)"
    fi
  fi

  # S3 and S4: what the hotkey must leave alone. Only meaningful once the hotkey fired (S2).
  for check in S3 S4; do
    if [ "$s2" -ne 1 ]; then
      result "$check" SKIP "the hotkey did not fire in S2, so a text left alone proves nothing"
      continue
    fi
    if [ "$check" = S3 ]; then
      text=$clean
      clip_set "$text" || true
      before=$(count)
    else
      text=$secret
      before=$("$bin/concealed" copy "$text" 2>>"$work/concealed.log") || before=refused
    fi
    key r ctrl alt || true
    sleep 2
    if clip_is "$text" && [ "$(count)" = "$before" ]; then
      if [ "$check" = S3 ]; then detail="clean text"; else detail="a concealed copy"; fi
      result "$check" PASS "$detail identical and the change count unchanged 2 s after the hotkey"
    else
      result "$check" FAIL "change count $before -> $(count); $(what_is_there)"
    fi
  done

  # S5 Cmd keys reach the page: paste and scrub, then copy the safe text.
  paste_and_copy() {
    key v cmd || true
    sleep 2
    key c cmd shift || true
    wait_for 5 scrubbed
  }
  s5=0
  if ! running; then
    result S5 FAIL "the app is not running"
  else
    activated=$(front "$app" && echo "front" || echo "not front")
    clip_set "$secret" || true
    if paste_and_copy; then
      s5=1
      keyed S5 1 "Cmd+V then Shift+Cmd+C: the pasteboard starts with DB_PASSWORD=[REDACTED ($activated)" ""
    else
      keyed S5 0 "" "after Cmd+V and Shift+Cmd+C $(what_is_there) (the app was $activated)"
    fi
  fi

  # S6 Cmd+V cannot paste a concealed copy.
  if [ "$s5" -ne 1 ]; then
    result S6 SKIP "Cmd+V did not reach the page in S5, so a paste that does nothing proves nothing"
  else
    key x cmd shift || true
    sleep 1
    before=$("$bin/concealed" copy "$secret" 2>>"$work/concealed.log") || before=refused
    key v cmd || true
    sleep 2
    key c cmd shift || true
    sleep 2
    if [ "$(count)" = "$before" ] && clip_is "$secret"; then
      result S6 PASS "after Shift+Cmd+X, a concealed copy, Cmd+V and Shift+Cmd+C the change count is unchanged"
    else
      result S6 FAIL "change count $before -> $(count): something was pasted or copied"
    fi
  fi

  # S7 wipe on lock: text in the panes, "screen locked", then Copy finds nothing to copy.
  if [ "$s5" -ne 1 ]; then
    result S7 SKIP "Cmd+V did not reach the page in S5, so empty panes after the lock prove nothing"
  else
    clip_set "$secret" || true
    if ! paste_and_copy; then
      result S7 FAIL "the panes did not take the line before the lock: $(what_is_there)"
    else
      clip_set "$sentinel" || true
      "$bin/notify" com.apple.screenIsLocked >>"$work/notify.log" 2>&1 || true
      sleep 2
      key c cmd shift || true
      sleep 1
      if clip_is "$sentinel"; then
        result S7 PASS "after com.apple.screenIsLocked Shift+Cmd+C left the sentinel: both panes were empty"
      else
        result S7 FAIL "after com.apple.screenIsLocked Shift+Cmd+C copied text: the panes still held it"
      fi
    fi
  fi

  # S8 close to the menu bar, then a second launch shows the window.
  if ! running; then
    result S8 FAIL "the app is not running"
  else
    front "$app" || true
    key w cmd || true
    if ! wait_for 5 not_shown "$app"; then
      keyed S8 0 "" "the window was still on screen 5 s after Cmd+W"
    elif ! running; then
      result S8 FAIL "the process exited on Cmd+W although tray mode is on"
    else
      shot S8-hidden
      launch app-s8-second.log
      second=$launched
      if ! wait_for 10 gone "$second"; then
        result S8 FAIL "the second launch did not exit within 10 s"
      elif ! wait_for 5 shown "$app"; then
        result S8 FAIL "the second launch exited, but the window stayed hidden"
      elif ! running; then
        result S8 FAIL "the first process is gone after the second launch"
      else
        n=$(instances)
        if [ "$n" = 1 ]; then
          keyed S8 1 "Cmd+W hid the window (process alive); a second launch exited and showed it; one process" ""
        else
          result S8 FAIL "the window is back, but $n processes run $name"
        fi
      fi
    fi
  fi

  # S9 a click on the Dock icon: `open` on the running app sends it the reopen event.
  if ! running; then
    result S9 FAIL "the app is not running"
  else
    front "$app" || true
    key w cmd || true
    if ! wait_for 5 not_shown "$app"; then
      keyed S9 0 "" "Cmd+W did not hide the window, so the reopen could not be tested"
    else
      open "$bundle" >>"$work/open.log" 2>&1 || true
      others=0
      for _ in $(seq 1 50); do
        if [ "$(instances)" -gt 1 ]; then others=1; fi
        if shown "$app"; then break; fi
        sleep 0.2
      done
      if [ "$others" -eq 1 ]; then
        result S9 FAIL "open started a second process (the single-instance path), not the reopen event"
      elif shown "$app"; then
        result S9 PASS "after Cmd+W, open $name.app showed the window again (the reopen event), no second process"
      else
        result S9 FAIL "the window stayed hidden 10 s after open"
      fi
    fi
  fi

  # S10 a right-click opens no menu. The picture decides: the lead looks at S10-right-click.png.
  if ! running; then
    result S10 FAIL "the app is not running"
  else
    front "$app" || true
    middle=$(windows_of "$app" | awk -F'\t' '$4 == "true" { split($3, s, "x"); split($6, p, ","); print p[1] + int(s[1] / 2), p[2] + int(s[2] / 2); exit }')
    read -r cx cy <<<"$middle" || true
    if [ -z "${cx:-}" ] || [ -z "${cy:-}" ]; then
      result S10 FAIL "no window on screen to click"
    elif ! "$bin/keys" rightclick "$cx" "$cy" >>"$work/keys.log" 2>&1; then
      result S10 FAIL "the right-click could not be posted (see logs/keys.log)"
    else
      sleep 1
      shot S10-right-click
      key escape || true
      result S10 PASS "right-click posted at ($cx, $cy); look at S10-right-click.png: no menu"
    fi
  fi

  # S11 Cmd+Q quits, and the web view left no data behind.
  if ! running; then
    result S11 FAIL "the app is not running"
  else
    front "$app" || true
    key q cmd || true
    if ! wait_for 5 gone "$app"; then
      keyed S11 0 "" "the process was still alive 5 s after Cmd+Q"
      kill "$app" 2>/dev/null || true
    else
      left=$(
        {
          find "$home" \( -name WebsiteData -o -name LocalStorage -o -name IndexedDB -o -name '*.binarycookies' \) 2>/dev/null
          for d in WebKit Caches HTTPStorages; do
            [ -d "$HOME/Library/$d/$bundle_id" ] &&
              find "$HOME/Library/$d/$bundle_id" \( -name WebsiteData -o -name LocalStorage -o -name IndexedDB \) 2>/dev/null
          done
          ls "$HOME/Library/Cookies/$bundle_id.binarycookies" "$HOME/Library/HTTPStorages/$bundle_id.binarycookies" 2>/dev/null
        } | sed "s|^$home|<scratch home>|; s|^$HOME|~|" || true
      )
      if [ -z "$left" ]; then
        keyed S11 1 "Cmd+Q ended the process within 5 s; no WebsiteData, LocalStorage, IndexedDB or cookie file" ""
      else
        result S11 FAIL "the process ended, but it left web data: $(echo "$left" | tr '\n' ' ')"
      fi
    fi
  fi
fi

total=$((passed + failed + skipped))
summary="macos-smoke: $passed passed, $failed failed, $skipped skipped on macOS $(sw_vers -productVersion) $(uname -m)"
echo "$summary"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  {
    if [ "$self_test" -eq 1 ]; then echo "### macOS smoke set-up (self-test)"; else echo "### macOS app smoke (L3)"; fi
    echo
    echo '```'
    printf '%s\n' "${lines[@]}"
    echo '```'
    echo
    echo "$summary"
    if [ "$skipped" -gt 0 ]; then
      echo
      echo "**Known limits:** the SKIP checks were not tested at L3 on this runner; their L2 tests stay."
    fi
  } >>"$GITHUB_STEP_SUMMARY"
fi
[ "$total" -gt 0 ] && [ "$failed" -eq 0 ]
