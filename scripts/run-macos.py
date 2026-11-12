#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Give cargo run's desktop binary a macOS application identity.

The CLI, test executables and benchmarks pass through unchanged. This is a
local development bundle; release bundles still use bundle-macos.sh.
"""

from contextlib import contextmanager
import os
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import sys
import tempfile


@contextmanager
def termination_handlers(handler):
    previous = {}
    try:
        for number in (signal.SIGTERM, signal.SIGHUP, signal.SIGQUIT):
            previous[number] = signal.signal(number, handler)
        yield
    finally:
        for number, original in previous.items():
            signal.signal(number, original)


def stop_startup(number, _):
    # SystemExit unwinds the temporary bundle and subprocess.run kills and
    # reaps iconutil if conversion is still running.
    raise SystemExit(128 + number)


def main():
    binary = Path(sys.argv[1]).absolute()
    arguments = sys.argv[2:]
    if binary.name != "euphoria-desktop":
        os.execv(binary, [str(binary), *arguments])

    with termination_handlers(stop_startup), tempfile.TemporaryDirectory(prefix="euphoria-run-") as temporary:
        contents = Path(temporary) / "Euphoria.app" / "Contents"
        executable = contents / "MacOS" / binary.name
        executable.parent.mkdir(parents=True)
        resources = contents / "Resources"
        resources.mkdir()
        shutil.copy2(binary, executable)
        (contents / "Info.plist").write_bytes(plistlib.dumps({
            "CFBundleName": "Euphoria",
            "CFBundleDisplayName": "Euphoria",
            "CFBundleIdentifier": "io.github.lvndry.euphoria",
            "CFBundleExecutable": binary.name,
            "CFBundlePackageType": "APPL",
            "CFBundleIconFile": "AppIcon.icns",
            "NSHighResolutionCapable": True,
        }))
        iconset = Path(temporary) / "AppIcon.iconset"
        iconset.mkdir()
        artwork = Path(__file__).resolve().parents[1] / "crates/euphoria-desktop/assets/dock-icon.png"
        shutil.copyfile(artwork, iconset / "icon_512x512@2x.png")
        subprocess.run(["iconutil", "--convert", "icns", "--output",
                        str(resources / "AppIcon.icns"), str(iconset)], check=True)
        child = subprocess.Popen([str(executable), *arguments])
        with termination_handlers(lambda number, _: child.send_signal(number)):
            try:
                status = child.wait()
            except KeyboardInterrupt:
                child.send_signal(signal.SIGINT)
                child.wait()
                status = -signal.SIGINT
        return status if status >= 0 else 128 - status


if __name__ == "__main__":
    sys.exit(main())
