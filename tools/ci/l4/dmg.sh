#!/usr/bin/env bash
# The L4 check for the macOS disk image: the app installs from it as a user installs it (copied
# out of the mounted image) and starts, on Apple silicon and on Intel.
#
# Usage: tools/ci/l4/dmg.sh <Redactor_<v>_universal.dmg> <expected-version> [--i-know-this-takes-my-desktop]
#
# The image is attached read-only, Redactor.app is copied with ditto (which keeps permissions and
# the signature) into a folder of its own, and the image is detached again. Then one line per
# check, `PASS <id>`, `FAIL <id> <reason>` or `SKIP <id> <reason>`; exit 0 when no line is FAIL:
#   drag_target       the image holds the app and a link to /Applications to drag it onto.
#   installed         the copied app has its executable.
#   universal         the executable holds arm64 and x86_64 (lipo -archs).
#   signature         codesign --verify --deep --strict passes on the copy (ad hoc signature).
#   license, notices  LICENSE and THIRD_PARTY_NOTICES.md in Contents/Resources, byte-identical
#                     to the repository's.
#   version_arm64     `Redactor --version` as arm64 prints `Redactor <expected-version>` and exits
#                     0 within 10 s. SKIP on an Intel Mac, which cannot run it.
#   version_x86_64    the same as x86_64: native on an Intel Mac, under Rosetta 2 on Apple
#                     silicon; SKIP with a ::notice:: when Rosetta is not installed there.
#   smoke             tools/ci/macos-smoke.sh on the copied app (window, hotkey, tray, lock,
#                     second launch; screenshots in SMOKE_OUT, default smoke-out). That script
#                     takes over the pasteboard, keyboard and mouse, so it runs only on a CI
#                     runner (CI=true) or with --i-know-this-takes-my-desktop, passed on to it.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=tools/ci/l4/lib.sh
. "$here/lib.sh"

usage() {
  echo "usage: $0 <dmg> <expected-version> [--i-know-this-takes-my-desktop]" >&2
  exit 2
}
[ "$#" -ge 2 ] && [ "$#" -le 3 ] || usage
dmg=$1 expect=$2
# An empty array under set -u needs the +-form: a Mac's own /bin/bash is 3.2.
smoke_flags=()
if [ "$#" -eq 3 ]; then
  [ "$3" = --i-know-this-takes-my-desktop ] || usage
  smoke_flags=("$3")
fi
if [ "$(uname -s)" != Darwin ]; then
  echo "dmg.sh: this needs macOS" >&2
  exit 2
fi
for tool in hdiutil ditto lipo codesign arch perl; do
  command -v "$tool" >/dev/null 2>&1 || {
    fail tool-missing "$tool"
    exit 1
  }
done
[ -f "$dmg" ] || {
  echo "dmg.sh: no such file: $dmg" >&2
  exit 2
}

work=$(mktemp -d "${TMPDIR:-/tmp}/redactor-dmg.XXXXXX")
mounted=
# shellcheck disable=SC2329 # the EXIT trap
cleanup() {
  if [ -n "$mounted" ]; then hdiutil detach -quiet "$mounted" 2>/dev/null || true; fi
  remove_work "$work"
}
trap cleanup EXIT

mkdir "$work/mnt" "$work/Applications"
hdiutil attach -nobrowse -readonly -noautoopen -mountpoint "$work/mnt" "$dmg" >/dev/null
mounted=$work/mnt
if [ -d "$mounted/Redactor.app" ] && [ -L "$mounted/Applications" ] &&
  [ "$(readlink "$mounted/Applications")" = /Applications ]; then
  pass drag_target
else
  fail drag_target "the image holds $(find "$mounted" -mindepth 1 -maxdepth 1 -exec basename {} \; | tr '\n' ' ')(expected Redactor.app and a link to /Applications)"
fi
ditto "$mounted/Redactor.app" "$work/Applications/Redactor.app"
hdiutil detach -quiet "$mounted"
mounted=

app=$work/Applications/Redactor.app
exe=$app/Contents/MacOS/Redactor
if [ ! -x "$exe" ]; then
  fail installed "the copied app has no executable Contents/MacOS/Redactor"
  exit 1
fi
pass installed

archs=$(lipo -archs "$exe" | tr ' ' '\n' | sort | tr '\n' ' ' | sed 's/ $//')
if [ "$archs" = "arm64 x86_64" ]; then
  pass universal
else
  fail universal "the executable holds '$archs', expected arm64 and x86_64"
fi

if out=$(codesign --verify --deep --strict "$app" 2>&1); then
  pass signature
else
  fail signature "$(printf '%s' "$out" | head -n 1)"
fi

for pair in "license:LICENSE" "notices:THIRD_PARTY_NOTICES.md"; do
  id=${pair%%:*} name=${pair#*:}
  if [ ! -f "$app/Contents/Resources/$name" ]; then
    fail "$id" "Contents/Resources/$name is missing"
  elif ! cmp -s "$app/Contents/Resources/$name" "$l4_root/$name"; then
    fail "$id" "Contents/Resources/$name differs from the repository's $name"
  else
    pass "$id"
  fi
done

# version_check <arch>: --version as that architecture, killed after 10 s (macOS has no timeout).
version_check() {
  local a=$1 got rc=0
  got=$(perl -e 'alarm shift; exec @ARGV' 10 arch "-$a" "$exe" --version 2>"$work/version-$a.log") || rc=$?
  if [ "$rc" -ne 0 ]; then
    fail "version_$a" "exit $rc, stderr: $(head -n 1 "$work/version-$a.log")"
  elif [ "$got" != "Redactor $expect" ]; then
    fail "version_$a" "printed '$got', expected 'Redactor $expect'"
  else
    pass "version_$a"
  fi
}
host=$(uname -m)
version_check "$host"
case "$host" in
  arm64)
    if arch -x86_64 /usr/bin/true 2>/dev/null; then
      version_check x86_64
    else
      echo "::notice::the x86_64 half of the app was not started: Rosetta 2 is not installed on this arm64 Mac (the Intel L4 job starts it)"
      echo "SKIP version_x86_64 Rosetta 2 is not installed"
    fi
    ;;
  x86_64) echo "SKIP version_arm64 an Intel Mac cannot run the arm64 half" ;;
esac

if "$l4_root/tools/ci/macos-smoke.sh" "$app" ${smoke_flags[@]+"${smoke_flags[@]}"}; then
  pass smoke
else
  fail smoke "tools/ci/macos-smoke.sh failed (its SMOKE lines above, logs in ${SMOKE_OUT:-smoke-out})"
fi

exit "$l4_failed"
