# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests of scripts/linux_libraries.py, on libraries and programs built here
with cc. Needs Linux, cc, patchelf and readelf. The real build is the one
named by EUPHORIA_LINKED_BINARY, such as CI's debug build of the euphoria
command, which links libkeyfinder and FFTW.

    EUPHORIA_LINKED_BINARY=target/debug/euphoria \\
        python3 -m unittest scripts/test_linux_libraries.py
"""

import os
import pathlib
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).parent))
import linux_libraries  # noqa: E402

HAS_TOOLS = platform.system() == "Linux" and all(
    shutil.which(tool) for tool in ("cc", "patchelf", "readelf")
)


def compile_library(folder, name, source, needs=(), flags=()):
    """Builds lib<name>.so in `folder` from C `source`, needing the
    libraries `needs`, given as (folder, name)."""
    source_file = folder / f"{name}.c"
    source_file.write_text(source)
    output = folder / f"lib{name}.so"
    command = ["cc", "-shared", "-fPIC", "-o", str(output), str(source_file),
               f"-Wl,-soname,lib{name}.so", *flags]  # fmt: skip
    for library_folder, library in needs:
        command += [f"-L{library_folder}", f"-l{library}"]
    subprocess.run(command, check=True, capture_output=True)
    return output


def compile_program(folder, source, needs=(), flags=()):
    source_file = folder / "main.c"
    source_file.write_text(source)
    output = folder / "main"
    command = ["cc", "-o", str(output), str(source_file), *flags]
    for library_folder, library in needs:
        command += [f"-L{library_folder}", f"-l{library}"]
    subprocess.run(command, check=True, capture_output=True)
    return output


def readelf_needed(path):
    listing = subprocess.run(
        ["readelf", "-d", str(path)], capture_output=True, text=True, check=True
    ).stdout
    return re.findall(r"\(NEEDED\)\s+Shared library: \[(.+?)\]", listing)


def readelf_versions(path):
    listing = subprocess.run(
        ["readelf", "-V", "-W", str(path)], capture_output=True, text=True, check=True
    ).stdout
    needed = listing.split(".gnu.version_r", 1)
    if len(needed) < 2:
        return set()
    return set(re.findall(r"Name: (\S+)\s+Flags", needed[1]))


@unittest.skipUnless(HAS_TOOLS, "needs Linux, cc, patchelf and readelf")
class ReaderTests(unittest.TestCase):
    def test_needed_libraries_and_versions_match_readelf(self):
        candidates = [pathlib.Path(sys.executable).resolve(), pathlib.Path("/bin/ls")]
        linked = os.environ.get("EUPHORIA_LINKED_BINARY")
        if linked:
            candidates.append(pathlib.Path(linked))
        for path in candidates:
            binary = linux_libraries.Elf(path)
            self.assertEqual(binary.needed, readelf_needed(path), path)
            self.assertEqual(
                {name for _, name in binary.version_needs}, readelf_versions(path), path
            )

    def test_the_minimum_glibc_is_the_newest_version_asked_for(self):
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            shutil.copy("/bin/ls", root / "ls")
            versions = [
                name[len("GLIBC_"):]
                for name in readelf_versions(root / "ls")
                if re.fullmatch(r"GLIBC_[0-9.]+", name)
            ]
            newest = max(versions, key=linux_libraries.version_key)
            self.assertEqual(linux_libraries.minimum_glibc([root]), newest)

    def test_a_folder_without_programs_has_no_minimum_glibc(self):
        with tempfile.TemporaryDirectory() as folder:
            pathlib.Path(folder, "notes.txt").write_text("text")
            self.assertIsNone(linux_libraries.minimum_glibc([pathlib.Path(folder)]))


@unittest.skipUnless(HAS_TOOLS, "needs Linux, cc, patchelf and readelf")
class SyntheticLibraryTests(unittest.TestCase):
    def setUp(self):
        self.scratch = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.scratch)
        self.outside = self.scratch / "outside"
        self.outside.mkdir()
        self.root = self.scratch / "package"
        (self.root / "bin").mkdir(parents=True)

    def build_chain(self, flags=()):
        """A program that needs libouter, which needs libinner, both in a
        folder outside the package that the program's run path names."""
        compile_library(self.outside, "inner", "int inner(void) { return 2; }\n")
        compile_library(
            self.outside,
            "outer",
            "int inner(void);\nint outer(void) { return inner() + 1; }\n",
            needs=[(self.outside, "inner")],
            flags=[f"-Wl,-rpath,{self.outside}"],
        )
        program = compile_program(
            self.outside,
            '#include <math.h>\nint outer(void);\n'
            'int main(int count, char **values) { return outer() == 3 && sqrt(count) > 0 ? 0 : 1; }\n',
            needs=[(self.outside, "outer")],
            flags=[f"-Wl,-rpath,{self.outside}", "-lm", *flags],
        )
        executable = self.root / "bin" / "main"
        shutil.copy(program, executable)
        return executable

    def bundle(self, executable):
        linux_libraries.bundle(
            self.root, executable, self.root / "lib", "$ORIGIN/../lib"
        )

    def test_a_chain_of_libraries_is_copied_and_runs_from_the_package(self):
        executable = self.build_chain()
        self.bundle(executable)
        self.assertEqual(
            sorted(path.name for path in (self.root / "lib").iterdir()),
            ["libinner.so", "libouter.so"],
        )
        self.assertEqual(linux_libraries.verify(self.root, executable), [])
        shutil.rmtree(self.outside)
        subprocess.run([str(executable)], check=True, env={"PATH": "/usr/bin:/bin"})

    def test_existing_library_symlinks_cannot_modify_files_outside_the_package(self):
        self.assert_existing_library_link_is_replaced(symbolic=True)

    def test_existing_library_hardlinks_cannot_modify_files_outside_the_package(self):
        self.assert_existing_library_link_is_replaced(symbolic=False)

    def assert_existing_library_link_is_replaced(self, symbolic):
        executable = self.build_chain()
        libraries = self.root / "lib"
        libraries.mkdir()
        unrelated = self.outside / "unrelated"
        unrelated.write_bytes(b"keep this file")
        destination = libraries / "libinner.so"
        if symbolic:
            destination.symlink_to(unrelated)
        else:
            os.link(unrelated, destination)
        self.bundle(executable)
        self.assertEqual(unrelated.read_bytes(), b"keep this file")
        self.assertEqual(linux_libraries.verify(self.root, executable), [])

    def test_an_executable_outside_the_package_is_not_modified(self):
        program = compile_program(self.outside, "int main(void) { return 0; }\n")
        original = program.read_bytes()
        with self.assertRaisesRegex(linux_libraries.LibraryError, "not under"):
            self.bundle(program)
        self.assertEqual(program.read_bytes(), original)

    def test_absolute_needed_paths_are_relocated_without_changing_sources(self):
        self.assert_needed_path_is_relocated(None)

    def test_origin_in_needed_paths_is_expanded_and_relocated(self):
        self.assert_needed_path_is_relocated("$ORIGIN/libinner.so")

    def assert_needed_path_is_relocated(self, name):
        inner = self.outside / "libinner.so"
        needed = str(inner) if name is None else name
        versions = self.outside / "inner.versions"
        versions.write_text("INNER_1 { global: inner; local: *; };\n")
        flags = [f"-Wl,-soname,{needed}"]
        if name is None:
            flags.append(f"-Wl,--version-script,{versions}")
        compile_library(self.outside, "inner", "int inner(void) { return 2; }\n", flags=flags)
        outer = compile_library(
            self.outside, "outer", "int inner(void); int outer(void) { return inner(); }\n",
            needs=[(self.outside, "inner")], flags=[f"-Wl,-rpath,{self.outside}"])
        original = outer.read_bytes()
        program = compile_program(
            self.outside, "int outer(void); int main(void) { return outer() == 2 ? 0 : 1; }\n",
            needs=[(self.outside, "outer")],
            flags=[f"-Wl,-rpath,{self.outside}", "-Wl,--allow-shlib-undefined"])
        subprocess.run([str(program)], check=True, env={"PATH": "/usr/bin:/bin"})
        executable = self.root / "bin" / "main"
        shutil.copy(program, executable)
        self.assertIn(needed, linux_libraries.Elf(outer).needed)
        self.bundle(executable)
        self.assertEqual(linux_libraries.verify(self.root, executable), [])
        self.assertEqual(outer.read_bytes(), original)
        self.assertIn("libinner.so", linux_libraries.Elf(self.root / "lib" / "libouter.so").needed)
        shutil.rmtree(self.outside)
        subprocess.run([str(executable)], check=True, env={"PATH": "/usr/bin:/bin"})

    def test_libraries_found_through_an_old_style_rpath_are_copied(self):
        executable = self.build_chain(flags=["-Wl,--disable-new-dtags"])
        self.assertIsNotNone(linux_libraries.Elf(executable).rpaths)
        self.bundle(executable)
        self.assertEqual(linux_libraries.verify(self.root, executable), [])
        self.assertIsNone(linux_libraries.Elf(executable).rpaths)
        self.assertEqual(linux_libraries.Elf(executable).run_paths, ["$ORIGIN/../lib"])

    def test_base_system_libraries_stay_outside(self):
        executable = self.build_chain()
        self.bundle(executable)
        self.assertIn("libm.so.6", linux_libraries.Elf(executable).needed)
        self.assertFalse((self.root / "lib" / "libm.so.6").exists())
        self.assertFalse((self.root / "lib" / "libc.so.6").exists())

    def test_a_missing_library_in_the_package_is_reported(self):
        executable = self.build_chain()
        self.bundle(executable)
        (self.root / "lib" / "libinner.so").unlink()
        problems = linux_libraries.verify(self.root, executable)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("needs libinner.so, which is not in", problems[0])

    def test_a_library_loaded_from_outside_is_reported(self):
        executable = self.build_chain()
        problems = linux_libraries.verify(self.root, executable)
        self.assertTrue(
            any(f"has the run path {self.outside}" in problem for problem in problems),
            problems,
        )
        self.assertTrue(
            any("loads" in problem and "from outside" in problem for problem in problems),
            problems,
        )

    def test_a_library_that_cannot_be_found_stops_the_bundle(self):
        executable = self.build_chain()
        (self.outside / "libinner.so").unlink()
        with self.assertRaisesRegex(linux_libraries.LibraryError, "libinner.so, which was not found"):
            self.bundle(executable)

    def test_a_file_that_is_not_a_library_does_not_stop_the_search(self):
        decoy = self.scratch / "decoy"
        decoy.mkdir()
        executable = self.build_chain()
        (decoy / "libouter.so").write_text("not a library")
        subprocess.run(
            ["patchelf", "--set-rpath", f"{decoy}:{self.outside}", str(executable)],
            check=True,
        )
        self.bundle(executable)
        self.assertEqual(linux_libraries.verify(self.root, executable), [])
        self.assertTrue(linux_libraries.is_elf(self.root / "lib" / "libouter.so"))

    def test_a_run_path_with_a_machine_dependent_part_stops_the_bundle(self):
        executable = self.build_chain()
        subprocess.run(
            ["patchelf", "--set-rpath", "$PLATFORM/lib", str(executable)], check=True
        )
        with self.assertRaisesRegex(linux_libraries.LibraryError, "cannot be expanded"):
            self.bundle(executable)


@unittest.skipUnless(
    HAS_TOOLS and os.environ.get("EUPHORIA_LINKED_BINARY"),
    "needs Linux tools and EUPHORIA_LINKED_BINARY",
)
class RealBuildTests(unittest.TestCase):
    def test_the_real_build_carries_libkeyfinder_and_fftw_and_runs(self):
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder) / "euphoria"
            (root / "bin").mkdir(parents=True)
            executable = root / "bin" / "euphoria"
            shutil.copy(os.environ["EUPHORIA_LINKED_BINARY"], executable)
            linux_libraries.bundle(root, executable, root / "lib", "$ORIGIN/../lib")
            bundled = sorted(path.name for path in (root / "lib").iterdir())
            self.assertTrue(any(name.startswith("libkeyfinder") for name in bundled), bundled)
            self.assertIn("libfftw3.so.3", bundled)
            self.assertIn("libfftw3_threads.so.3", bundled)
            self.assertEqual(linux_libraries.verify(root, executable), [])
            self.assertRegex(linux_libraries.minimum_glibc([root]), r"^2\.[0-9]+$")
            result = subprocess.run(
                [str(executable), "--version"],
                env={"PATH": "/usr/bin:/bin", "LD_LIBRARY_PATH": ""},
                capture_output=True, text=True, check=True,
            )  # fmt: skip
            self.assertIn("euphoria", result.stdout)


if __name__ == "__main__":
    unittest.main()
