#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Signs the Mach-O files of a macOS app bundle or command line folder with codesign.

    macos_sign.py bundle --identity <identity> <Euphoria.app>
    macos_sign.py files --identity <identity> <file>...

`bundle` signs every Mach-O file inside the bundle other than its main
executable, the deepest first, and the bundle last, so each signature seals
files that are already signed. `files` signs the files in the order given.

The identity `-` is an ad hoc signature, without a timestamp. Any other
identity, a Developer ID Application certificate, is signed with the hardened
runtime and a secure timestamp, which notarization requires. No entitlements
are passed: nothing in Euphoria needs one under the hardened runtime.
"""

import argparse
import pathlib
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import macos_libraries

AD_HOC = "-"


def codesign_command(identity, path):
    command = ["codesign", "--force", "--sign", identity]
    if identity == AD_HOC:
        command.append("--timestamp=none")
    else:
        command += ["--options", "runtime", "--timestamp"]
    command.append(str(path))
    return command


def nested_files(bundle):
    main_executable = bundle / "Contents" / "MacOS" / "Euphoria"
    files = [
        path
        for path in macos_libraries.mach_o_files(bundle)
        if path != main_executable
    ]
    return sorted(files, key=lambda path: (-len(path.parts), str(path)))


def signing_order(bundle):
    return [*nested_files(bundle), bundle]


def sign(identity, paths):
    for path in paths:
        subprocess.run(codesign_command(identity, path), check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    bundle = commands.add_parser("bundle")
    bundle.add_argument("--identity", required=True)
    bundle.add_argument("bundle", type=pathlib.Path)
    files = commands.add_parser("files")
    files.add_argument("--identity", required=True)
    files.add_argument("files", nargs="+", type=pathlib.Path)
    arguments = parser.parse_args()
    if arguments.command == "bundle":
        paths = signing_order(arguments.bundle.resolve())
    else:
        paths = [path for path in arguments.files if path.is_file()]
    try:
        sign(arguments.identity, paths)
    except subprocess.CalledProcessError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
