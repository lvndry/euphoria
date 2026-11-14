#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests the order and flags of scripts/macos_sign.py with a recording codesign.

    python3 -m unittest scripts/test_macos_sign.py
"""

import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parent / "macos_sign.py"
MACH_O_MAGIC = bytes.fromhex("cffaedfe") + b"\0" * 12


class SigningOrderTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="euphoria sign ")
        self.addCleanup(temporary.cleanup)
        self.folder = pathlib.Path(temporary.name).resolve()
        self.log = self.folder / "codesign.log"
        tools = self.folder / "tools"
        tools.mkdir()
        codesign = tools / "codesign"
        codesign.write_text(
            f"#!{sys.executable}\nimport sys\n"
            f"open({str(self.log)!r}, 'a').write('\\t'.join(sys.argv[1:]) + '\\n')\n"
        )
        codesign.chmod(0o755)
        self.environment = {**os.environ, "PATH": f"{tools}{os.pathsep}{os.environ['PATH']}"}
        self.bundle = self.folder / "Euphoria.app"
        for relative in (
            "Contents/MacOS/Euphoria",
            "Contents/Frameworks/libkeyfinder.dylib",
            "Contents/Frameworks/libfftw3.dylib",
            "Contents/Frameworks/Helper.framework/Versions/A/Helper",
            "Contents/Helpers/tool",
        ):
            path = self.bundle / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(MACH_O_MAGIC)
        (self.bundle / "Contents/Info.plist").write_text("not a Mach-O file")

    def run_script(self, *arguments):
        subprocess.run(
            [sys.executable, str(SCRIPT), *arguments],
            check=True,
            env=self.environment,
        )
        return [line.split("\t") for line in self.log.read_text().splitlines()]

    def test_the_bundle_is_signed_last_and_the_deepest_files_first(self):
        calls = self.run_script("bundle", "--identity", "Developer ID Application: Test (TEAM)", str(self.bundle))
        signed = [pathlib.Path(call[-1]) for call in calls]
        self.assertEqual(signed[-1], self.bundle)
        self.assertEqual(signed[0], self.bundle / "Contents/Frameworks/Helper.framework/Versions/A/Helper")
        depths = [len(path.parts) for path in signed[:-1]]
        self.assertEqual(depths, sorted(depths, reverse=True))
        self.assertNotIn(self.bundle / "Contents/MacOS/Euphoria", signed)
        self.assertNotIn(self.bundle / "Contents/Info.plist", signed)
        self.assertEqual(len(signed), 5)

    def test_a_developer_id_uses_the_hardened_runtime_and_a_timestamp(self):
        calls = self.run_script("bundle", "--identity", "Developer ID Application: Test (TEAM)", str(self.bundle))
        for call in calls:
            self.assertEqual(
                call[:-1],
                ["--force", "--sign", "Developer ID Application: Test (TEAM)", "--options", "runtime", "--timestamp"],
            )

    def test_the_ad_hoc_identity_has_no_runtime_and_no_timestamp(self):
        calls = self.run_script("bundle", "--identity", "-", str(self.bundle))
        for call in calls:
            self.assertEqual(call[:-1], ["--force", "--sign", "-", "--timestamp=none"])

    def test_files_are_signed_in_the_order_given(self):
        first = self.bundle / "Contents/Frameworks/libkeyfinder.dylib"
        second = self.bundle / "Contents/MacOS/Euphoria"
        calls = self.run_script("files", "--identity", "-", str(first), str(second))
        self.assertEqual([call[-1] for call in calls], [str(first), str(second)])


if __name__ == "__main__":
    unittest.main()
