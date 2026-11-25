#!/usr/bin/env python3
"""The Flatpak manifest asks for no more than the app uses, and every download is pinned.

Usage: python3 -I tools/ci/l4/flatpak-manifest-check.py <manifest.json>

Standard library only. One line per check, `PASS <id>` or `FAIL <id> <reason>`; exit 0 only when
every line is PASS:
  runtime        runtime-version is a string (YAML or a careless edit turns 51 into a number
                 and 50.1 into a float), and the runtime and SDK are GNOME's.
  finish_args    finish-args are exactly the approved permissions, each once: no network, no
                 file system, no x11 socket besides the Wayland fallback, no sound.
  sources_pinned every archive and file source (also in the generated source lists the manifest
                 names, and in nested modules) has a sha256 or sha512, every git source a commit,
                 nothing is fetched over plain http, and every local path (patches, the
                 repository) exists.
  build_offline  the app's module builds with --offline / --locked and no network permission.
"""

import json
import os
import sys

APPROVED_FINISH_ARGS = [
    "--share=ipc",
    "--socket=wayland",
    "--socket=fallback-x11",
    "--device=dri",
    "--talk-name=org.kde.StatusNotifierWatcher",
    "--talk-name=org.freedesktop.Notifications",
    "--talk-name=org.freedesktop.ScreenSaver",
    "--talk-name=org.gnome.ScreenSaver",
    "--talk-name=org.cinnamon.ScreenSaver",
    "--talk-name=org.mate.ScreenSaver",
    "--talk-name=org.xfce.ScreenSaver",
    "--system-talk-name=org.freedesktop.login1",
]

failed = False


def result(check, problem):
    global failed
    if problem:
        failed = True
        print(f"FAIL {check} {problem}")
    else:
        print(f"PASS {check}")


def modules(mods, base):
    """Every module, nested ones too, with the folder its relative paths start from."""
    for mod in mods:
        if isinstance(mod, str):
            path = os.path.join(base, mod)
            with open(path, encoding="utf-8") as f:
                yield from modules([json.load(f)], os.path.dirname(path))
            continue
        yield mod, base
        yield from modules(mod.get("modules", []), base)


def sources(mod, base):
    """A module's sources; a string names a JSON file with a list of them (flatpak-builder's
    way of including generated sources)."""
    for src in mod.get("sources", []):
        if isinstance(src, str):
            path = os.path.join(base, src)
            with open(path, encoding="utf-8") as f:
                listed = json.load(f)
            for item in listed if isinstance(listed, list) else [listed]:
                yield src, item
        else:
            yield mod.get("name", "?"), src


def is_hex(value, length):
    return len(value) == length and all(c in "0123456789abcdef" for c in value)


def source_problem(origin, src, base):
    kind = src.get("type")
    local = ([src["path"]] if "path" in src else []) + list(src.get("paths", []))
    for rel in local:
        if not os.path.exists(os.path.join(base, rel)):
            return f"{origin}: {rel} does not exist next to the manifest"
    urls = [src[k] for k in ("url",) if k in src] + list(src.get("mirror-urls", []))
    for url in urls:
        if not url.startswith("https://"):
            return f"{origin}: {url} is not https"
    if kind in ("archive", "file") and "url" in src:
        # flatpak-builder checks either; the npm generator writes the registry's sha512.
        if not (is_hex(src.get("sha256", ""), 64) or is_hex(src.get("sha512", ""), 128)):
            return f"{origin}: {src['url']} has no sha256 or sha512"
    if kind == "git":
        commit = src.get("commit", "")
        if len(commit) != 40:
            return f"{origin}: git {src.get('url')} is not pinned to a commit"
    if kind in ("bzr", "svn", "extra-data"):
        return f"{origin}: a {kind} source cannot be pinned by hash"
    return None


def main():
    if len(sys.argv) != 2:
        print("usage: flatpak-manifest-check.py <manifest.json>", file=sys.stderr)
        return 2
    path = sys.argv[1]
    try:
        with open(path, encoding="utf-8") as f:
            manifest = json.load(f)
    except (OSError, ValueError) as e:
        print(f"FAIL manifest cannot read {path}: {e}")
        return 1
    base = os.path.dirname(os.path.abspath(path))

    rv = manifest.get("runtime-version")
    if not isinstance(rv, str):
        result("runtime", f"runtime-version is {rv!r}, not a string")
    elif manifest.get("runtime") != "org.gnome.Platform" or manifest.get("sdk") != "org.gnome.Sdk":
        result("runtime", "runtime and sdk must be org.gnome.Platform and org.gnome.Sdk")
    else:
        result("runtime", None)

    args = manifest.get("finish-args", [])
    extra = [a for a in args if a not in APPROVED_FINISH_ARGS]
    missing = [a for a in APPROVED_FINISH_ARGS if a not in args]
    dupes = sorted({a for a in args if args.count(a) > 1})
    if extra:
        result("finish_args", f"not approved: {' '.join(extra)}")
    elif missing:
        result("finish_args", f"missing: {' '.join(missing)}")
    elif dupes:
        result("finish_args", f"listed twice: {' '.join(dupes)}")
    else:
        result("finish_args", None)

    problems = []
    try:
        all_modules = list(modules(manifest.get("modules", []), base))
        for mod, mbase in all_modules:
            for origin, src in sources(mod, mbase):
                p = source_problem(origin, src, mbase)
                if p:
                    problems.append(p)
    except (OSError, ValueError) as e:
        all_modules = []
        problems.append(f"cannot read a listed file: {e}")
    if not all_modules and not problems:
        problems.append("no modules")
    result("sources_pinned", f"{len(problems)} problem(s), first: {problems[0]}" if problems else None)

    app = [m for m, _ in all_modules if m.get("name") == "redactor"]
    online = [
        m.get("name", "?")
        for m in [manifest] + [m for m, _ in all_modules]
        if "--share=network" in m.get("build-options", {}).get("build-args", [])
    ]
    if online:
        result("build_offline", f"asks for the network while building: {' '.join(online)}")
    elif len(app) != 1:
        result("build_offline", "no single module named redactor")
    else:
        commands = " ".join(app[0].get("build-commands", []))
        if "cargo build" not in commands or "--offline" not in commands or "--locked" not in commands:
            result("build_offline", "cargo must build with --locked --offline")
        elif "npm ci --offline" not in commands:
            result("build_offline", "npm must install with npm ci --offline")
        else:
            result("build_offline", None)

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
