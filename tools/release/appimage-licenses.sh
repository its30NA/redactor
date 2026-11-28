#!/usr/bin/env bash
# Collect the licenses of the libraries an AppImage bundles. The AppImage carries the build
# machine's libraries (WebKitGTK, GTK and their stack from Ubuntu), many of them LGPL, so it
# ships their license texts too: tools/release/bundle-linux.sh puts <out-dir> into the AppImage
# at usr/share/doc/redactor/bundled-libraries.
#
# Usage: tools/release/appimage-licenses.sh <AppImage> <out-dir>
# Run on the machine that built the AppImage (a Debian or Ubuntu system with dpkg): every
# regular *.so* file under the AppImage's usr/lib is looked up in the dpkg database by its
# system path (the multiarch library folder first), and the package's Debian copyright file is
# copied to <out-dir>/<package>.copyright. <out-dir>/INDEX.txt gets one line per library,
# `<path under usr/lib> <package> <version>`, sorted. Fails when a library belongs to no package
# or a package has no copyright file. <out-dir> is emptied first.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <AppImage> <out-dir>" >&2
  exit 2
fi
appimage=$(realpath -- "$1")
out=$2
[ -f "$appimage" ] || {
  echo "appimage-licenses: no such file: $1" >&2
  exit 2
}
for tool in dpkg dpkg-query; do
  command -v "$tool" >/dev/null || {
    echo "appimage-licenses: $tool is missing (run on the Debian or Ubuntu build machine)" >&2
    exit 2
  }
done
multiarch=$(dpkg-architecture -qDEB_HOST_MULTIARCH 2>/dev/null || echo x86_64-linux-gnu)

work=$(mktemp -d "${TMPDIR:-/tmp}/redactor-licenses.XXXXXX")
trap 'rm -rf -- "$work"' EXIT
chmod +x "$appimage" 2>/dev/null || true
(cd "$work" && "$appimage" --appimage-extract >/dev/null)
lib="$work/squashfs-root/usr/lib"
[ -d "$lib" ] || {
  echo "appimage-licenses: the AppImage has no usr/lib" >&2
  exit 1
}

# owner <path under usr/lib>: the package that installed the library on this machine, from its
# system path. dpkg prints `pkg[:arch][, pkg2...]: /path`; a shared path names several packages,
# the first is taken.
owner() {
  local rel=$1 path line
  for path in "/usr/lib/$multiarch/$rel" "/lib/$multiarch/$rel" "/usr/lib/$rel" "/lib/$rel"; do
    line=$(dpkg -S "$path" 2>/dev/null | grep -v '^diversion by' | head -n 1) || true
    if [ -n "$line" ]; then
      line=${line%%: /*}
      line=${line%%,*}
      echo "${line%%:*}"
      return 0
    fi
  done
  # linuxdeploy puts a library it found in a sub-folder (libproxy/libpxbackend-1.0.so) straight
  # into usr/lib: any path of this architecture that ends in the name, when one package owns it.
  local pkgs
  pkgs=$(dpkg -S "*/$rel" 2>/dev/null | grep -v '^diversion by' | grep -F "/$multiarch/" |
    sed 's/: \/.*//; s/,.*//; s/:.*//' | sort -u) || true
  if [ -n "$pkgs" ] && [ "$(printf '%s\n' "$pkgs" | grep -c .)" -eq 1 ]; then
    echo "$pkgs"
    return 0
  fi
  [ -z "$pkgs" ] || echo "appimage-licenses: $rel matches several packages: $(tr "\n" " " <<<"$pkgs")" >&2
  return 1
}

rm -rf -- "$out"
mkdir -p "$out"
status=0
declare -A version_of=()
index=$work/index
: >"$index"
while IFS= read -r rel; do
  if ! pkg=$(owner "$rel"); then
    echo "appimage-licenses: $rel belongs to no installed package" >&2
    status=1
    continue
  fi
  if [ -z "${version_of[$pkg]+x}" ]; then
    version_of[$pkg]=$(dpkg-query -W -f='${Version}' "$pkg" 2>/dev/null ||
      dpkg-query -W -f='${Version}' "$pkg:$(dpkg --print-architecture)")
    if [ -f "/usr/share/doc/$pkg/copyright" ]; then
      cp -L "/usr/share/doc/$pkg/copyright" "$out/$pkg.copyright"
    else
      echo "appimage-licenses: $pkg (for $rel) has no /usr/share/doc/$pkg/copyright" >&2
      status=1
    fi
  fi
  printf '%s %s %s\n' "$rel" "$pkg" "${version_of[$pkg]}" >>"$index"
done < <(cd "$lib" && find . -name '*.so*' -type f | sed 's|^\./||' | LC_ALL=C sort)

{
  echo "# The libraries this AppImage bundles, from the system that built it:"
  echo "# <path under usr/lib> <Debian package> <package version>."
  echo "# Each package's license is in <package>.copyright in this folder."
  cat "$index"
} >"$out/INDEX.txt"

libs=$(grep -c . "$index" || true)
pkgs=${#version_of[@]}
if [ "$status" -ne 0 ]; then
  echo "appimage-licenses: FAILED ($libs libraries listed)" >&2
  exit 1
fi
echo "appimage-licenses: $libs libraries from $pkgs packages, licenses in $out"
