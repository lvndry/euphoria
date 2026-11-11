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


def otool_listing(*platform_lines):
    """otool -l output with one LC_BUILD_VERSION per `platform` line given as
    (platform, minimum), as two slices of a universal file print it."""
    parts = []
    for architecture, (platform_value, minimum) in zip(
        ["x86_64", "arm64", "arm64e"], platform_lines, strict=False
    ):
        parts.append(
            f"""lib.dylib (architecture {architecture}):
Load command 9
      cmd LC_BUILD_VERSION
  cmdsize 32
 platform {platform_value}
    minos {minimum}
      sdk 27.0
   ntools 1
     tool LD
  version 1167.4
Load command 10
          cmd LC_LOAD_DYLIB
      cmdsize 56
         name /usr/lib/libSystem.B.dylib (offset 24)
"""
        )
    return "".join(parts)


class OtoolOutputTests(unittest.TestCase):
    def test_the_platform_is_read_as_a_number_or_a_name(self):
        for macos in ["1", "MACOS", "macos"]:
            listing = otool_listing((macos, "14.2"), (macos, "27.0"))
            self.assertEqual(
                macos_libraries.parse_macos_versions(listing, "lib.dylib"),
                ["14.2", "27.0"],
                macos,
            )

    def test_other_platforms_do_not_set_the_minimum_macos(self):
        for other in ["6", "MACCATALYST", "2", "IOS"]:
            listing = otool_listing(("MACOS", "13.0"), (other, "18.0"))
            self.assertEqual(
                macos_libraries.parse_macos_versions(listing, "lib.dylib"),
                ["13.0"],
                other,
            )

    def test_an_unknown_platform_is_an_error(self):
        for unknown in ["99", "NEWOS"]:
            listing = otool_listing(("1", "13.0"), (unknown, "30.0"))
            with self.assertRaisesRegex(
                macos_libraries.LibraryError, f"unknown platform {unknown}"
            ):
                macos_libraries.parse_macos_versions(listing, "lib.dylib")

    def test_older_minimum_commands_are_read_too(self):
        listing = """lib.dylib:
Load command 8
      cmd LC_VERSION_MIN_MACOSX
  cmdsize 16
  version 10.13
      sdk 10.14
"""
        self.assertEqual(
            macos_libraries.parse_macos_versions(listing, "lib.dylib"), ["10.13"]
        )


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
        # Room for the run paths some tests add to it.
        "-Wl,-headerpad_max_install_names",
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
        # Room for the longer library names some tests write into it.
        "-Wl,-headerpad_max_install_names",
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

    def replace_run_paths(self, binary, run_paths):
        for existing in macos_libraries.MachO(binary).run_paths:
            run("install_name_tool", "-delete_rpath", existing, binary)
        for run_path in run_paths:
            run("install_name_tool", "-add_rpath", run_path, binary)

    def test_an_rpath_search_passes_over_what_dyld_cannot_load(self):
        compile_library(self.outside, "first")
        compile_executable(self.executable, self.outside, ["first"])
        # Earlier run paths hold a folder and a text file named like the
        # library; the real one is in the last.
        folder_decoy = self.folder / "folder-decoy"
        (folder_decoy / "libfirst.dylib").mkdir(parents=True)
        file_decoy = self.folder / "file-decoy"
        file_decoy.mkdir()
        (file_decoy / "libfirst.dylib").write_text("not a library\n")
        self.replace_run_paths(
            self.executable, [str(folder_decoy), str(file_decoy), str(self.outside)]
        )
        chain = [(self.executable, macos_libraries.MachO(self.executable).run_paths)]
        self.assertEqual(
            macos_libraries.resolve("@rpath/libfirst.dylib", chain, self.executable),
            self.outside / "libfirst.dylib",
        )
        self.assertEqual(self.bundle_and_verify(), [])
        self.assertTrue(macos_libraries.is_mach_o(self.frameworks / "libfirst.dylib"))

    def test_nothing_loadable_on_any_run_path_stops_the_build(self):
        compile_library(self.outside, "first")
        compile_executable(self.executable, self.outside, ["first"])
        folder_decoy = self.folder / "folder-decoy"
        (folder_decoy / "libfirst.dylib").mkdir(parents=True)
        self.replace_run_paths(self.executable, [str(folder_decoy)])
        with self.assertRaisesRegex(macos_libraries.LibraryError, "libfirst.dylib"):
            macos_libraries.bundle(self.app, self.executable, self.frameworks, RUN_PATH)

    def test_run_paths_inside_the_bundle_are_kept_and_others_removed(self):
        compile_library(self.outside, "first")
        compile_executable(self.executable, self.outside, ["first"])
        kept = [
            "@executable_path/../Frameworks",
            "@executable_path",
            "@loader_path/../Frameworks",
            "@loader_path",
        ]
        removed = [
            str(self.outside),
            # Contents/MacOS/../../.. is the folder holding the app.
            "@loader_path/../../..",
            "@executable_path/../../../outside",
            "relative/folder",
        ]
        self.replace_run_paths(self.executable, [*removed[:1], *kept, *removed[1:]])
        self.assertEqual(self.bundle_and_verify(), [])
        self.assertEqual(macos_libraries.MachO(self.executable).run_paths, kept)

        # A library in the bundle keeps @loader_path run paths that lead into
        # the bundle from where it is, and loses one that leads out of it.
        library = self.frameworks / "libfirst.dylib"
        run("install_name_tool", "-add_rpath", "@loader_path", library)
        run("install_name_tool", "-add_rpath", "@loader_path/../../..", library)
        self.assertEqual(
            macos_libraries.verify(self.app, self.executable),
            [f"{library} has the run path @loader_path/../../.., outside {self.app}"],
        )
        macos_libraries.bundle(self.app, self.executable, self.frameworks, RUN_PATH)
        self.assertEqual(macos_libraries.MachO(library).run_paths, ["@loader_path"])
        self.assertEqual(macos_libraries.verify(self.app, self.executable), [])

    def test_a_run_path_dyld_does_not_expand_stops_the_build(self):
        compile_library(self.outside, "first")
        compile_executable(self.executable, self.outside, ["first"])
        self.replace_run_paths(self.executable, ["@rpath/../lib", str(self.outside)])
        message = "names @rpath/../lib, a form that cannot be expanded"
        with self.assertRaisesRegex(macos_libraries.LibraryError, message):
            macos_libraries.bundle(self.app, self.executable, self.frameworks, RUN_PATH)
        with self.assertRaisesRegex(macos_libraries.LibraryError, message):
            macos_libraries.verify(self.app, self.executable)

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

    def test_universal_files_with_32_and_64_bit_offsets_are_mach_o(self):
        arm = compile_library(self.outside, "armpart")
        intel = self.outside / "libintelpart.dylib"
        run(
            "clang",
            "-dynamiclib",
            "-arch",
            "x86_64",
            "-mmacosx-version-min=10.13",
            "-o",
            intel,
            self.outside / "armpart.c",
        )
        universal = self.outside / "universal.dylib"
        universal_64 = self.outside / "universal64.dylib"
        run("lipo", "-create", arm, intel, "-output", universal)
        run("lipo", "-create", "-fat64", arm, intel, "-output", universal_64)
        self.assertEqual(universal.read_bytes()[:4], bytes.fromhex("cafebabe"))
        self.assertEqual(universal_64.read_bytes()[:4], bytes.fromhex("cafebabf"))
        for path in (arm, intel, universal, universal_64):
            self.assertTrue(macos_libraries.is_mach_o(path), path.name)
        self.assertEqual(
            sorted(
                macos_libraries.macos_versions(universal_64),
                key=macos_libraries.version_key,
            ),
            ["10.13", "13.0"],
        )

        # The byte swapped forms, and files that are not Mach-O.
        swapped = self.outside / "swapped"
        swapped.write_bytes(bytes.fromhex("bfbafeca") + bytes(28))
        self.assertTrue(macos_libraries.is_mach_o(swapped))
        for name, start in [("text", b"#!/b"), ("empty", b""), ("short", b"\xca\xfe")]:
            other = self.outside / name
            other.write_bytes(start)
            self.assertFalse(macos_libraries.is_mach_o(other), name)

        # A 64 bit universal library in the bundle is found and scanned.
        nested = self.frameworks / "Universal64.framework"
        nested.mkdir(parents=True)
        shutil.copyfile(universal_64, nested / "Universal64")
        self.assertIn(nested / "Universal64", macos_libraries.mach_o_files(self.app))

    def test_a_name_that_leads_out_of_the_system_folders_is_not_a_system_library(self):
        for name in [
            "/usr/lib/libSystem.B.dylib",
            "/usr/lib/swift/libswiftCore.dylib",
            "/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit",
            "/usr/lib/./libc++.1.dylib",
        ]:
            self.assertTrue(macos_libraries.is_system(name), name)
        for name in [
            "/usr/lib/../../opt/homebrew/lib/libexample.dylib",
            "/System/../opt/lib.dylib",
            "/usr/libexec/lib.dylib",
            "/usr/local/lib/lib.dylib",
            "@rpath/libexample.dylib",
            "usr/lib/libexample.dylib",
        ]:
            self.assertFalse(macos_libraries.is_system(name), name)

    def test_a_library_reached_through_dot_dot_from_usr_lib_is_bundled(self):
        library = compile_library(self.outside, "escape")
        compile_executable(self.executable, self.outside, ["escape"])
        sneaky = "/usr/lib/../.." + str(library)
        run(
            "install_name_tool",
            "-change",
            "@rpath/libescape.dylib",
            sneaky,
            self.executable,
        )
        self.assertIn(
            sneaky,
            [name for name, _ in macos_libraries.MachO(self.executable).dependencies],
        )
        # verify reports it as a library from outside the bundle.
        problems = macos_libraries.verify(self.app, self.executable)
        self.assertTrue(
            any(
                "libescape.dylib" in problem and "outside" in problem
                for problem in problems
            ),
            problems,
        )
        # bundle copies it in and rewrites the reference.
        self.assertEqual(self.bundle_and_verify(), [])
        self.assertTrue((self.frameworks / "libescape.dylib").exists())
        names = [
            name for name, _ in macos_libraries.MachO(self.executable).dependencies
        ]
        self.assertIn("@rpath/libescape.dylib", names)
        self.assertNotIn(sneaky, names)

    def test_a_missing_library_named_through_dot_dot_stops_the_build(self):
        compile_library(self.outside, "gone")
        compile_executable(self.executable, self.outside, ["gone"])
        sneaky = "/usr/lib/../.." + str(self.outside / "libnot-there.dylib")
        run(
            "install_name_tool",
            "-change",
            "@rpath/libgone.dylib",
            sneaky,
            self.executable,
        )
        with self.assertRaisesRegex(macos_libraries.LibraryError, "libnot-there.dylib"):
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
