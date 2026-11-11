#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests scripts/macos_libraries.py on real binaries and on small libraries
built with clang. Needs macOS, the Xcode command line tools and, for the
tests on a real binary, a build that links libkeyfinder:

    cargo build --release -p euphoria
    python3 -m unittest scripts/test_macos_libraries.py

The real binary is the release build of the desktop app, or the file named
by EUPHORIA_LINKED_BINARY, such as CI's debug build of the euphoria command,
which links the same libraries.
"""

import os
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
BINARY = pathlib.Path(
    os.environ.get(
        "EUPHORIA_LINKED_BINARY",
        REPOSITORY / "target" / "release" / "euphoria-desktop",
    )
)
RUN_PATH = "@executable_path/../Frameworks"


def run(*command):
    subprocess.run([str(part) for part in command], check=True, capture_output=True)


@unittest.skipUnless(
    platform.system() == "Darwin" and BINARY.exists(),
    "needs macOS and a build that links libkeyfinder",
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


def compile_library(folder, name, links=(), architecture="arm64", minimum="13.0"):
    """Builds lib<name>.dylib in `folder` with the install name
    @rpath/lib<name>.dylib, linked to the libraries named in `links`."""
    source = folder / f"{name}.c"
    source.write_text(f"int {name}_value(void) {{ return 1; }}\n")
    run(
        "clang",
        "-dynamiclib",
        "-arch",
        architecture,
        f"-mmacosx-version-min={minimum}",
        "-install_name",
        f"@rpath/lib{name}.dylib",
        "-o",
        folder / f"lib{name}.dylib",
        source,
        "-L",
        folder,
        *[f"-l{link}" for link in links],
    )
    return folder / f"lib{name}.dylib"


def compile_executable(path, folder, links):
    source = folder / "main.c"
    source.write_text("int main(void) { return 0; }\n")
    path.parent.mkdir(parents=True, exist_ok=True)
    run(
        "clang",
        "-arch",
        "arm64",
        "-mmacosx-version-min=13.0",
        "-o",
        path,
        source,
        "-L",
        folder,
        *[f"-l{link}" for link in links],
        "-Wl,-rpath," + str(folder),
    )


@unittest.skipUnless(
    platform.system() == "Darwin" and shutil.which("clang"), "needs macOS and clang"
)
class SyntheticLibraryTests(unittest.TestCase):
    def setUp(self):
        self.folder = pathlib.Path(tempfile.mkdtemp()).resolve()
        self.addCleanup(shutil.rmtree, self.folder)
        self.outside = self.folder / "outside"
        self.outside.mkdir()
        self.app = self.folder / "Test.app"
        self.executable = self.app / "Contents" / "MacOS" / "Test"
        self.frameworks = self.app / "Contents" / "Frameworks"

    def bundle_and_verify(self):
        macos_libraries.bundle(self.app, self.executable, self.frameworks, RUN_PATH)
        return macos_libraries.verify(self.app, self.executable)

    def test_a_cycle_between_libraries_is_bundled_and_the_walk_ends(self):
        # first needs second and second needs first. A first build of
        # first lets second link against it.
        compile_library(self.outside, "first")
        compile_library(self.outside, "second", links=["first"])
        compile_library(self.outside, "first", links=["second"])
        compile_executable(self.executable, self.outside, ["first"])
        self.assertEqual(self.bundle_and_verify(), [])
        self.assertEqual(
            sorted(path.name for path in self.frameworks.iterdir()),
            ["libfirst.dylib", "libsecond.dylib"],
        )

    def test_a_chain_longer_than_the_old_limit_is_followed_to_its_end(self):
        length = 80
        for index in reversed(range(length)):
            links = [f"link{index + 1}"] if index + 1 < length else []
            compile_library(self.outside, f"link{index}", links=links)
        compile_executable(self.executable, self.outside, ["link0"])
        self.assertEqual(self.bundle_and_verify(), [])
        self.assertEqual(len(list(self.frameworks.iterdir())), length)

        # The end of the chain going missing is found, however deep it is.
        (self.frameworks / f"liblink{length - 1}.dylib").unlink()
        problems = macos_libraries.verify(self.app, self.executable)
        self.assertTrue(
            any(f"liblink{length - 1}.dylib" in problem for problem in problems),
            problems,
        )

    def test_a_walk_that_would_not_end_is_an_error(self):
        compile_library(self.outside, "first")
        compile_executable(self.executable, self.outside, ["first"])
        limit = macos_libraries.MOST_WALK_STEPS
        macos_libraries.MOST_WALK_STEPS = 1
        self.addCleanup(setattr, macos_libraries, "MOST_WALK_STEPS", limit)
        with self.assertRaisesRegex(macos_libraries.LibraryError, "more than 1 steps"):
            macos_libraries.bundle(self.app, self.executable, self.frameworks, RUN_PATH)

    def test_the_minimum_macos_is_the_newest_of_every_slice_of_every_file(self):
        compile_executable(self.executable, self.outside, [])
        nested = self.frameworks / "Nested.framework" / "Versions" / "A"
        nested.mkdir(parents=True)
        # One file holds an arm64 slice for macOS 14.2, with LC_BUILD_VERSION,
        # and an x86_64 slice for 10.13, which older tools mark with
        # LC_VERSION_MIN_MACOSX.
        arm = compile_library(self.outside, "armslice", minimum="14.2")
        intel = self.outside / "libintelslice.dylib"
        run(
            "clang",
            "-dynamiclib",
            "-arch",
            "x86_64",
            "-mmacosx-version-min=10.13",
            "-install_name",
            "@rpath/libarmslice.dylib",
            "-o",
            intel,
            self.outside / "armslice.c",
        )
        run("lipo", "-create", arm, intel, "-output", nested / "Nested")
        versions = macos_libraries.macos_versions(nested / "Nested")
        self.assertEqual(
            sorted(versions, key=macos_libraries.version_key), ["10.13", "14.2"]
        )
        self.assertEqual(macos_libraries.minimum_macos(self.app, "11.0"), "14.2")
        self.assertEqual(macos_libraries.minimum_macos(self.app, "15.0"), "15.0")


@unittest.skipUnless(
    platform.system() == "Darwin" and BINARY.exists(),
    "needs macOS and a build that links libkeyfinder",
)
class RealBundleVersionTests(unittest.TestCase):
    def test_the_real_bundle_needs_the_newest_macos_of_its_files(self):
        folder = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, folder)
        app = folder / "Test.app"
        executable = app / "Contents" / "MacOS" / "Test"
        executable.parent.mkdir(parents=True)
        shutil.copyfile(BINARY, executable)
        executable.chmod(0o755)
        macos_libraries.bundle(
            app, executable, app / "Contents" / "Frameworks", RUN_PATH
        )
        found = macos_libraries.minimum_macos(app, "11.0")
        for path in macos_libraries.mach_o_files(app):
            for version in macos_libraries.macos_versions(path):
                self.assertGreaterEqual(
                    macos_libraries.version_key(found),
                    macos_libraries.version_key(version),
                    path.name,
                )
        self.assertIn(
            found,
            [
                version
                for path in macos_libraries.mach_o_files(app)
                for version in macos_libraries.macos_versions(path)
            ],
        )


if __name__ == "__main__":
    unittest.main()
