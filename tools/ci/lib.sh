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

# data_left <xdg-root> <secret...>: what an app run left under <xdg-root>/{config,data,cache,state}
# that it should not have. The app may keep its settings.json, and WebKit its (empty)
# hsts-storage.sqlite and files named `salt`. The system libraries the window loads keep their
# own caches in XDG_CACHE_HOME, made of fonts, GPU shaders and plugin lists, never of what the
# app shows: fontconfig, mesa_shader_cache (mesa_shader_cache_db in newer Mesa) and
# gstreamer-1.0; those folders are named here one by one, so any other folder still counts.
# Prints one line per other regular file, then one per file of any kind (those caches too)
# that holds one of the secrets. Names only, relative to <xdg-root>, never contents; nothing
# when there is nothing to say.
data_left() {
  local xdg=$1
  shift
  (
    cd "$xdg" || exit 0
    find config data cache state -type f ! -name settings.json ! -name hsts-storage.sqlite \
      ! -name salt ! -path 'cache/fontconfig/*' ! -path 'cache/mesa_shader_cache/*' \
      ! -path 'cache/mesa_shader_cache_db/*' ! -path 'cache/gstreamer-1.0/*' -print 2>/dev/null || true
    local s
    for s in "$@"; do
      { grep -rlaF -e "$s" config data cache state 2>/dev/null || true; } | sed 's/$/ (holds a secret)/'
    done
  ) | sort -u
}

# remove_work <dir>: remove a script's temporary folder, for its EXIT trap. Never fails, so the
# clean-up cannot change the script's exit status; a folder it cannot remove is named on stderr.
# A bus with the machine's services activates the document portal on the first portal call,
# and it mounts a FUSE file system on $XDG_RUNTIME_DIR/doc that it may still hold for a moment
# after the bus is gone. Every mount under <dir> is unmounted (lazily) first, so rm never walks
# into it or fails on it; then rm is tried for up to 5 s.
remove_work() {
  local dir=$1 tries=10 mount
  [ -n "$dir" ] && [ -e "$dir" ] || return 0
  dir=$(realpath -- "$dir" 2>/dev/null) || return 0
  while :; do
    # Field 5 of mountinfo is the mount point; the deepest goes first.
    while read -r mount; do
      fusermount3 -u -z "$mount" 2>/dev/null || fusermount -u -z "$mount" 2>/dev/null ||
        umount -l "$mount" 2>/dev/null || true
    done < <(awk -v d="$dir" '$5 == d || index($5, d "/") == 1 { print $5 }' \
      /proc/self/mountinfo 2>/dev/null | sort -r)
    rm -rf -- "$dir" 2>/dev/null && return 0
    tries=$((tries - 1))
    if [ "$tries" -eq 0 ]; then
      echo "warning: $dir was not removed: $(rm -rf -- "$dir" 2>&1 | head -n 1)" >&2
      return 0
    fi
    sleep 0.5
  done
}
