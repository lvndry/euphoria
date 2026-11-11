#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Writes the notes of a release from the commit messages since the last one.

    scripts/release/release_notes.py --since v2026.11.0 --until HEAD --version 2026.11.1

Commits follow Conventional Commits, `type(scope): summary (#123)`. The notes
group them by type, link each pull request, and leave out the commits that
make no difference to someone using Euphoria (chore, ci, test, refactor,
style, build) unless they are marked breaking. Without --since, every commit
up to --until is counted.
"""

import argparse
import re
import subprocess

SECTIONS = [
    ("feat", "Features"),
    ("fix", "Fixes"),
    ("perf", "Performance"),
    ("docs", "Documentation"),
]
BREAKING_FOOTER = re.compile(
    # The footer runs to a blank line or to the next footer, such as Refs:.
    r"^BREAKING[ -]CHANGE: (?P<description>.+(?:\n(?!\n|[A-Za-z][A-Za-z-]*: ).+)*)",
    re.MULTILINE,
)
SUBJECT = re.compile(
    r"^(?P<type>[a-z]+)(?:\((?P<scope>[^)]*)\))?(?P<breaking>!)?: (?P<summary>.+)$"
)
PULL_REQUEST = re.compile(r" \(#(?P<number>[0-9]+)\)$")


def commits(since, until):
    revisions = f"{since}..{until}" if since else until
    log = subprocess.run(
        ["git", "log", "--no-merges", "--format=%H%x1f%s%x1f%b%x1e", revisions],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    for record in log.split("\x1e"):
        if record.strip():
            sha, subject, body = record.strip("\n").split("\x1f")
            yield sha, subject, body


def entry(subject, sha, repository):
    match = SUBJECT.match(subject)
    if not match:
        return None, subject
    summary = match["summary"]
    scope = f"**{match['scope']}:** " if match["scope"] else ""
    pull_request = PULL_REQUEST.search(summary)
    if pull_request:
        summary = summary[: pull_request.start()]
        link = f"[#{pull_request['number']}](https://github.com/{repository}/pull/{pull_request['number']})"
    else:
        link = f"[{sha[:7]}](https://github.com/{repository}/commit/{sha})"
    summary = summary[:1].upper() + summary[1:]
    return match, f"- {scope}{summary} ({link})"


def breaking_description(body):
    """The text of a `BREAKING CHANGE:` footer, joined into one line, or
    None."""
    match = BREAKING_FOOTER.search(body)
    if not match:
        return None
    return " ".join(line.strip() for line in match["description"].splitlines())


def notes(version, since, until, repository):
    grouped = {kind: [] for kind, _ in SECTIONS}
    breaking = []
    for sha, subject, body in commits(since, until):
        match, line = entry(subject, sha, repository)
        if match is None:
            continue
        description = breaking_description(body)
        if match["breaking"] or description:
            # The subject says which change it is; the footer says what
            # breaks and what to do about it.
            breaking.append(f"{line}\n  {description}" if description else line)
        if match["type"] in grouped:
            grouped[match["type"]].append(line)
    lines = [f"Euphoria {version}.", ""]
    lines += [
        "Download `Euphoria-"
        + version
        + ".dmg` below, open it and drag Euphoria onto Applications. "
        "The app is not notarized, so the first time you open it, right click it in Applications and choose Open. "
        "[Installation](https://github.com/"
        + repository
        + "/blob/main/docs/getting-started/installation.md) "
        "has the details. It needs a Mac with Apple Silicon.",
        "",
    ]
    if breaking:
        lines += ["## Breaking changes", "", *breaking, ""]
    for kind, title in SECTIONS:
        if grouped[kind]:
            lines += [f"## {title}", "", *grouped[kind], ""]
    if not any(grouped.values()) and not breaking:
        lines += [
            "No changes to features, fixes, performance or documentation since the last release.",
            "",
        ]
    if since:
        lines += [
            f"Every change: https://github.com/{repository}/compare/{since}...v{version}",
            "",
        ]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--version", required=True)
    parser.add_argument("--since", help="the tag of the previous release")
    parser.add_argument("--until", default="HEAD")
    parser.add_argument("--repository", default="lvndry/euphoria")
    arguments = parser.parse_args()
    print(
        notes(
            arguments.version, arguments.since, arguments.until, arguments.repository
        ),
        end="",
    )


if __name__ == "__main__":
    main()
