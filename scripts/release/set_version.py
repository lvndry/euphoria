#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Sets the version of every package in the workspace.

    scripts/release/set_version.py 2026.11.0

Changes the `version` of each crate's [package], the version the workspace
asks for euphoria-core, and the entries of the workspace's own packages in
Cargo.lock, and nothing else, so the result does not depend on the network
or on which Cargo runs it. `cargo metadata --locked` afterwards confirms that
Cargo.lock still matches.
"""

import pathlib
import re
import sys

REPOSITORY = pathlib.Path(__file__).resolve().parents[2]
VERSION = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")


def replace_once(text, pattern, replacement, where):
    changed, count = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if count != 1:
        raise SystemExit(f"no version found in {where}")
    return changed


def main():
    if len(sys.argv) != 2 or not VERSION.match(sys.argv[1]):
        raise SystemExit("usage: set_version.py YEAR.MONTH.N, such as 2026.11.0")
    version = sys.argv[1]

    package_names = []
    for manifest in sorted((REPOSITORY / "crates").glob("*/Cargo.toml")):
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

    workspace = REPOSITORY / "Cargo.toml"
    text = workspace.read_text(encoding="utf-8")
    text = replace_once(
        text, r'^(euphoria-core = \{ version = )"[^"]+"', rf'\1"{version}"', workspace
    )
    workspace.write_text(text, encoding="utf-8")

    lock = REPOSITORY / "Cargo.lock"
    text = lock.read_text(encoding="utf-8")
    for name in package_names:
        pattern = rf'^(\[\[package\]\]\nname = "{re.escape(name)}"\nversion = )"[^"]+"(\n(?!source))'
        text = replace_once(
            text, pattern, rf'\1"{version}"\2', f"Cargo.lock for {name}"
        )
    lock.write_text(text, encoding="utf-8")
    print(f"Set {', '.join(package_names)} to {version}")


if __name__ == "__main__":
    main()
