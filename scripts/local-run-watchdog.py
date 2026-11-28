"""Renders the Local Run watchdog report (see local-run-watchdog.sh).

usage: local-run-watchdog.py <base dump> <latest dump> <missing.tsv> <coverage stdout>
       <studio version> <luau upstream> <luau linked> <luau constant>
"""

import json
import os
import sys
from collections import defaultdict

LEVELS = ["None", "PluginSecurity", "LocalUserSecurity"]
RETIRED = {"Deprecated", "Hidden", "NotScriptable"}
LIMIT = 60000  # GitHub issue bodies stop at 65536 characters


def security(member):
    s = member.get("Security", "None")
    return s.get("Read", "None") if isinstance(s, dict) else s


def shape(member):
    keep = ("Security", "Tags", "ValueType", "ReturnType", "Parameters")
    return {k: member.get(k) for k in keep}


def members(path):
    dump = json.load(open(path))
    out = {}
    for c in dump["Classes"]:
        class_retired = RETIRED & set(c.get("Tags") or [])
        for m in c.get("Members", []):
            retired = class_retired or RETIRED & set(m.get("Tags") or [])
            out[(c["Name"], m["Name"])] = (m, bool(retired))
    return out, {c["Name"] for c in dump["Classes"]}


def release(version):
    return int(version.split(".")[1])


def main():
    base_path, latest_path, missing_path, totals_path = sys.argv[1:5]
    studio, upstream, linked, constant = sys.argv[5:9]
    base, base_classes = members(base_path)
    latest, latest_classes = members(latest_path)
    missing = {}
    for line in open(missing_path):
        level, cls, kind, name = line.rstrip("\n").split("\t")
        missing[(cls, name)] = (level, kind)

    added = sorted(k for k in latest if k not in base)
    removed = sorted(k for k in base if k not in latest)
    changed = sorted(
        k for k in latest if k in base and shape(latest[k][0]) != shape(base[k][0])
    )
    todo = defaultdict(list)
    for key in added + changed:
        if key in missing:
            level, kind = missing[key]
            tag = "changed" if key in base else "new"
            todo[level].append(f"- [ ] `{key[0]}.{key[1]}` ({kind}, {tag})")

    luau_behind = release(constant) < release(studio) or release(linked) < release(constant)
    lines = [
        "Daily check of Local Run against live Roblox, by `scripts/local-run-watchdog.sh` "
        "(`.github/workflows/local-run-watchdog.yml`). This issue is rewritten each run.",
        "",
        "## Luau",
        "",
        f"- Live Studio: `{studio}` (Roblox runs the Luau release of the same number)",
        f"- `rbx_lua::LUAU_VERSION`: `{constant}`, linked `vendor/luau0-src`: `{linked}`",
        f"- Latest upstream release: `{upstream}`",
        "",
        (
            "**Behind:** bump `vendor/luau0-src` and `LUAU_VERSION` (see `vendor/README.md`)."
            if luau_behind
            else "Up to date."
        ),
        "",
        "## API dump",
        "",
        f"Latest dump against the one `COVERAGE.md` was last generated from: "
        f"{len(latest_classes - base_classes)} new classes, "
        f"{len(base_classes - latest_classes)} removed, {len(added)} new members, "
        f"{len(changed)} changed, {len(removed)} removed.",
        "",
    ]
    for title, names in [
        ("New classes", sorted(latest_classes - base_classes)),
        ("Removed classes", sorted(base_classes - latest_classes)),
    ]:
        if names:
            lines += [f"{title}: " + ", ".join(f"`{n}`" for n in names), ""]
    if removed:
        lines += [
            "Removed members: " + ", ".join(f"`{c}.{m}`" for c, m in removed),
            "",
        ]
    lines += ["## To do: new or changed members Local Run doesn't have", ""]
    if not todo:
        lines += ["Nothing: every new or changed scriptable member is there.", ""]
    for level in LEVELS:
        if todo[level]:
            lines += [f"### `{level}` ({len(todo[level])})", ""] + todo[level] + [""]
    counts = defaultdict(int)
    for level, _ in missing.values():
        counts[level] += 1
    lines += [
        "## Coverage against the latest dump",
        "",
        "| Where | Security | Functions | Events | Properties | All |",
        "|---|---|---|---|---|---|",
    ]
    lines += [l for l in open(totals_path).read().splitlines() if l.startswith("| ")][:3]
    lines += [
        "",
        "Not there overall: "
        + ", ".join(f"`{level}` {counts[level]}" for level in LEVELS)
        + ".",
    ]
    body = "\n".join(lines) + "\n"
    # The workflow only keeps the tracking issue open while something is behind.
    behind = luau_behind or any(todo.values())
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as f:
            f.write(f"behind={'true' if behind else 'false'}\n")
    if len(body) > LIMIT:
        body = body[:LIMIT].rsplit("\n", 1)[0] + "\n\n(truncated)\n"
    sys.stdout.write(body)
    sys.stderr.write(
        f"todo={sum(len(v) for v in todo.values())} luau_behind={str(luau_behind).lower()}\n"
    )


main()
