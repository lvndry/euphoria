# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Keeps the vendored copy of Signalsmith Stretch and Signalsmith Linear in
crates/euphoria-core/vendor in step with an upstream release, and checks it.

    python3 scripts/vendor_signalsmith.py update 1.4.0
    python3 scripts/vendor_signalsmith.py check

`update` downloads the given tag of Signalsmith Stretch and the tag of
Signalsmith Linear that its CMakeLists.txt pins, replaces the vendored files
with theirs, and records the tags, commits and SHA-256 checksums in
VENDORED.md. `check` fails unless the vendored files are exactly the ones
VENDORED.md records, no more and no fewer.
"""

import hashlib
import io
import json
import re
import shutil
import sys
import tarfile
import urllib.request
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parent.parent
VENDOR = REPOSITORY / "crates" / "euphoria-core" / "vendor"
RECORD = VENDOR / "VENDORED.md"
STRETCH = "Signalsmith-Audio/signalsmith-stretch"
LINEAR = "Signalsmith-Audio/linear"
CHECKSUM_LINE = re.compile(r"^\| `(?P<path>[^`]+)` \| `(?P<checksum>[0-9a-f]{64})` \|$")

# The files each upstream release contributes: the headers the build
# includes, their platform variants and the licence. Tests, benchmarks,
# examples and the web demo are left out.
STRETCH_FILES = ["signalsmith-stretch.h", "LICENSE.txt"]
LINEAR_PATTERNS = ["*.h", "platform/*.h", "platform/*.cpp", "LICENSE.txt"]


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": "euphoria-vendor-script"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def commit_of(repository, tag):
    reference = json.loads(fetch(f"https://api.github.com/repos/{repository}/git/refs/tags/{tag}"))
    target = reference["object"]
    if target["type"] == "tag":
        target = json.loads(fetch(target["url"]))["object"]
    return target["sha"]


def extract(repository, tag, destination):
    archive = fetch(f"https://github.com/{repository}/archive/refs/tags/{tag}.tar.gz")
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
        for member in tar.getmembers():
            if not member.isfile():
                continue
            # Every archive member sits in one top folder, which is dropped.
            relative = Path(*Path(member.name).parts[1:])
            if relative.is_absolute() or ".." in relative.parts:
                raise SystemExit(f"refusing the archive path {member.name}")
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(tar.extractfile(member).read())


def checksum(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def vendored_files():
    return sorted(
        path.relative_to(VENDOR).as_posix()
        for path in VENDOR.rglob("*")
        if path.is_file() and path != RECORD
    )


def update(stretch_tag):
    work = Path(__import__("tempfile").mkdtemp())
    try:
        stretch_source = work / "stretch"
        extract(STRETCH, stretch_tag, stretch_source)
        cmake = (stretch_source / "CMakeLists.txt").read_text()
        pinned = re.search(r"GIT_REPOSITORY https://github.com/Signalsmith-Audio/linear.git\s+GIT_TAG (\S+)", cmake)
        if pinned is None:
            raise SystemExit("cannot find the Signalsmith Linear tag in CMakeLists.txt")
        linear_tag = pinned.group(1)
        linear_source = work / "linear"
        extract(LINEAR, linear_tag, linear_source)

        if VENDOR.exists():
            shutil.rmtree(VENDOR)
        stretch_target = VENDOR / "signalsmith-stretch"
        linear_target = VENDOR / "signalsmith-linear"
        for name in STRETCH_FILES:
            (stretch_target / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(stretch_source / name, stretch_target / name)
        for pattern in LINEAR_PATTERNS:
            for source in sorted(linear_source.glob(pattern)):
                target = linear_target / source.relative_to(linear_source)
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)

        lines = [
            "# Vendored Signalsmith libraries",
            "",
            "Written by `scripts/vendor_signalsmith.py`; do not edit by hand. Both are MIT",
            "licensed; their licences are next to their sources and ship with the app",
            "and the command line tool.",
            "",
            "| Library | Tag | Commit |",
            "| --- | --- | --- |",
            f"| [Signalsmith Stretch](https://github.com/{STRETCH}) | {stretch_tag} | `{commit_of(STRETCH, stretch_tag)}` |",
            f"| [Signalsmith Linear](https://github.com/{LINEAR}) | {linear_tag} | `{commit_of(LINEAR, linear_tag)}` |",
            "",
            "| File | SHA-256 |",
            "| --- | --- |",
        ]
        lines += [f"| `{path}` | `{checksum(VENDOR / path)}` |" for path in vendored_files()]
        RECORD.write_text("\n".join(lines) + "\n")
        print(f"vendored Signalsmith Stretch {stretch_tag} and Signalsmith Linear {linear_tag}")
    finally:
        shutil.rmtree(work, ignore_errors=True)


def recorded_checksums():
    recorded = {}
    for line in RECORD.read_text().splitlines():
        match = CHECKSUM_LINE.match(line)
        if match:
            recorded[match.group("path")] = match.group("checksum")
    return recorded


def check():
    recorded = recorded_checksums()
    if not recorded:
        raise SystemExit(f"{RECORD} lists no files")
    present = set(vendored_files())
    problems = []
    for path in sorted(set(recorded) - present):
        problems.append(f"missing: {path}")
    for path in sorted(present - set(recorded)):
        problems.append(f"not recorded: {path}")
    for path in sorted(present & set(recorded)):
        if checksum(VENDOR / path) != recorded[path]:
            problems.append(f"changed: {path}")
    if problems:
        raise SystemExit(
            "the vendored Signalsmith files differ from VENDORED.md:\n  "
            + "\n  ".join(problems)
            + "\nRun scripts/vendor_signalsmith.py update <tag> to replace them."
        )
    print(f"{len(recorded)} vendored files match VENDORED.md")


def main(arguments):
    if arguments[:1] == ["update"] and len(arguments) == 2:
        update(arguments[1])
    elif arguments == ["check"]:
        check()
    else:
        raise SystemExit("usage: vendor_signalsmith.py update <stretch tag> | check")


if __name__ == "__main__":
    main(sys.argv[1:])
