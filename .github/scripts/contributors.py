#!/usr/bin/env python3
"""Fills contributors.html's wall from GitHub and contributors.json.

GitHub gives everyone with a commit on rbx-native, contributors.json adds
roles and the people who never commit (testers, design, ideas). Runs in
pages.yml before the upload, so the published page is plain HTML and nobody's
browser calls the GitHub API. Without GITHUB_TOKEN it still works, just under
the anonymous rate limit.
"""
import html
import json
import os
import sys
import urllib.request

REPO = "chteau/rbx-native"
MARK = "<!-- wall -->"
ROLE_NAMES = {
    "code": "Code", "testing": "Testing", "design": "Design", "ideas": "Ideas",
    "docs": "Docs", "translation": "Translation", "framework": "Framework",
}


def github(path):
    request = urllib.request.Request(f"https://api.github.com/{path}")
    request.add_header("Accept", "application/vnd.github+json")
    token = os.environ.get("GITHUB_TOKEN")
    if token:
        request.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(request, timeout=30) as reply:
        return json.load(reply)


def code_contributors():
    people, page = [], 1
    while True:
        batch = github(f"repos/{REPO}/contributors?per_page=100&page={page}")
        people += [p for p in batch if p.get("type") == "User"]
        if len(batch) < 100:
            return people
        page += 1


def card(person):
    name = html.escape(person["name"])
    link = person.get("url")
    if person.get("avatar"):
        face = f'<img src="{html.escape(person["avatar"])}&s=144" alt="" width="72" height="72" loading="lazy">'
    else:
        face = f'<span class="initial" aria-hidden="true">{html.escape(person["name"][:1].upper())}</span>'
    roles = "".join(f"<li>{ROLE_NAMES.get(r, html.escape(r.title()))}</li>" for r in person["roles"])
    note = f'<p>{html.escape(person["note"])}</p>' if person.get("note") else ""
    head = f'<a href="{html.escape(link)}">{face}<strong>{name}</strong></a>' if link else f"{face}<strong>{name}</strong>"
    return f'<li class="person">{head}{note}<ul class="roles">{roles}</ul></li>'


def main(page_path, list_path):
    listed = json.load(open(list_path))
    skip = {s.lower() for s in listed.get("skip", [])}
    people, by_login = [], {}
    try:
        commits = code_contributors()
    except Exception as error:  # the wall still has the hand-kept people
        print(f"contributors: GitHub didn't answer ({error}), hand-kept list only", file=sys.stderr)
        commits = []
    for c in commits:
        if c["login"].lower() in skip:
            continue
        person = {"name": c["login"], "url": c["html_url"], "avatar": c["avatar_url"],
                  "roles": ["code"], "weight": c["contributions"]}
        by_login[c["login"].lower()] = person
        people.append(person)
    for entry in listed["people"]:
        login = (entry.get("github") or "").lower()
        person = by_login.get(login)
        if person is None:
            person = {"name": entry.get("name") or entry.get("github"), "weight": 0, "roles": []}
            if login:
                person["url"] = f"https://github.com/{entry['github']}"
                person["avatar"] = f"https://avatars.githubusercontent.com/{entry['github']}?v=4"
            people.append(person)
        if entry.get("name"):
            person["name"] = entry["name"]
        if entry.get("url"):
            person["url"] = entry["url"]
        if entry.get("note"):
            person["note"] = entry["note"]
        person["roles"] = list(dict.fromkeys(entry.get("roles") or person["roles"]))
    people.sort(key=lambda p: (-p["weight"], p["name"].lower()))
    page = open(page_path).read()
    if MARK not in page:
        sys.exit(f"contributors: no {MARK} in {page_path}")
    wall = '<ul class="wall">' + "".join(card(p) for p in people) + "</ul>"
    open(page_path, "w").write(page.replace(MARK, wall))
    print(f"contributors: {len(people)} on the wall")


if __name__ == "__main__":
    main(*(sys.argv[1:] or ["contributors.html", "contributors.json"]))
