#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Decides what a run of the Release workflow does: publish a new version,
or finish one whose tag was pushed but whose release never got all its files.

    scripts/release/plan_release.py

Prints `mode`, `version` and `previous_tag` lines in the key=value form of
$GITHUB_OUTPUT. The mode is `resume` when the newest release tag has no
GitHub Release, or one without every file a release carries; the workflow then
rebuilds that version from the commit the tag was made on and publishes it,
leaving the tag where it is. Otherwise the mode is `new`, for the next
calendar version. Reads the tags of this clone and the releases through `gh`.
"""

import json
import subprocess
import sys

import next_version


def expected_assets(version):
    """The files every release carries."""
    return {
        f"Euphoria-{version}.dmg",
        "Euphoria.dmg",
        f"euphoria-{version}-aarch64-apple-darwin.tar.gz",
        "SHA256SUMS",
    }


def release_tags(tags):
    """The calendar release tags, newest first."""
    found = [(parts, tag) for tag in tags if (parts := next_version.parse_tag(tag))]
    return [tag for _, tag in sorted(found, reverse=True)]


def plan(tags, published_assets, today):
    """What to do, given the tags, a function that returns the file names of
    the GitHub Release of a tag or None when it has none, and today's date.
    Returns (mode, version, previous tag)."""
    ordered = release_tags(tags)
    if ordered:
        newest = ordered[0]
        assets = published_assets(newest)
        version = newest[1:]
        if assets is None or not expected_assets(version) <= set(assets):
            previous = ordered[1] if len(ordered) > 1 else ""
            return "resume", version, previous
    version = next_version.next_version(tags, today)
    return "new", version, ordered[0] if ordered else ""


def published_assets(tag):
    result = subprocess.run(
        ["gh", "release", "view", tag, "--json", "assets"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        if "release not found" in result.stderr.lower():
            return None
        raise SystemExit(f"cannot read the release {tag}: {result.stderr.strip()}")
    return [asset["name"] for asset in json.loads(result.stdout)["assets"]]


def main():
    listed = subprocess.run(
        ["git", "tag", "--list", "v*"], capture_output=True, text=True, check=True
    )
    mode, version, previous = plan(
        listed.stdout.split(), published_assets, next_version.today()
    )
    print(f"mode={mode}")
    print(f"version={version}")
    print(f"previous_tag={previous}")


if __name__ == "__main__":
    sys.exit(main())
