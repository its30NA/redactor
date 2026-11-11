#!/usr/bin/env bash
# Regenerate THIRD_PARTY_NOTICES.md: the licenses of everything compiled or bundled into a
# release download.
#
#   Rust   normal (non-dev, non-build) dependencies of `scrub` on the four CLI release targets and
#          of `redactor-app` on Windows; texts come from the crate sources in the cargo registry.
#   npm    production dependencies of app/ui (from package-lock.json and node_modules).
#   fonts  Inter and JetBrains Mono, with their OFL 1.1 text.
#
# Needs: cargo fetch --locked (registry sources) and npm ci in app/ui (node_modules). No network,
# no extra tools. The output is deterministic: CI re-runs this and fails on a diff.
#
# Usage: tools/gen-notices.sh
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [ ! -d app/ui/node_modules ]; then
  echo "app/ui/node_modules is missing: run 'npm ci' in app/ui first" >&2
  exit 1
fi

python3 -I - "$root" <<'PY'
import hashlib, json, os, re, subprocess, sys

root = sys.argv[1]
OUT = os.path.join(root, "THIRD_PARTY_NOTICES.md")

# Root crate -> release targets it ships on (see .github/workflows/release.yml).
RUST_SHIPPED = {
    "scrub": [
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-musl",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ],
    "redactor-app": ["x86_64-pc-windows-msvc"],
}
FONT_PACKAGES = [
    ("Inter", "@fontsource-variable/inter"),
    ("JetBrains Mono", "@fontsource-variable/jetbrains-mono"),
]

MIT_TEXT = """MIT License

Copyright (c) {holder}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE."""

BSD3_TEXT = """BSD 3-Clause License

Copyright (c) {holder}

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from
   this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE."""

LICENSE_FILE = re.compile(r"(?i)^(licen[cs]e|copying|notice|unlicense)")


def norm(text):
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    lines = [l.rstrip() for l in text.split("\n")]
    while lines and not lines[-1]:
        lines.pop()
    while lines and not lines[0]:
        lines.pop(0)
    return "\n".join(lines)


def read_license_files(d):
    out = []
    for name in sorted(os.listdir(d)):
        p = os.path.join(d, name)
        if os.path.isfile(p) and LICENSE_FILE.match(name):
            with open(p, encoding="utf-8", errors="replace") as f:
                t = norm(f.read())
            if t:
                out.append(t)
    return out


def fence(text):
    n = 3
    while ("~" * n) in text:
        n += 1
    f = "~" * n
    return f"{f}text\n{text}\n{f}\n"


def clean_repo(url):
    if not url:
        return ""
    url = re.sub(r"^git\+", "", url)
    url = re.sub(r"\.git$", "", url)
    url = re.sub(r"^git://", "https://", url)
    return url


def holder_from_authors(authors, fallback):
    names = []
    for a in authors or []:
        a = re.sub(r"\s*<[^>]*>", "", a).strip()
        if a:
            names.append(a)
    return ", ".join(names) if names else fallback


# ---------------------------------------------------------------- Rust
rust = {}
for crate, targets in RUST_SHIPPED.items():
    for target in targets:
        meta = json.loads(
            subprocess.check_output(
                ["cargo", "metadata", "--format-version", "1", "--locked", "--offline",
                 "--filter-platform", target],
                cwd=root,
            )
        )
        by_id = {p["id"]: p for p in meta["packages"]}
        nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
        start = [p["id"] for p in meta["packages"] if p["name"] == crate and not p["source"]][0]
        seen, stack = set(), [start]
        while stack:
            i = stack.pop()
            if i in seen:
                continue
            seen.add(i)
            for d in nodes[i]["deps"]:
                # Normal edges only: dev- and build-only dependencies are not shipped.
                if any(k["kind"] is None for k in d["dep_kinds"]):
                    stack.append(d["pkg"])
        for i in seen:
            p = by_id[i]
            if p["source"]:  # workspace crates are the project itself (MIT, LICENSE)
                rust[(p["name"], p["version"])] = p

crates = []
for (name, version), p in sorted(rust.items()):
    d = os.path.dirname(p["manifest_path"])
    crates.append({
        "name": name, "version": version,
        "license": p.get("license") or "see license file",
        "repo": clean_repo(p.get("repository") or ""),
        "authors": p.get("authors"),
        "texts": read_license_files(d),
    })

# A crate with no license file in its package gets the standard text for its SPDX license
# (MIT, BSD-3-Clause) or, for MPL-2.0, the text shipped by another crate under the same license.
mpl_text = None
for c in crates:
    if c["license"] == "MPL-2.0":
        for t in c["texts"]:
            if "Mozilla Public License Version 2.0" in t:
                mpl_text = t
                break
    if mpl_text:
        break
fallback_used = []
for c in crates:
    if c["texts"]:
        continue
    holder = holder_from_authors(c["authors"], f"The {c['name']} authors")
    expr = c["license"]
    if expr == "MIT":
        c["texts"] = [norm(MIT_TEXT.format(holder=holder))]
    elif expr == "BSD-3-Clause":
        c["texts"] = [norm(BSD3_TEXT.format(holder=holder))]
    elif expr == "MPL-2.0" and mpl_text:
        c["texts"] = [mpl_text]
    else:
        sys.exit(f"no license text for {c['name']} {c['version']} ({expr})")
    fallback_used.append(c)

groups = {}
for c in crates:
    for t in c["texts"]:
        groups.setdefault(t, []).append(c)

# ---------------------------------------------------------------- npm
ui = os.path.join(root, "app", "ui")
with open(os.path.join(ui, "package-lock.json"), encoding="utf-8") as f:
    lock = json.load(f)
font_dirs = {pkg for _, pkg in FONT_PACKAGES}
npm = []
for key, entry in sorted(lock["packages"].items()):
    if not key or entry.get("dev"):
        continue
    assert key.startswith("node_modules/"), key
    pkg_dir = os.path.join(ui, key)
    name = key.split("node_modules/")[-1]
    if name in font_dirs:
        continue
    with open(os.path.join(pkg_dir, "package.json"), encoding="utf-8") as f:
        pj = json.load(f)
    repo = pj.get("repository")
    if isinstance(repo, dict):
        repo = repo.get("url")
    lic = pj.get("license") or entry.get("license") or "see license file"
    if isinstance(lic, dict):
        lic = lic.get("type", "see license file")
    texts = read_license_files(pkg_dir)
    fallback = False
    if not texts:
        author = pj.get("author")
        if isinstance(author, dict):
            author = author.get("name")
        author = re.sub(r"\s*[<(][^>)]*[>)]", "", author or "").strip()
        if lic != "MIT" or not author:
            sys.exit(f"no license file for npm package {name}")
        texts = [norm(MIT_TEXT.format(holder=author))]
        fallback = True
    npm.append({"name": name, "version": pj["version"], "license": lic,
                "repo": clean_repo(repo or ""), "texts": texts, "fallback": fallback})
npm_groups = {}
for c in npm:
    for t in c["texts"]:
        npm_groups.setdefault(t, []).append(c)

# ---------------------------------------------------------------- output
def entry_line(c):
    s = f"- `{c['name']}` {c['version']} ({c['license']})"
    if c["repo"]:
        s += f" - {c['repo']}"
    return s


def render_groups(gs, prefix):
    ordered = sorted(gs.items(), key=lambda kv: (sorted((c["name"], c["version"]) for c in kv[1])[0], kv[0]))
    out = []
    for n, (text, members) in enumerate(ordered, 1):
        members = sorted(members, key=lambda c: (c["name"], c["version"]))
        out.append(f"### {prefix}{n}\n")
        out.append("Applies to:\n")
        out.extend(entry_line(c) for c in members)
        out.append("")
        out.append(fence(text))
    return "\n".join(out)


doc = []
doc.append("# Third-party notices\n")
doc.append(
    "Redactor is licensed under the MIT License (see `LICENSE`). Every download also contains the\n"
    "third-party components listed below, each under its own license. This file is generated by\n"
    "`tools/gen-notices.sh`; do not edit it by hand.\n"
)
doc.append("- Rust: " + str(len(crates)) + " crates compiled into `scrub` (Windows, Linux, macOS) and the Windows app.")
doc.append("- npm: " + str(len(npm)) + " packages bundled into the app's UI.")
doc.append("- Fonts: " + str(len(FONT_PACKAGES)) + " (Inter, JetBrains Mono), SIL Open Font License 1.1.\n")

doc.append("## Rust crates\n")
doc.append(
    "Each group below shares one license text, shown once after the list of crates it applies to.\n"
    "A crate with two license files (for example MIT and Apache-2.0) appears in two groups.\n"
)
if fallback_used:
    names = ", ".join(f"`{c['name']}`" for c in fallback_used)
    doc.append(
        "These crates ship no license file in their package: " + names + ". For them the standard\n"
        "text of the license named in their `Cargo.toml` is used (the copyright line comes from the\n"
        "crate's `authors`, or names its authors generically when it has none).\n"
    )
doc.append(render_groups(groups, "R"))

doc.append("## npm packages\n")
doc.append("The production dependencies of the UI (`app/ui`), each license text shown once per group.\n")
npm_fb = [c for c in npm if c["fallback"]]
if npm_fb:
    names = ", ".join(f"`{c['name']}`" for c in npm_fb)
    doc.append(
        "These packages ship no license file: " + names + ". For them the standard MIT text is used,\n"
        "with the copyright line naming the author from the package's `package.json`.\n"
    )
doc.append(render_groups(npm_groups, "N"))

doc.append("## Fonts\n")
doc.append(
    "The UI bundles Inter and JetBrains Mono through the `@fontsource-variable` npm packages. Both\n"
    "are licensed under the SIL Open Font License, Version 1.1; the copyright line and the\n"
    "license text of each follow, as shipped with the font package.\n"
)
for label, pkg in FONT_PACKAGES:
    p = os.path.join(ui, "node_modules", pkg)
    with open(os.path.join(p, "package.json"), encoding="utf-8") as f:
        pj = json.load(f)
    texts = read_license_files(p)
    if len(texts) != 1 or "SIL OPEN FONT LICENSE Version 1.1" not in texts[0]:
        sys.exit(f"unexpected license file for {pkg}")
    doc.append(f"### {label}\n")
    doc.append(f"- Package: `{pkg}` {pj['version']} ({pj['license']})")
    doc.append("- Upstream: " + clean_repo(pj["homepage"] if "homepage" in pj else "") + "\n")
    doc.append(fence(texts[0]))

text = "\n".join(doc).rstrip("\n") + "\n"
with open(OUT, "w", encoding="utf-8", newline="\n") as f:
    f.write(text)
print(f"wrote THIRD_PARTY_NOTICES.md: {len(crates)} crates, {len(npm)} npm packages, {len(FONT_PACKAGES)} fonts")
PY
