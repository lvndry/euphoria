#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Makes a macOS executable carry every library it needs, and proves it does.

    scripts/macos_libraries.py bundle --root Euphoria.app \\
        --executable Euphoria.app/Contents/MacOS/Euphoria \\
        --libraries Euphoria.app/Contents/Frameworks \\
        --run-path @executable_path/../Frameworks
    scripts/macos_libraries.py verify --root Euphoria.app \\
        --executable Euphoria.app/Contents/MacOS/Euphoria

`bundle` follows every library the executable loads, and every library those
load, resolving each install name the way dyld does: `@executable_path` from
the main executable, `@loader_path` from the binary that refers to it, and
`@rpath` through the run paths of that binary and of every binary that loaded
it. A library outside the system (`/usr/lib`, `/System`) and outside `--root`
is copied into `--libraries` with the install name `@rpath/<file>`, and the
reference to it is changed to match. A reference that does not resolve to an
existing file is an error. Afterwards, run paths that lead outside `--root`
are removed and the executable gets `--run-path`.

`verify` checks the result without trusting how it was made: every Mach-O
file under `--root` is loaded the way dyld would load it, starting from the
executable, and each of its libraries must be a system library or a file
under `--root`, and each of its run paths must lead under `--root`.

Changing a file breaks its code signature, so signing is left to the caller.
"""

import argparse
import os
import pathlib
import re
import shutil
import subprocess
import sys

SYSTEM_PREFIXES = ("/usr/lib/", "/System/")
LOAD_COMMANDS = {
    "LC_LOAD_DYLIB",
    "LC_LOAD_WEAK_DYLIB",
    "LC_REEXPORT_DYLIB",
    "LC_LAZY_LOAD_DYLIB",
    "LC_LOAD_UPWARD_DYLIB",
}
WEAK_COMMANDS = {"LC_LOAD_WEAK_DYLIB"}
MACH_O_MAGICS = {
    bytes.fromhex("cffaedfe"),
    bytes.fromhex("cefaedfe"),
    bytes.fromhex("feedfacf"),
    bytes.fromhex("feedface"),
    bytes.fromhex("cafebabe"),
    bytes.fromhex("bebafeca"),
}
VALUE = re.compile(r"^\s*(?:name|path) (?P<value>.+?) \(offset \d+\)$")


class LibraryError(Exception):
    """A library that cannot be found or bundled."""


def is_system(path):
    return path.startswith(SYSTEM_PREFIXES)


def is_mach_o(path):
    try:
        with open(path, "rb") as file:
            return file.read(4) in MACH_O_MAGICS
    except OSError:
        return False


class MachO:
    """What dyld reads from a Mach-O file: its install name, the libraries it
    loads and its run paths."""

    def __init__(self, path):
        self.path = pathlib.Path(path)
        listing = subprocess.run(
            ["otool", "-l", str(self.path)], capture_output=True, text=True, check=True
        ).stdout
        self.install_name = None
        self.dependencies = []
        self.run_paths = []
        command = None
        for line in listing.splitlines():
            stripped = line.strip()
            if stripped.startswith("cmd "):
                command = stripped.split()[1]
                continue
            match = VALUE.match(line)
            if not match or command is None:
                continue
            value = match["value"]
            if command == "LC_ID_DYLIB":
                self.install_name = value
            elif command in LOAD_COMMANDS:
                entry = (value, command in WEAK_COMMANDS)
                if entry not in self.dependencies:
                    self.dependencies.append(entry)
            elif command == "LC_RPATH" and value not in self.run_paths:
                self.run_paths.append(value)
            command = None


def expand(path, executable, loader):
    """`path` with `@executable_path` and `@loader_path` replaced, where
    `loader` is the file whose load command holds `path`."""
    if path.startswith("@executable_path/"):
        return str(executable.parent / path[len("@executable_path/") :])
    if path.startswith("@loader_path/"):
        return str(loader.parent / path[len("@loader_path/") :])
    return path


def resolve(install_name, chain, executable):
    """The file dyld loads for `install_name`, referred to by the last binary
    of `chain`, or None. `chain` lists, from the executable down, the binaries
    that led to the referring one, as (path to resolve from, run paths)."""
    referrer, _ = chain[-1]
    if install_name.startswith("@rpath/"):
        rest = install_name[len("@rpath/") :]
        # dyld searches the referring binary's run paths, then those of each
        # binary that loaded it, back to the executable.
        for loader, run_paths in reversed(chain):
            for run_path in run_paths:
                directory = expand(run_path, executable, loader)
                if directory.startswith("@"):
                    continue
                candidate = pathlib.Path(directory) / rest
                if candidate.exists():
                    return candidate.resolve()
        return None
    candidate = pathlib.Path(expand(install_name, executable, referrer))
    if candidate.is_absolute() and candidate.exists():
        return candidate.resolve()
    return None


def inside(path, root):
    try:
        pathlib.Path(path).resolve().relative_to(root)
        return True
    except ValueError:
        return False


def run(*command):
    subprocess.run([str(part) for part in command], check=True, capture_output=True)


def bundle(root, executable, libraries, run_path):
    root = root.resolve()
    executable = executable.resolve()
    libraries.mkdir(parents=True, exist_ok=True)
    libraries = libraries.resolve()
    # Where each copied library came from, by the name of its copy.
    copied_from = {}
    copy_names = {}
    # Each entry: the file to change, the file whose location its relative
    # install names are resolved from, and the chain that loaded it.
    pending = [(executable, executable, [])]
    visited = set()
    while pending:
        editable, original, chain = pending.pop()
        key = (str(editable), tuple(str(path) for path, _ in chain))
        if key in visited or len(chain) > 64:
            continue
        visited.add(key)
        binary = MachO(original)
        here = [*chain, (original, binary.run_paths)]
        for install_name, is_weak in binary.dependencies:
            if is_system(install_name):
                continue
            target = resolve(install_name, here, executable)
            if target is None:
                if is_weak:
                    continue
                raise LibraryError(
                    f"{original} needs {install_name}, which does not resolve to a file"
                )
            if is_system(str(target)):
                continue
            if inside(target, root):
                pending.append((target, target, here))
                continue
            # The copy is named as the reference names it, such as
            # libkeyfinder.2.dylib rather than the file that link leads to,
            # and a file reached under two names is copied once.
            name = copy_names.setdefault(target, pathlib.PurePath(install_name).name)
            copy = libraries / name
            if name in copied_from and copied_from[name] != target:
                raise LibraryError(
                    f"two different libraries are named {name}: {copied_from[name]} and {target}"
                )
            if name not in copied_from:
                shutil.copyfile(target, copy)
                copy.chmod(0o755)
                run("install_name_tool", "-id", f"@rpath/{name}", copy)
                copied_from[name] = target
            run(
                "install_name_tool", "-change", install_name, f"@rpath/{name}", editable
            )
            # The copy's own references still name places next to the original.
            pending.append((copy, target, here))

    for path in mach_o_files(root):
        for existing in MachO(path).run_paths:
            directory = expand(existing, executable, path)
            if directory.startswith("@") or not inside(directory, root):
                run("install_name_tool", "-delete_rpath", existing, path)
    if run_path not in MachO(executable).run_paths:
        run("install_name_tool", "-add_rpath", run_path, executable)


def mach_o_files(root):
    return sorted(
        path
        for path in root.rglob("*")
        if path.is_file() and not path.is_symlink() and is_mach_o(path)
    )


def verify(root, executable):
    """Every problem with the libraries under `root`, as text. Empty when
    every library resolves to a system library or a file under `root`."""
    root = root.resolve()
    executable = executable.resolve()
    problems = []
    reached = set()
    seen = set()

    def walk(start, chain):
        pending = [(start, chain)]
        while pending:
            path, loaders = pending.pop()
            key = (str(path), tuple(str(loader) for loader, _ in loaders))
            if key in seen or len(loaders) > 64:
                continue
            seen.add(key)
            reached.add(path)
            binary = MachO(path)
            for run_path in binary.run_paths:
                directory = expand(run_path, executable, path)
                if directory.startswith("@") or not inside(directory, root):
                    problems.append(
                        f"{path} has the run path {run_path}, outside {root}"
                    )
            here = [*loaders, (path, binary.run_paths)]
            for install_name, is_weak in binary.dependencies:
                if is_system(install_name):
                    continue
                target = resolve(install_name, here, executable)
                if target is None:
                    if not is_weak:
                        problems.append(
                            f"{path} needs {install_name}, which does not resolve"
                        )
                    continue
                if is_system(str(target)):
                    continue
                if not inside(target, root):
                    problems.append(f"{path} loads {target} from outside {root}")
                    continue
                pending.append((target, here))

    walk(executable, [])
    # A library nothing loads is checked as if the executable loaded it.
    executable_run_paths = MachO(executable).run_paths
    for path in mach_o_files(root):
        if path not in reached:
            walk(path, [(executable, executable_run_paths)])
    return list(dict.fromkeys(problems))


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("bundle", "verify"):
        command = commands.add_parser(name)
        command.add_argument("--root", required=True, type=pathlib.Path)
        command.add_argument("--executable", required=True, type=pathlib.Path)
        if name == "bundle":
            command.add_argument("--libraries", required=True, type=pathlib.Path)
            command.add_argument("--run-path", required=True)
    arguments = parser.parse_args()
    try:
        if arguments.command == "bundle":
            bundle(
                arguments.root,
                arguments.executable,
                arguments.libraries,
                arguments.run_path,
            )
        problems = verify(arguments.root, arguments.executable)
    except (LibraryError, subprocess.CalledProcessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    for problem in problems:
        print(f"error: {problem}", file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    os.umask(0o022)
    sys.exit(main())
