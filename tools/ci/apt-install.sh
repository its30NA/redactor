#!/usr/bin/env bash
# Installs Debian packages for a CI job, from a .deb cache when a previous run left one.
#
# Usage: tools/ci/apt-install.sh <cache-dir> <package>...
#
# On 2026-11-11 the Ubuntu mirror failed DNS for over an hour and blocked a release. So:
# - cache hit: the .deb files in <cache-dir> are installed with dpkg, without any mirror access;
# - otherwise (or when the cached set does not install): apt downloads the packages into
#   <cache-dir> (5 retries per file, the whole step tried 3 times, 20 s apart) and installs
#   them, leaving the .deb files for actions/cache to save.
# The cache key must name the runner image, because the set of missing packages depends on it.
# Exit 0 only when every package is installed.
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: $0 <cache-dir> <package>..." >&2
  exit 64
fi
cache="$1"
shift

all_installed() {
  local p
  for p in "$@"; do
    dpkg -s "$p" > /dev/null 2>&1 || return 1
  done
}

if all_installed "$@"; then
  echo "apt-install: all packages are already installed"
  exit 0
fi

mkdir -p "$cache/partial"

if compgen -G "$cache/*.deb" > /dev/null; then
  echo "apt-install: installing $(find "$cache" -maxdepth 1 -name '*.deb' | wc -l) cached .deb files"
  if sudo dpkg -i "$cache"/*.deb && all_installed "$@"; then
    echo "apt-install: done from the cache"
    exit 0
  fi
  echo "apt-install: the cache did not install cleanly; downloading instead"
  rm -f "$cache"/*.deb
fi

apt=(sudo apt-get -o Acquire::Retries=5 -o "Dir::Cache::archives=$cache")
for attempt in 1 2 3; do
  if "${apt[@]}" update &&
    "${apt[@]}" install -y --download-only "$@" &&
    "${apt[@]}" install -y "$@"; then
    break
  fi
  if [ "$attempt" -eq 3 ]; then
    echo "apt-install: apt failed three times" >&2
    exit 1
  fi
  echo "apt-install: attempt $attempt failed, retrying in 20 s"
  sleep 20
done

# apt ran as root: hand the files to the runner user so actions/cache can save them.
sudo rm -rf "$cache/partial" "$cache/lock"
sudo chown -R "$(id -u):$(id -g)" "$cache"

if ! all_installed "$@"; then
  echo "apt-install: some packages are still missing" >&2
  exit 1
fi
echo "apt-install: done, $(find "$cache" -maxdepth 1 -name '*.deb' | wc -l) .deb files cached"
