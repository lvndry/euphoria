#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Decides what a run of the Release workflow does: publish a new version,
or finish one whose tag was pushed but whose release never got all its files.

    scripts/release/plan_release.py

Prints `mode`, `version` and `previous_tag` lines in the key=value form of
$GITHUB_OUTPUT. The mode is `resume` when the newest release tag has no
GitHub Release, or one the workflow did not finish: without every file, without
its title, without the end of its notes, or not marked as the latest release.
The workflow then
rebuilds that version from the commit the tag was made on and publishes it,
leaving the tag where it is. Otherwise the mode is `new`, for the next
calendar version. Reads the tags of this clone and the releases through `gh`.
"""

import json
import subprocess
import sys

import next_version
from release_notes import COMPLETE_MARKER


# The script that packages the Linux files. A tag whose files hold it is
# released with them; an older tag is released with the macOS files only.
LINUX_PACKAGING = "scripts/bundle-linux.sh"


def expected_assets(version, linux=True):
    """The files every release carries, with the Linux ones when `linux`."""
    assets = {
        f"Euphoria-{version}.dmg",
        "Euphoria.dmg",
        f"euphoria-{version}-aarch64-apple-darwin.tar.gz",
        "SHA256SUMS",
    }
    if linux:
        assets |= {
            f"Euphoria-{version}-x86_64.AppImage",
            "Euphoria-x86_64.AppImage",
            f"euphoria-{version}-x86_64-unknown-linux-gnu.tar.gz",
        }
    return assets


def release_tags(tags):
    """The calendar release tags, newest first."""
    found = [(parts, tag) for tag in tags if (parts := next_version.parse_tag(tag))]
    return [tag for _, tag in sorted(found, reverse=True)]


def is_complete(release, version, linux=True):
    """Whether a GitHub Release, as `release_state` describes it, is what the
    workflow leaves when it finishes: every file, its title, its notes, which
    end with the marker the workflow writes last, and marked as the latest
    release."""
    return (
        release is not None
        and expected_assets(version, linux) <= set(release["assets"])
        and release["title"] == f"Euphoria {version}"
        and release["notes"].rstrip().endswith(COMPLETE_MARKER)
        and release["is_latest"]
    )


def plan(tags, release_state, today, packages_linux=lambda tag: True):
    """What to do, given the tags, a function that describes the GitHub
    Release of a tag or returns None when it has none, today's date, and a
    function that tells whether the files of a tag package the Linux
    release. Returns (mode, version, previous tag)."""
    ordered = release_tags(tags)
    if ordered:
        newest = ordered[0]
        version = newest[1:]
        if not is_complete(release_state(newest), version, packages_linux(newest)):
            previous = ordered[1] if len(ordered) > 1 else ""
            return "resume", version, previous
    version = next_version.next_version(tags, today)
    return "new", version, ordered[0] if ordered else ""


def release_state(tag):
    """The files, title and notes of the GitHub Release of `tag`, and
    whether it is the latest release, or None when the tag has no release."""
    result = subprocess.run(
        ["gh", "release", "view", tag, "--json", "assets,name,body"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        if "release not found" in result.stderr.lower():
            return None
        raise SystemExit(f"cannot read the release {tag}: {result.stderr.strip()}")
    release = json.loads(result.stdout)
    latest = subprocess.run(
        ["gh", "api", "repos/{owner}/{repo}/releases/latest", "--jq", ".tag_name"],
        check=False,
        capture_output=True,
        text=True,
    )
    return {
        "assets": [asset["name"] for asset in release["assets"]],
        "title": release["name"],
        "notes": release["body"] or "",
        "is_latest": latest.returncode == 0 and latest.stdout.strip() == tag,
    }


def packages_linux(tag):
    """Whether the files of `tag` hold the Linux packaging script."""
    result = subprocess.run(
        ["git", "cat-file", "-e", f"{tag}:{LINUX_PACKAGING}"],
        check=False,
        capture_output=True,
    )
    return result.returncode == 0


def main():
    listed = subprocess.run(
        ["git", "tag", "--list", "v*"], capture_output=True, text=True, check=True
    )
    mode, version, previous = plan(
        listed.stdout.split(), release_state, next_version.today(), packages_linux
    )
    print(f"mode={mode}")
    print(f"version={version}")
    print(f"previous_tag={previous}")


if __name__ == "__main__":
    sys.exit(main())
