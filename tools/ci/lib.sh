# shellcheck shell=bash
# Shared helpers for the scripts in tools/ci (sourced, not run). Package installs are not
# here: tools/ci/apt-install.sh does that, with a .deb cache.

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

# show_log <name> <file>: print the tail of a log in a collapsed group, for a step that failed.
show_log() {
  local name=$1 file=$2
  [ -f "$file" ] || return 0
  echo "::group::log: $name"
  tail -n 40 "$file"
  echo "::endgroup::"
}
