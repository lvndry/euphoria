# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Run the installation guide's real launcher commands on scratch AppImages."""

import ctypes
import ctypes.util
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]


class LinuxLauncherTests(unittest.TestCase):
    def create_launcher(self, home_name):
        temporary = tempfile.TemporaryDirectory(prefix="euphoria-launcher-")
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        home = root / home_name
        (home / "Applications").mkdir(parents=True)
        (home / ".local/share").mkdir(parents=True)
        installed = home / "Applications/Euphoria-x86_64.AppImage"
        marker = root / "launched"
        installed.write_text('#!/bin/sh\nprintf "%s" "$0" > "$EUPHORIA_LAUNCH_MARKER"\n')
        installed.chmod(0o755)
        extractor = root / "Euphoria-x86_64.AppImage"
        extractor.write_text('''#!/bin/sh
set -eu
mkdir -p squashfs-root/usr/share/applications squashfs-root/usr/share/icons
cat > squashfs-root/usr/share/applications/euphoria.desktop <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Euphoria
Exec=euphoria-desktop
Terminal=false
DESKTOP
''')
        extractor.chmod(0o755)
        document = (ROOT / "docs/getting-started/installation.md").read_text()
        blocks = re.findall(r"```[^\n]*\n(.*?)```", document, re.S)
        commands, = [block for block in blocks if "--appimage-extract 'usr/share/*'" in block]
        environment = {**os.environ, "HOME": str(home), "EUPHORIA_LAUNCH_MARKER": str(marker)}
        result = subprocess.run(["sh", "-eu", "-c", commands], cwd=root, env=environment,
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        launcher = home / ".local/share/applications/euphoria.desktop"
        return launcher, installed, marker, environment

    def assert_reference_launch(self, launcher, installed, marker, environment):
        if sys.platform != "linux":
            return  # The generated command is checked above on other hosts.
        # Use the actual GLib desktop-entry parser/launcher, without a GUI,
        # shell approximation or a user-installed application.
        library = ctypes.util.find_library("gio-2.0")
        if library is None:
            self.skipTest("GLib's desktop-entry parser is unavailable")
        gio = ctypes.CDLL(library)
        gio.g_desktop_app_info_new_from_filename.argtypes = [ctypes.c_char_p]
        gio.g_desktop_app_info_new_from_filename.restype = ctypes.c_void_p
        gio.g_app_info_launch.argtypes = [ctypes.c_void_p] * 4
        gio.g_app_info_launch.restype = ctypes.c_int
        gio.g_object_unref.argtypes = [ctypes.c_void_p]
        info = gio.g_desktop_app_info_new_from_filename(os.fsencode(launcher))
        self.assertTrue(info, launcher.read_text())
        gio.g_app_launch_context_new.restype = ctypes.c_void_p
        gio.g_app_launch_context_setenv.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
        context = gio.g_app_launch_context_new()
        for name in ("HOME", "EUPHORIA_LAUNCH_MARKER"):
            gio.g_app_launch_context_setenv(context, name.encode(), os.fsencode(environment[name]))
        try:
            self.assertTrue(gio.g_app_info_launch(info, None, context, None))
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                if marker.exists() and marker.read_text() == str(installed):
                    break
                time.sleep(0.01)
            self.assertEqual(marker.read_text(), str(installed))
        finally:
            gio.g_object_unref(context)
            gio.g_object_unref(info)

    def test_manual_launcher_handles_a_home_directory_with_spaces(self):
        self.assert_reference_launch(*self.create_launcher("Home with spaces"))

    def test_manual_launcher_preserves_reserved_path_characters(self):
        home = 'Home & | %f "quotes" $cash \\slash ' + chr(96) + 'tick'
        self.assert_reference_launch(*self.create_launcher(home))


if __name__ == "__main__":
    unittest.main()
