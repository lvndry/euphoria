#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests scripts/macos_libraries.py on real binaries. Needs macOS, the
Xcode command line tools and a release build of the desktop app:

    cargo build --release -p euphoria
    python3 -m unittest scripts/test_macos_libraries.py
"""

import pathlib
import platform
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import macos_libraries

REPOSITORY = pathlib.Path(__file__).resolve().parent.parent
BINARY = REPOSITORY / "target" / "release" / "euphoria-desktop"
RUN_PATH = "@executable_path/../Frameworks"


def run(*command):
    subprocess.run([str(part) for part in command], check=True, capture_output=True)


@unittest.skipUnless(
    platform.system() == "Darwin" and BINARY.exists(), "needs macOS and a release build"
)
class BundleTests(unittest.TestCase):
    def setUp(self):
        self.folder = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.folder)
        self.app = self.folder / "Test.app"
        self.executable = self.app / "Contents" / "MacOS" / "Test"
        self.frameworks = self.app / "Contents" / "Frameworks"
        self.executable.parent.mkdir(parents=True)
        shutil.copyfile(BINARY, self.executable)
        self.executable.chmod(0o755)

    def bundle(self):
        macos_libraries.bundle(self.app, self.executable, self.frameworks, RUN_PATH)

    def problems(self):
        return macos_libraries.verify(self.app, self.executable)

    def test_the_real_build_is_bundled_and_verified(self):
        self.assertTrue(self.problems(), "an unbundled binary should not verify")
        self.bundle()
        self.assertEqual(self.problems(), [])
        names = sorted(path.name for path in self.frameworks.iterdir())
        self.assertIn("libkeyfinder.2.dylib", names)
        self.assertIn("libfftw3.3.dylib", names)
        for path in [self.executable, *self.frameworks.iterdir()]:
            binary = macos_libraries.MachO(path)
            for install_name, _ in binary.dependencies:
                self.assertTrue(
                    install_name.startswith(("@rpath/", "/usr/lib/", "/System/")),
                    f"{path.name} refers to {install_name}",
                )
            for run_path in binary.run_paths:
                self.assertTrue(
                    run_path.startswith("@"), f"{path.name} has the run path {run_path}"
                )

    def test_a_missing_library_inside_the_bundle_is_rejected(self):
        self.bundle()
        (self.frameworks / "libfftw3.3.dylib").unlink()
        problems = self.problems()
        self.assertTrue(
            any(
                "libfftw3.3.dylib" in problem and "does not resolve" in problem
                for problem in problems
            )
        )

    def test_a_library_needing_one_that_does_not_exist_cannot_be_bundled(self):
        self.bundle()
        keyfinder = self.frameworks / "libkeyfinder.2.dylib"
        run(
            "install_name_tool",
            "-change",
            "@rpath/libfftw3.3.dylib",
            "@rpath/libnot-there.dylib",
            keyfinder,
        )
        self.assertTrue(
            any("libnot-there.dylib" in problem for problem in self.problems())
        )

        # The same broken reference met while bundling stops the bundling.
        broken = self.folder / "elsewhere" / "libkeyfinder.2.dylib"
        broken.parent.mkdir()
        shutil.copyfile(keyfinder, broken)
        shutil.rmtree(self.frameworks)
        shutil.copyfile(BINARY, self.executable)
        run("install_name_tool", "-add_rpath", broken.parent, self.executable)
        original = next(
            name
            for name, _ in macos_libraries.MachO(self.executable).dependencies
            if "libkeyfinder" in name
        )
        run(
            "install_name_tool",
            "-change",
            original,
            "@rpath/libkeyfinder.2.dylib",
            self.executable,
        )
        with self.assertRaisesRegex(macos_libraries.LibraryError, "libnot-there.dylib"):
            self.bundle()

    def test_a_reference_through_a_run_path_outside_the_bundle_is_bundled(self):
        # The library is reached through @rpath and a run path that leads out
        # of the bundle, which the old check, looking for Homebrew paths, let
        # through.
        outside = self.folder / "outside"
        outside.mkdir()
        original = next(
            name
            for name, _ in macos_libraries.MachO(self.executable).dependencies
            if "libkeyfinder" in name
        )
        shutil.copyfile(
            macos_libraries.resolve(original, [(self.executable, [])], self.executable),
            outside / "libkeyfinder.2.dylib",
        )
        run(
            "install_name_tool",
            "-change",
            original,
            "@rpath/libkeyfinder.2.dylib",
            self.executable,
        )
        run("install_name_tool", "-add_rpath", outside, self.executable)
        problems = self.problems()
        self.assertTrue(any("outside" in problem for problem in problems), problems)

        self.bundle()
        self.assertEqual(self.problems(), [])
        self.assertTrue((self.frameworks / "libkeyfinder.2.dylib").exists())
        self.assertNotIn(str(outside), macos_libraries.MachO(self.executable).run_paths)

    def test_a_run_path_that_leads_outside_is_reported(self):
        self.bundle()
        run(
            "install_name_tool",
            "-add_rpath",
            "/opt/homebrew/lib",
            self.frameworks / "libfftw3.3.dylib",
        )
        self.assertTrue(
            any("/opt/homebrew/lib" in problem for problem in self.problems())
        )


if __name__ == "__main__":
    unittest.main()
