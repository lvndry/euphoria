#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Prints the version of the next release.

    scripts/release/next_version.py              # from the tags of this clone
    scripts/release/next_version.py --tags v2026.11.0 v2026.11.1 --date 2026-11-30

Euphoria uses calendar versions, YEAR.MONTH.N: N counts the releases made in
that month, from 0, and nothing has a leading zero. The first release of
November 2026 is 2026.11.0, the second 2026.11.1, the first of December
2026.12.0. The month is the month in UTC when the release is made.

The version is valid semver, so Cargo accepts it, and it is three dot
separated numbers, so Info.plist accepts it as CFBundleShortVersionString and
CFBundleVersion.
"""

import argparse
import datetime
import re
import subprocess
import sys

TAG = re.compile(
    r"^v(?P<year>[1-9][0-9]{3})\.(?P<month>[1-9]|1[0-2])\.(?P<number>0|[1-9][0-9]*)$"
)


def release_tags(tags):
    """The (year, month, number) of every tag that names a calendar release."""
    found = []
    for tag in tags:
        match = TAG.match(tag)
        if match:
            found.append(
                (int(match["year"]), int(match["month"]), int(match["number"]))
            )
    return found


def next_version(tags, today):
    released = release_tags(tags)
    if released and max(released)[:2] > (today.year, today.month):
        newest = ".".join(str(part) for part in max(released))
        raise SystemExit(
            f"the newest release, {newest}, is from a later month than {today:%Y-%m}"
        )
    numbers = [
        number
        for year, month, number in released
        if (year, month) == (today.year, today.month)
    ]
    number = max(numbers) + 1 if numbers else 0
    return f"{today.year}.{today.month}.{number}"


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument(
        "--tags", nargs="*", help="tags to count instead of those of this clone"
    )
    parser.add_argument(
        "--date",
        type=datetime.date.fromisoformat,
        help="the day of the release, in UTC",
    )
    arguments = parser.parse_args()
    if arguments.tags is None:
        listed = subprocess.run(
            ["git", "tag", "--list", "v*"], capture_output=True, text=True, check=True
        )
        tags = listed.stdout.split()
    else:
        tags = arguments.tags
    today = arguments.date or datetime.datetime.now(datetime.UTC).date()
    print(next_version(tags, today))


if __name__ == "__main__":
    sys.exit(main())
