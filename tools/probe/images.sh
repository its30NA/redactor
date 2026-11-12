#!/usr/bin/env bash
# P1: what a runner image offers (glibc, WebKitGTK 4.1 in apt, macOS version, CPU).
# Usage: tools/probe/images.sh <runner-label>
set -uo pipefail
. "$(dirname "$0")/lib.sh"

runner=${1:?runner label}
id="P1-$runner"

case "$(uname -s)" in
  Linux)
    . /etc/os-release
    glibc=$(ldd --version 2>/dev/null | head -n 1 | awk '{print $NF}')
    sudo apt-get update -qq >/dev/null 2>&1 || true
    webkit=$(apt-cache policy libwebkit2gtk-4.1-0 2>/dev/null | awk '/Candidate:/ {print $2}')
    if [ -n "$glibc" ] && [ -n "$webkit" ] && [ "$webkit" != "(none)" ]; then
      probe_result "$id" PASS "$PRETTY_NAME $(uname -m); glibc $glibc; libwebkit2gtk-4.1-0 $webkit"
    else
      probe_result "$id" FAIL "$PRETTY_NAME $(uname -m); glibc ${glibc:-?}; libwebkit2gtk-4.1-0 ${webkit:-not in apt}"
    fi
    ;;
  Darwin)
    probe_result "$id" PASS "macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion)) $(uname -m); Xcode $(xcodebuild -version 2>/dev/null | head -n 1 | awk '{print $2}')"
    ;;
  *)
    probe_broke "$id" "unknown OS $(uname -s)"
    ;;
esac

probe_exit
