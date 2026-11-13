#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Sets the version of every package in the workspace.

    scripts/release/set_version.py 2026.11.0
    scripts/release/set_version.py 2026.11.0 --root path/to/checkout

Changes the `version` of each crate's [package], the version the workspace
asks for euphoria-core, and the entries of the workspace's own packages in
Cargo.lock, and nothing else, so the result does not depend on the network
or on which Cargo runs it. `cargo metadata --locked` afterwards confirms that
Cargo.lock still matches.
"""

import argparse
import pathlib
import re

REPOSITORY = pathlib.Path(__file__).resolve().parents[2]
PACKAGE_FOLDERS = ("apps", "crates")
VERSION = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")


def replace_once(text, pattern, replacement, where):
    changed, count = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if count != 1:
        raise SystemExit(f"no version found in {where}")
    return changed


def set_version(root, version):
    """Sets `version` in the checkout at `root` and returns the names of the
    packages it changed."""
    if not VERSION.match(version):
        raise SystemExit(f"{version!r} is not a version such as 2026.11.0")

    package_names = []
    manifests = [
        path for folder in PACKAGE_FOLDERS for path in (root / folder).glob("*/Cargo.toml")
    ]
    for manifest in sorted(manifests):
        text = manifest.read_text(encoding="utf-8")
        name = re.search(r'^\[package\]\nname = "([^"]+)"', text, flags=re.MULTILINE)
        if not name:
            raise SystemExit(f"{manifest} does not start its [package] with a name")
        package_names.append(name[1])
        text = replace_once(
            text,
            r'^(\[package\]\nname = "[^"]+"\nversion = )"[^"]+"',
            rf'\1"{version}"',
            manifest,
        )
        manifest.write_text(text, encoding="utf-8")

    workspace = root / "Cargo.toml"
    text = workspace.read_text(encoding="utf-8")
    text = replace_once(
        text, r'^(euphoria-core = \{ version = )"[^"]+"', rf'\1"{version}"', workspace
    )
    workspace.write_text(text, encoding="utf-8")

    lock = root / "Cargo.lock"
    text = lock.read_text(encoding="utf-8")
    for name in package_names:
        pattern = rf'^(\[\[package\]\]\nname = "{re.escape(name)}"\nversion = )"[^"]+"(\n(?!source))'
        text = replace_once(
            text, pattern, rf'\1"{version}"\2', f"Cargo.lock for {name}"
        )
    lock.write_text(text, encoding="utf-8")
    return package_names


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("version", help="such as 2026.11.0")
    parser.add_argument("--root", type=pathlib.Path, default=REPOSITORY)
    arguments = parser.parse_args()
    names = set_version(arguments.root, arguments.version)
    print(f"Set {', '.join(names)} to {arguments.version}")


if __name__ == "__main__":
    main()
