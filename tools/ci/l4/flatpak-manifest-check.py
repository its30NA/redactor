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
  toolchains     the Rust archive is a release of the channel rust-toolchain.toml names and, where
                 that channel's rustc runs here, the very release it reports (rustup takes the
                 newest point release, so a new 1.99.x shows up here first); the Node archive has
                 the major version the workflows' setup-node steps ask for.
"""

import glob
import json
import os
import re
import shutil
import subprocess
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


def find_up(start, name):
    """The first <dir>/<name> from start upwards, or None."""
    d = start
    while True:
        if os.path.exists(os.path.join(d, name)):
            return d
        parent = os.path.dirname(d)
        if parent == d:
            return None
        d = parent


def archive_version(all_modules, module, pattern):
    """The version in the URL of the named module's archive, or None."""
    for mod, _ in all_modules:
        if mod.get("name") != module:
            continue
        for src in mod.get("sources", []):
            if isinstance(src, dict) and src.get("type") == "archive":
                m = re.search(pattern, src.get("url", ""))
                if m:
                    return m.group(1)
    return None


def local_rustc(repo):
    """`rustc --version` run in the repository (so rustup picks rust-toolchain.toml), or None
    when there is no rustc or it cannot answer. rustup must not install anything for this."""
    if not shutil.which("rustc"):
        return None
    env = dict(os.environ, RUSTUP_AUTO_INSTALL="0")
    try:
        out = subprocess.run(
            ["rustc", "--version"], cwd=repo, env=env, capture_output=True, text=True, timeout=60
        )
    except (OSError, subprocess.SubprocessError):
        return None
    m = re.match(r"rustc (\d+\.\d+\.\d+)", out.stdout) if out.returncode == 0 else None
    return m.group(1) if m else None


def toolchain_problem(all_modules, base):
    repo = find_up(base, "rust-toolchain.toml")
    if repo is None:
        return "no rust-toolchain.toml above the manifest"
    with open(os.path.join(repo, "rust-toolchain.toml"), encoding="utf-8") as f:
        m = re.search(r'^channel\s*=\s*"([0-9.]+)"', f.read(), re.M)
    if not m:
        return "rust-toolchain.toml names no numbered channel"
    channel = m.group(1)
    rust = archive_version(all_modules, "rust", r"/rust-(\d+\.\d+\.\d+)-")
    if rust is None:
        return "no rust module with a rust-<version>- archive"
    if rust != channel and not rust.startswith(channel + "."):
        return f"the manifest's Rust {rust} is not of the channel {channel} in rust-toolchain.toml"
    here = local_rustc(repo)
    if here is None or not (here == channel or here.startswith(channel + ".")):
        print(f"toolchains: Rust {rust} not compared with a local rustc {channel}", file=sys.stderr)
    elif here != rust:
        return (
            f"the manifest pins Rust {rust}, the {channel} channel here is {here}: pin the same "
            f"release (url and sha256) or run `rustup update {channel}`"
        )

    node = archive_version(all_modules, "node", r"/node-v(\d+)\.\d+\.\d+-")
    if node is None:
        return "no node module with a node-v<version>- archive"
    wanted = set()
    for wf in glob.glob(os.path.join(repo, ".github", "workflows", "*.yml")):
        with open(wf, encoding="utf-8") as f:
            wanted.update(re.findall(r'node-version:\s*"?(\d+)', f.read()))
    if not wanted:
        return "no node-version in .github/workflows"
    if wanted != {node}:
        return f"the manifest's Node is {node}.x, the workflows ask for {' '.join(sorted(wanted))}"
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

    result("toolchains", toolchain_problem(all_modules, base))

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
