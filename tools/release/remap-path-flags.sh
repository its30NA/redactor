#!/usr/bin/env bash
# Print the RUSTFLAGS for a release build that keep the runner's paths out of the binaries:
# the checkout, the cargo home (registry and git sources of every dependency) and the rustup
# home are remapped to fixed names. The standard library is already remapped to /rustc/<hash>
# by the official toolchain.
#
# Usage: tools/release/remap-path-flags.sh <workspace-dir>
# Reads CARGO_HOME and RUSTUP_HOME (defaults ~/.cargo and ~/.rustup). Under Git Bash on
# Windows the paths are converted to Windows form, which is what rustc sees.
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <workspace-dir>" >&2
  exit 2
fi

native() {
  if command -v cygpath > /dev/null 2>&1; then
    cygpath -w "$1"
  else
    printf '%s\n' "$1"
  fi
}

workspace="$(native "$1")"
cargo_home="$(native "${CARGO_HOME:-$HOME/.cargo}")"
rustup_home="$(native "${RUSTUP_HOME:-$HOME/.rustup}")"

# RUSTFLAGS is split on whitespace, so a path with a space cannot be passed this way.
for p in "$workspace" "$cargo_home" "$rustup_home"; do
  case "$p" in
    *[[:space:]]*)
      echo "path contains whitespace, cannot remap via RUSTFLAGS: $p" >&2
      exit 1
      ;;
  esac
done

printf -- '--remap-path-prefix=%s=/build --remap-path-prefix=%s=/cargo --remap-path-prefix=%s=/rustup\n' \
  "$workspace" "$cargo_home" "$rustup_home"
