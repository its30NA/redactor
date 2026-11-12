#!/usr/bin/env bash
# P6: the macOS desktop on a CI runner. (a) a concealed pasteboard write seen from a second
# process; (b) a Carbon hot key reached by a posted key event; (c) com.apple.screenIsLocked
# delivered between processes; (d) screencapture gives a picture that is not black.
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/lib.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Build every helper first; one that does not compile is a broken script, not a result.
for helper in pasteboard hotkey lock imagecheck; do
  if ! swiftc -O -o "$work/$helper" "$here/macos/$helper.swift" >"$work/$helper.build.log" 2>&1; then
    show_log "swiftc $helper" "$work/$helper.build.log"
    probe_broke "P6-$helper" "swiftc failed"
  fi
done

# (a) Pasteboard: types and changeCount as a second process sees them.
if [ -x "$work/pasteboard" ]; then
  text="redactor-probe-$RANDOM-$RANDOM"
  wrote=$("$work/pasteboard" write "$text")
  read_out=$("$work/pasteboard" read)
  wrote_count=$(printf '%s\n' "$wrote" | sed -n 's/^changeCount=\([0-9]*\).*/\1/p')
  read_count=$(printf '%s\n' "$read_out" | sed -n 's/^changeCount=//p')
  types=$(printf '%s\n' "$read_out" | sed -n 's/^types=//p')
  got=$(printf '%s\n' "$read_out" | sed -n 's/^text=//p')
  detail="writer: $wrote; reader changeCount=$read_count; types=$types"
  if [ "$got" = "$text" ] && [ "$wrote_count" = "$read_count" ] &&
    printf '%s' "$types" | grep -q 'org.nspasteboard.ConcealedType'; then
    probe_result P6a PASS "$detail"
  else
    probe_result P6a FAIL "$detail; text matches: $([ "$got" = "$text" ] && echo yes || echo no)"
  fi
fi

# (b) Carbon hot key fired by CGEventPost.
if [ -x "$work/hotkey" ]; then
  out=$("$work/hotkey" 2>&1 | tr '\n' ' ') # exits by itself after 2.5 s (macOS has no `timeout`)
  if printf '%s' "$out" | grep -q 'fired=yes'; then
    probe_result P6b PASS "$out"
  else
    probe_result P6b FAIL "${out:-no output}"
  fi
fi

# (c) Distributed notification between two processes.
if [ -x "$work/lock" ]; then
  "$work/lock" listen >"$work/lock.out" 2>&1 &
  listener=$!
  if wait_for 5 grep -q ready "$work/lock.out"; then
    "$work/lock" post >/dev/null 2>&1
  fi
  wait "$listener" 2>/dev/null
  if grep -q 'received=yes' "$work/lock.out"; then
    probe_result P6c PASS "com.apple.screenIsLocked reached a second process"
  else
    probe_result P6c FAIL "listener: $(tr '\n' ' ' <"$work/lock.out")"
  fi
fi

# (d) A screenshot that is not black.
if screencapture -x "$work/shot.png" 2>"$work/shot.log" && [ -s "$work/shot.png" ]; then
  stats=$([ -x "$work/imagecheck" ] && "$work/imagecheck" "$work/shot.png" || echo "imagecheck missing")
  brightest=$(printf '%s' "$stats" | sed -n 's/.*brightest=\([0-9]*\).*/\1/p')
  if [ -n "$brightest" ] && [ "$brightest" -gt 16 ]; then
    probe_result P6d PASS "$stats"
  else
    probe_result P6d FAIL "$stats"
  fi
else
  probe_result P6d FAIL "screencapture: $(tr '\n' ' ' <"$work/shot.log")"
fi

probe_exit
