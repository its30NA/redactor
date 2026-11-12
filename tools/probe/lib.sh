# shellcheck shell=bash
# Shared helpers for the port probes (sourced, not run). A probe reports one line,
# `PROBE <id> PASS|FAIL <detail>`, on stdout and in the job summary. A probe that fails
# is a result, not an error: scripts exit non-zero only when the script itself broke
# (`probe_broke`), so the workflow shows which probes need a second round.

PROBE_BROKE=0

probe_result() {
  local id=$1 status=$2
  shift 2
  local line="PROBE $id $status $*"
  echo "$line"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    printf -- '- `%s`\n' "$line" >>"$GITHUB_STEP_SUMMARY"
  fi
}

# The script itself broke (a tool it needs is missing, a helper did not compile).
probe_broke() {
  local id=$1
  shift
  PROBE_BROKE=1
  probe_result "$id" FAIL "script broke: $*"
}

probe_exit() {
  exit "$PROBE_BROKE"
}

# wait_for <seconds> <command...>: run the command every half second until it succeeds.
wait_for() {
  local limit=$1
  shift
  local tries=$((limit * 2))
  while [ "$tries" -gt 0 ]; do
    if "$@" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.5
    tries=$((tries - 1))
  done
  return 1
}

# has_socket <dir> <name-glob>: the folder holds a socket matching the glob.
has_socket() {
  find "$1" -maxdepth 1 -name "$2" -type s -print -quit 2>/dev/null | grep -q .
}

# apt_install <package...>: install each package on its own, print the ones that failed.
apt_install() {
  local failed=() pkg
  sudo apt-get update -qq >/dev/null 2>&1 || true
  for pkg in "$@"; do
    if ! sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "$pkg" >/dev/null 2>&1; then
      failed+=("$pkg")
    fi
  done
  echo "${failed[*]}"
}

# Print the tail of a log in a collapsed group, for a probe that failed.
show_log() {
  local name=$1 file=$2
  [ -f "$file" ] || return 0
  echo "::group::log: $name"
  tail -n 40 "$file"
  echo "::endgroup::"
}
