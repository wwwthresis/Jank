#!/usr/bin/env python3
"""Generates app/assets/THIRD_PARTY_LICENSES.txt for the Jank! "Licenses" window.

Source of truth: `cargo metadata` (crate name, version, SPDX expression, repository) plus
the licence files shipped inside each crate in the local cargo registry.
Run from the repository root:  python3 tools/gen_licenses.py
"""
import json, os, subprocess, sys, glob, hashlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
APP = os.path.join(ROOT, "app")
OUT = os.path.join(APP, "assets", "THIRD_PARTY_LICENSES.txt")
OWN = {"jank", "psxcore"}
# Fonts bundled by egui (default_fonts) carry their own licences, see the egui crates' metadata.
env = {k: v for k, v in os.environ.items() if k.lower() != "no_proxy"}
meta = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1", "--manifest-path", os.path.join(APP, "Cargo.toml")],
    env=env, stderr=subprocess.DEVNULL))

# Only crates that are actually part of the normal build graph of the app on Linux.
resolve = {n["id"]: n for n in meta["resolve"]["nodes"]}
by_id = {p["id"]: p for p in meta["packages"]}
root = next(p["id"] for p in meta["packages"] if p["name"] == "jank")
seen, stack = set(), [root]
while stack:
    i = stack.pop()
    if i in seen:
        continue
    seen.add(i)
    for d in resolve[i]["deps"]:
        kinds = {k["kind"] for k in d["dep_kinds"]}
        if kinds <= {"dev"}:
            continue
        stack.append(d["pkg"])
pkgs = sorted((by_id[i] for i in seen if by_id[i]["name"] not in OWN), key=lambda p: (p["name"], p["version"]))

def chosen(expr):
    """Human-readable choice for an 'A OR B' expression (we always pick the most permissive option)."""
    e = (expr or "UNKNOWN").replace("/", " OR ")
    if " OR " in e and "AND" not in e:
        opts = [o.strip().strip("()") for o in e.split(" OR ")]
        for pref in ("MIT", "Apache-2.0", "Zlib", "Unlicense", "BSD-3-Clause", "ISC", "0BSD"):
            if pref in opts:
                return pref
    return expr or "UNKNOWN"

def license_files(pkg):
    d = os.path.dirname(pkg["manifest_path"])
    names = []
    for pat in ("LICENSE*", "LICENCE*", "COPYING*", "UNLICENSE*", "license*"):
        names += glob.glob(os.path.join(d, pat))
    # Bundled font packages keep their licences next to the fonts.
    names += glob.glob(os.path.join(d, "fonts", "*.txt"))
    return sorted(set(n for n in names if os.path.isfile(n)))

MIT_TEMPLATE = """MIT License

Copyright (c) {holders}

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

def apache_text():
    for p in pkgs:
        if p["name"] == "serde":
            f = os.path.join(os.path.dirname(p["manifest_path"]), "LICENSE-APACHE")
            if os.path.isfile(f):
                return open(f, encoding="utf8").read().strip()
    return None

lines, texts = [], {}
lines.append("THIRD-PARTY LICENSES FOR JANK!")
lines.append("=" * 72)
for l in [
    "Jank! consists of its own code (the crates `jank` and `psxcore`)",
    "and of the open-source crates listed below. The Jank! crates use",
    "only the Rust standard library for modeling, rasterizing, image",
    "and video codecs and the project format.",
    "",
    "Each crate is used under the licence shown. Where a crate offers",
    "a choice (\"A OR B\"), the permissive option marked 'used under'",
    "applies. No crate is used under GPL or AGPL terms.",
]:
    lines.append(l)
lines.append("")
lines.append(f"Crates: {len(pkgs)}")
lines.append("")
lines.append("PART 1 - CRATE LIST")
lines.append("-" * 72)
missing = []
for p in pkgs:
    lines.append(f"{p['name']} {p['version']}")
    lines.append(f"    licence: {p['license'] or 'UNKNOWN'}   (used under: {chosen(p['license'])})")
    if p.get("repository"):
        lines.append(f"    source:  {p['repository']}")
    fs = license_files(p)
    if not fs:
        missing.append(p["name"])
    for f in fs:
        data = open(f, "rb").read().decode("utf8", "replace").strip()
        h = hashlib.sha1(data.encode()).hexdigest()
        texts.setdefault(h, {"text": data, "crates": []})["crates"].append(f"{p['name']} {p['version']}")
# Crates published without a licence file: fall back to the standard text of the SPDX
# licence they declare (MIT preferred), naming the authors from their manifest.
fallback = []
apache = apache_text()
for p in pkgs:
    if p["name"] in missing:
        pick = chosen(p["license"])
        holders = ", ".join(p.get("authors") or []) or f"the {p['name']} authors"
        if pick == "MIT":
            body = MIT_TEMPLATE.format(holders=holders)
        elif pick == "Apache-2.0" and apache:
            body = apache
        else:
            continue
        h = hashlib.sha1((pick + holders + body).encode()).hexdigest()
        texts.setdefault(h, {"text": body, "crates": []})["crates"].append(f"{p['name']} {p['version']}")
        fallback.append(p["name"])
missing = [m for m in missing if m not in fallback]
lines.append("")
lines.append("PART 2 - LICENCE TEXTS")
lines.append("-" * 72)
lines.append("Identical texts are printed once, followed by the crates that ship them. For crates that")
lines.append("publish no licence file, the standard text of their declared licence is printed with the")
lines.append("authors named in the crate manifest.")
lines.append("")
for h, t in sorted(texts.items(), key=lambda kv: kv[1]["crates"][0]):
    cr = t["crates"]
    shown = ", ".join(cr[:12]) + (f" and {len(cr) - 12} more" if len(cr) > 12 else "")
    lines.append("=" * 72)
    lines.append(f"Applies to: {shown}")
    lines.append("=" * 72)
    lines.append(t["text"])
    lines.append("")
if missing:
    lines.append("=" * 72)
    lines.append("Crates whose package carries no licence file (the SPDX expression above applies):")
    lines.append(", ".join(missing))
# Assets shipped with Jank! itself (not crates).
lines.append("")
lines.append("=" * 72)
lines.append("PART 3 - BUNDLED ASSETS")
lines.append("=" * 72)
lines.append("Inter (Regular, SemiBold) - the interface font. Copyright The Inter Project Authors")
lines.append("(https://github.com/rsms/inter). Licensed under the SIL Open Font License 1.1:")
lines.append("")
ofl = os.path.join(APP, "assets", "fonts", "Inter-LICENSE-OFL.txt")
lines.append(open(ofl, encoding="utf8").read().strip())
lines.append("")
lines.append("Butterfly model and texture (core/assets/butterfly.obj, butterfly_texture.png; demo/butterfly.jank) - supplied by the project owner")
lines.append("for use in Jank!. They are built into the program as the Butterfly primitive and carried inside the demo project.")
lines.append("The butterfly icon in the Add row, the demo's meadow, bark, needle and sky pictures are original work made for Jank! and generated")
lines.append("in code (core/examples/make_butterfly_demo.rs); no third-party pictures are included.")
os.makedirs(os.path.dirname(OUT), exist_ok=True)
open(OUT, "w", encoding="utf8").write("\n".join(lines) + "\n")
print(f"wrote {OUT}: {len(pkgs)} crates, {len(texts)} distinct texts, {len(missing)} without licence file")
