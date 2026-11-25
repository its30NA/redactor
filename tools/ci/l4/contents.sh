#!/usr/bin/env bash
# The L4 contents check: a package carries the app, its license and the third-party notices,
# and no path of the machine that built it.
#
# Usage: tools/ci/l4/contents.sh <file | flatpak:<app-id>> [--expect-version <v>]
#   <file> is a .deb, .rpm, .AppImage or .dmg; flatpak:<app-id> is an installed Flatpak.
#
# One line per check, `PASS <id>` or `FAIL <id> <reason>`; exit 0 only when every line is PASS:
#   license, notices   LICENSE and THIRD_PARTY_NOTICES.md are in the app's resources and
#                      byte-identical to the repository's.
#   binary             the app binary is there, executable, and a binary for the format's OS.
#   desktop_file       (Linux) the desktop file starts that binary, names an icon the package
#                      carries, and its StartupWMClass is the window's class (Redactor).
#   no_build_paths     the binary holds none of the build machine's paths: the runner's home,
#                      the cargo registry sources, the rustup toolchains, flatpak-builder's
#                      /run/build/, the caller's HOME.
#   every_bundled_library_has_a_license
#                      (AppImage) every bundled library has an entry in
#                      usr/share/doc/redactor/bundled-libraries/INDEX.txt and its package's
#                      Debian copyright file next to it, and APPIMAGE-PARTS.txt there names
#                      the licenses of the runtime, AppRun and the GTK hook.
#                      (Flatpak) every license folder the manifest installs
#                      (packaging/flatpak/<app-id>.json: /app/share/licenses/<module>) is there
#                      and not empty: one per C library built into the Flatpak.
#   version            (with --expect-version) the package's own version field says <v>; for
#                      an AppImage, which has none, its file name.
# A tool the format needs that is missing here (dpkg-deb, rpm2cpio and cpio, rpm, hdiutil,
# flatpak) is `FAIL tool-missing <tool>`, never a skip.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=tools/ci/l4/lib.sh
. "$here/lib.sh"

usage() {
  echo "usage: $0 <file | flatpak:<app-id>> [--expect-version <v>]" >&2
  exit 2
}
[ "$#" -ge 1 ] || usage
target=$1
shift
expect=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --expect-version)
      [ "$#" -ge 2 ] || usage
      expect=$2
      shift 2
      ;;
    *) usage ;;
  esac
done

work=$(mktemp -d "${TMPDIR:-/tmp}/redactor-contents.XXXXXX")
mounted=
# shellcheck disable=SC2329 # the EXIT trap
cleanup() {
  if [ -n "$mounted" ]; then hdiutil detach -quiet "$mounted" 2>/dev/null || true; fi
  remove_work "$work"
}
trap cleanup EXIT

need() {
  local tool
  for tool in "$@"; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      fail tool-missing "$tool"
      exit 1
    fi
  done
}

# Unpack the package into $root and name the paths inside it: the binary, the resources folder
# and the desktop file (none for macOS).
root=$work/root
desktop=
kind=linux
case "$target" in
  flatpak:*)
    need flatpak
    app_id=${target#flatpak:}
    location=$(flatpak info --show-location "$app_id" 2>/dev/null) || {
      fail installed "flatpak info found no $app_id"
      exit 1
    }
    root=$location/files
    bin=bin/Redactor
    res=share/licenses/$app_id
    desktop=share/applications/$app_id.desktop
    ;;
  *.deb)
    need dpkg-deb
    mkdir "$root"
    dpkg-deb -x "$target" "$root"
    bin=usr/bin/Redactor res=usr/lib/Redactor desktop=usr/share/applications/Redactor.desktop
    ;;
  *.rpm)
    need rpm2cpio cpio
    mkdir "$root"
    abs=$(realpath -- "$target")
    (cd "$root" && rpm2cpio "$abs" | cpio -idm --quiet 2>/dev/null)
    bin=usr/bin/Redactor res=usr/lib/Redactor desktop=usr/share/applications/Redactor.desktop
    ;;
  *.AppImage)
    abs=$(realpath -- "$target")
    [ -x "$abs" ] || {
      fail binary "$target is not executable (chmod +x)"
      exit 1
    }
    # The runtime unpacks without FUSE, into the current folder.
    (cd "$work" && "$abs" --appimage-extract >/dev/null)
    root=$work/squashfs-root
    bin=usr/bin/Redactor res=usr/lib/Redactor desktop=usr/share/applications/Redactor.desktop
    ;;
  *.dmg)
    need hdiutil
    mkdir "$work/mnt"
    hdiutil attach -nobrowse -readonly -noautoopen -mountpoint "$work/mnt" "$target" >/dev/null
    mounted=$work/mnt
    root=$work/mnt
    bin=Redactor.app/Contents/MacOS/Redactor res=Redactor.app/Contents/Resources
    kind=macos
    ;;
  *)
    echo "contents.sh: unknown package type: $target" >&2
    exit 2
    ;;
esac
if [ ! -d "$root" ]; then
  fail unpacked "nothing was unpacked from $target"
  exit 1
fi

for pair in "license:LICENSE" "notices:THIRD_PARTY_NOTICES.md"; do
  id=${pair%%:*} name=${pair#*:}
  if [ ! -f "$root/$res/$name" ]; then
    fail "$id" "$res/$name is missing"
  elif ! cmp -s "$root/$res/$name" "$l4_root/$name"; then
    fail "$id" "$res/$name differs from the repository's $name"
  else
    pass "$id"
  fi
done

# magic <file>: the first four bytes in hex.
magic() { head -c 4 "$1" | od -An -tx1 | tr -d ' \n'; }
if [ ! -f "$root/$bin" ]; then
  fail binary "$bin is missing"
elif [ ! -x "$root/$bin" ]; then
  fail binary "$bin is not executable"
else
  m=$(magic "$root/$bin")
  case "$kind:$m" in
    linux:7f454c46) pass binary ;;
    # Mach-O 64-bit (either byte order) or a universal (fat) binary.
    macos:cffaedfe | macos:feedfacf | macos:cafebabe) pass binary ;;
    *) fail binary "$bin is not a $kind binary (magic $m)" ;;
  esac
fi

if [ "$kind" = linux ]; then
  # field <key>: the value of a key in the desktop file's [Desktop Entry] group.
  field() {
    awk -F= -v k="$1" '/^\[/ { g = ($0 == "[Desktop Entry]") } g && $1 == k { sub(/^[^=]*=/, ""); print; exit }' \
      "$root/$desktop"
  }
  if [ ! -f "$root/$desktop" ]; then
    fail desktop_file "$desktop is missing"
  else
    exec_word=$(field Exec | awk '{ print $1 }')
    icon=$(field Icon)
    wm_class=$(field StartupWMClass)
    if [ "$(field Type)" != Application ]; then
      fail desktop_file "Type is '$(field Type)', expected Application"
    elif [ "$(basename -- "${exec_word:-none}")" != "$(basename -- "$bin")" ]; then
      fail desktop_file "Exec starts '$exec_word', expected $(basename -- "$bin")"
    elif [ -z "$icon" ] || [ -z "$(find "$root" -path "*/icons/hicolor/*" -name "$icon.png" -print -quit)" ]; then
      fail desktop_file "no icon file named '$icon.png' under icons/hicolor"
    elif [ "$wm_class" != Redactor ]; then
      fail desktop_file "StartupWMClass is '$wm_class', expected Redactor (the window's class)"
    else
      pass desktop_file
    fi
  fi
fi

if [ -f "$root/$bin" ]; then
  found=()
  patterns=(/home/runner/ /Users/runner/ /.cargo/registry/src/ /.rustup/toolchains/ /run/build/)
  case "${HOME:-/}" in
    / | "") ;;
    *) patterns+=("${HOME%/}/") ;;
  esac
  for p in "${patterns[@]}"; do
    if grep -qaF -- "$p" "$root/$bin"; then found+=("$p"); fi
  done
  if [ "${#found[@]}" -eq 0 ]; then
    pass no_build_paths
  else
    fail no_build_paths "$bin holds ${found[*]}"
  fi
fi

# every_bundled_library_has_a_license (AppImage): the AppImage carries Ubuntu's libraries, so it
# carries their licenses too (tools/release/appimage-licenses.sh): every *.so* under usr/lib has
# a line `<path under usr/lib> <package> <version>` in INDEX.txt, and <package>.copyright is there.
# APPIMAGE-PARTS.txt covers the runtime, AppRun and the GTK hook script.
case "$target" in
  *.AppImage)
    licenses=usr/share/doc/redactor/bundled-libraries
    if [ ! -f "$root/$licenses/INDEX.txt" ]; then
      fail every_bundled_library_has_a_license "$licenses/INDEX.txt is missing"
    else
      missing=()
      while IFS= read -r lib; do
        pkg=$(awk -v l="$lib" '$1 == l { print $2; exit }' "$root/$licenses/INDEX.txt")
        if [ -z "$pkg" ]; then
          missing+=("$lib (not in INDEX.txt)")
        elif [ ! -s "$root/$licenses/$pkg.copyright" ]; then
          missing+=("$lib ($pkg.copyright missing)")
        fi
      done < <(cd "$root/usr/lib" && find . -name '*.so*' -type f | sed 's|^\./||' | sort)
      # The parts that are not libraries: the runtime, AppRun and the GTK hook
      # (packaging/appimage/APPIMAGE-PARTS.txt).
      if [ ! -s "$root/$licenses/APPIMAGE-PARTS.txt" ]; then
        missing+=("the runtime, AppRun and the GTK hook ($licenses/APPIMAGE-PARTS.txt missing)")
      fi
      if [ "${#missing[@]}" -eq 0 ]; then
        pass every_bundled_library_has_a_license
      else
        fail every_bundled_library_has_a_license "${#missing[@]} without a license, first: ${missing[0]}"
      fi
    fi
    ;;
esac

# (Flatpak) the modules built into it install their licenses under share/licenses/<module>; the
# manifest names each folder, so a module added without its license install fails here.
case "$target" in
  flatpak:*)
    manifest=$l4_root/packaging/flatpak/$app_id.json
    folders=$(grep -o '/app/share/licenses/[A-Za-z0-9._-]*' "$manifest" 2>/dev/null | sed 's|^/app/||' | sort -u || true)
    missing=()
    count=0
    for folder in $folders; do
      count=$((count + 1))
      if [ -z "$(find "$root/$folder" -type f -size +0 -print -quit 2>/dev/null)" ]; then
        missing+=("$folder")
      fi
    done
    if [ "$count" -lt 2 ]; then
      fail every_bundled_library_has_a_license "$manifest names no module license folders"
    elif [ "${#missing[@]}" -eq 0 ]; then
      pass every_bundled_library_has_a_license
    else
      fail every_bundled_library_has_a_license "${#missing[@]} missing or empty, first: ${missing[0]}"
    fi
    ;;
esac

if [ -n "$expect" ]; then
  case "$target" in
    flatpak:*) got=$(flatpak info "$app_id" 2>/dev/null | sed -n 's/^ *Version: *//p' | head -n 1) ;;
    *.deb) got=$(dpkg-deb -f "$target" Version) ;;
    *.rpm)
      need rpm
      got=$(rpm -qp --qf '%{VERSION}' "$target" 2>/dev/null)
      ;;
    *.AppImage)
      got=$(basename -- "$target" | sed -n 's/^Redactor_\([^_]*\)_.*\.AppImage$/\1/p')
      ;;
    *.dmg) got=$(plutil -extract CFBundleShortVersionString raw -o - "$root/Redactor.app/Contents/Info.plist" 2>/dev/null) ;;
  esac
  if [ "$got" = "$expect" ]; then
    pass version
  else
    fail version "the package says '$got', expected '$expect'"
  fi
fi

exit "$l4_failed"
