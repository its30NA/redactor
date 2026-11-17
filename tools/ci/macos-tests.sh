#!/usr/bin/env bash
# The desktop tests on macOS (evidence level L2).
#
# They take over the clipboard of the desktop they run on: whatever it holds is gone
# afterwards. The desktop binary also presses Ctrl+Alt+J and Shift+Cmd+J there and posts
# "screen locked" and "going to sleep" notifications to the apps of the session (an app that
# listens for them reacts; nothing is locked or put to sleep). The tests are `#[ignore]`d and
# refuse to run unless REDACTOR_TEST_PASTEBOARD (and for the desktop binary
# REDACTOR_TEST_DESKTOP) is set. Only this script sets them, and it runs only on a CI runner
# (CI=true) or when told that the clipboard may be taken:
#
#   tools/ci/macos-tests.sh --i-know-this-takes-my-clipboard
#
# Fails unless every listed test binary reports "0 failed" and at least one test passed.
# Lines the tests print as "OBSERVED ..." (what this macOS does where that is not a pass or
# fail) and "not checked ..." (a check that could not be made here, such as a hotkey when the
# desktop does not hand posted keys on) are collected at the end, and in the job summary on
# GitHub.
set -euo pipefail
cd "$(dirname "$0")/../.."

tests=(macos_clipboard macos_desktop)

if [ "$(uname -s)" != "Darwin" ]; then
  echo "macos-tests: these tests need macOS" >&2
  exit 2
fi
if [ "${CI:-}" != "true" ] && [ "${1:-}" != "--i-know-this-takes-my-clipboard" ]; then
  echo "macos-tests: these tests take over the clipboard. On your own Mac run:" >&2
  echo "  tools/ci/macos-tests.sh --i-know-this-takes-my-clipboard" >&2
  exit 2
fi
for tool in pbcopy pbpaste; do
  if ! command -v "$tool" >/dev/null; then
    echo "macos-tests: $tool is missing" >&2
    exit 2
  fi
done

export REDACTOR_TEST_PASTEBOARD=1
export REDACTOR_TEST_DESKTOP=1
# pbcopy and pbpaste take their text encoding from the locale.
export LC_ALL=en_US.UTF-8

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

failed=()
passed=0
for t in "${tests[@]}"; do
  # --show-output: what a passing test printed (the OBSERVED lines) is shown after the run.
  # The other process of the race tests is started by them, not run as a test. The desktop
  # binary has no test harness and ignores the arguments.
  set +e
  cargo test -p redactor-app --locked --test "$t" -- --ignored --test-threads=1 --show-output \
    --skip the_other_process_of_the_race_tests 2>&1 | tee "$work/$t.log"
  status=${PIPESTATUS[0]}
  set -e
  green=$(grep -cE '^test result: ok\. [1-9][0-9]* passed; 0 failed' "$work/$t.log" || true)
  n=$(sed -nE 's/^test result: ok\. ([0-9]+) passed; 0 failed.*/\1/p' "$work/$t.log" | awk '{ n += $1 } END { print n + 0 }')
  if [ "$status" -ne 0 ] || [ "$green" -ne 1 ]; then
    failed+=("$t")
  else
    passed=$((passed + n))
  fi
done

observed=$(cat "$work"/*.log | grep -E '^(OBSERVED|not checked) ' | sort | uniq -c || true)
if [ -n "$observed" ]; then
  echo "macos-tests: observed on $(sw_vers -productVersion) $(uname -m):"
  echo "$observed"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
      echo "### macOS desktop tests: observed on $(sw_vers -productVersion) $(uname -m)"
      echo '```'
      echo "$observed"
      echo '```'
    } >>"$GITHUB_STEP_SUMMARY"
  fi
fi

if [ "${#failed[@]}" -ne 0 ]; then
  echo "macos-tests: FAILED (${failed[*]})" >&2
  exit 1
fi
echo "macos-tests: ok, $passed tests in ${#tests[@]} test binaries"
