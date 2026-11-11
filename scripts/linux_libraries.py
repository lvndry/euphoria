#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Makes a Linux executable carry every library it needs, and proves it does.

    scripts/linux_libraries.py bundle --root Euphoria.AppDir \\
        --executable Euphoria.AppDir/usr/bin/euphoria-desktop \\
        --libraries Euphoria.AppDir/usr/lib --run-path '$ORIGIN/../lib'
    scripts/linux_libraries.py verify --root Euphoria.AppDir \\
        --executable Euphoria.AppDir/usr/bin/euphoria-desktop
    scripts/linux_libraries.py minimum-glibc --root Euphoria.AppDir

`bundle` follows every library the executable needs (DT_NEEDED), and every
library those need, resolving each name the way the glibc dynamic loader
does: through the DT_RUNPATH of the binary that needs it, or through the
DT_RPATH of that binary and of every binary that loaded it when it has no
DT_RUNPATH, then through the loader's cache and its default folders. A
library of the base system, one that every Linux desktop has and that must
come from the system (BASE_SYSTEM_LIBRARIES), is left out. Every other
library outside `--root` is copied into `--libraries` and given the run path
`$ORIGIN`, and the executable gets `--run-path`, so each copy is found next
to the binary that needs it wherever the folder is unpacked. A library that
cannot be found is an error. The executable must be under `--root`, and
packaged files to be edited must be separate copies, not hard links.

`verify` checks the result without trusting how it was made: every ELF file
under `--root` is loaded the way the dynamic loader would load it, starting
from the executable, with no cache and no default folders, and each library
it needs must be a base system library or a file under `--root` reached
through a run path that leads under `--root`.

`minimum-glibc` prints the newest glibc version any ELF file under any
`--root` asks for through its symbol versions (GLIBC_2.34 and the like): the
oldest glibc the files run on.

The ELF files are read here, without readelf, so that the check depends only
on Python. Changing run paths needs patchelf.
"""

import argparse
import os
import pathlib
import re
import shutil
import struct
import subprocess
import sys

ELF_MAGIC = b"\x7fELF"
PT_LOAD = 1
PT_DYNAMIC = 2
DT_NULL = 0
DT_NEEDED = 1
DT_STRTAB = 5
DT_SONAME = 14
DT_RPATH = 15
DT_RUNPATH = 29
DT_VERNEED = 0x6FFFFFFE
DT_VERNEEDNUM = 0x6FFFFFFF
ET_EXEC = 2
ET_DYN = 3

# Libraries that a Linux desktop always has and that must not be bundled:
# glibc and the C++ runtime, which have to match the loader and each other,
# and the libraries that talk to the display server, the graphics driver and
# the sound server of the machine. It follows the AppImage project's list of
# libraries that are never bundled, plus the keyboard map libraries that the
# X11 and Wayland back ends of the window toolkit link against. Vulkan, the
# Wayland client library and the GPU drivers are loaded at run time by name,
# so they never appear as needed libraries.
BASE_SYSTEM_LIBRARIES = frozenset({
    "ld-linux.so.2", "ld-linux-x86-64.so.2", "ld-linux-aarch64.so.1",
    "libanl.so.1", "libBrokenLocale.so.1", "libc.so.6", "libdl.so.2",
    "libm.so.6", "libmvec.so.1", "libpthread.so.0", "libresolv.so.2",
    "librt.so.1", "libthread_db.so.1", "libutil.so.1",
    "libnss_compat.so.2", "libnss_dns.so.2", "libnss_files.so.2",
    "libgcc_s.so.1", "libstdc++.so.6",
    "libGL.so.1", "libEGL.so.1", "libGLdispatch.so.0", "libGLX.so.0",
    "libOpenGL.so.0", "libdrm.so.2", "libgbm.so.1",
    "libxcb.so.1", "libX11.so.6", "libX11-xcb.so.1", "libwayland-client.so.0",
    "libxkbcommon.so.0", "libxkbcommon-x11.so.0",
    "libasound.so.2", "libfontconfig.so.1", "libfreetype.so.6",
    "libharfbuzz.so.0", "libexpat.so.1", "libz.so.1", "libuuid.so.1",
})  # fmt: skip
GLIBC_VERSION = re.compile(r"^GLIBC_(?P<version>[0-9]+(?:\.[0-9]+)+)$")
# A guard against a walk that never ends, which only a damaged or hostile
# file could cause: the walk stops at each library already loaded higher up
# the chain, so real libraries need a few dozen steps.
MOST_WALK_STEPS = 100_000


class LibraryError(Exception):
    """A library that cannot be found, read or bundled."""


def count_step(steps):
    steps[0] += 1
    if steps[0] > MOST_WALK_STEPS:
        raise LibraryError(
            f"following the libraries took more than {MOST_WALK_STEPS} steps; "
            "a library file is probably damaged"
        )


def is_elf(path):
    try:
        with open(path, "rb") as file:
            return file.read(4) == ELF_MAGIC
    except OSError:
        return False


class Elf:
    """What the dynamic loader reads from an ELF file: its class and machine,
    the libraries it needs, its run paths and the symbol versions it asks
    each library for."""

    def __init__(self, path):
        self.path = pathlib.Path(path)
        data = self.path.read_bytes()
        if data[:4] != ELF_MAGIC or len(data) < 16:
            raise LibraryError(f"{self.path} is not an ELF file")
        elf_class = data[4]
        byte_order = {1: "<", 2: ">"}.get(data[5])
        if elf_class not in (1, 2) or byte_order is None:
            raise LibraryError(f"{self.path} has an unknown ELF class or byte order")
        self.is_64_bit = elf_class == 2
        self.byte_order = byte_order
        self.data = data
        if self.is_64_bit:
            header = self.unpack("HHIQQQIHHHHHH", 16)
        else:
            header = self.unpack("HHIIIIIHHHHHH", 16)
        (self.file_type, self.machine, _, _, program_offset, _, _, _,
         program_entry_size, program_count, _, _, _) = header  # fmt: skip
        self.identity = (elf_class, data[5], self.machine)
        self.needed = []
        self.soname = None
        self.run_paths = None
        self.rpaths = None
        self.version_needs = []
        loads = []
        dynamic = None
        for index in range(program_count):
            offset = program_offset + index * program_entry_size
            if self.is_64_bit:
                (kind, _, file_offset, address, _, file_size, _, _) = self.unpack(
                    "IIQQQQQQ", offset
                )
            else:
                (kind, file_offset, address, _, file_size, _, _, _) = self.unpack(
                    "IIIIIIII", offset
                )
            if kind == PT_LOAD:
                loads.append((address, file_offset, file_size))
            elif kind == PT_DYNAMIC:
                dynamic = (file_offset, file_size)
        if dynamic is None:
            return
        self.loads = loads
        entries = self.dynamic_entries(*dynamic)
        string_table = next((value for tag, value in entries if tag == DT_STRTAB), None)
        if string_table is None:
            raise LibraryError(f"{self.path} has a dynamic section without strings")
        string_offset = self.file_offset(string_table)
        for tag, value in entries:
            if tag == DT_NEEDED:
                name = self.string(string_offset, value)
                if name not in self.needed:
                    self.needed.append(name)
            elif tag == DT_SONAME:
                self.soname = self.string(string_offset, value)
            elif tag == DT_RUNPATH:
                self.run_paths = split_paths(self.string(string_offset, value))
            elif tag == DT_RPATH:
                self.rpaths = split_paths(self.string(string_offset, value))
        need_address = next((value for tag, value in entries if tag == DT_VERNEED), None)
        need_count = next((value for tag, value in entries if tag == DT_VERNEEDNUM), 0)
        if need_address is not None:
            self.read_version_needs(self.file_offset(need_address), need_count, string_offset)

    def unpack(self, layout, offset):
        layout = self.byte_order + layout
        end = offset + struct.calcsize(layout)
        if offset < 0 or end > len(self.data):
            raise LibraryError(f"{self.path} is cut short")
        return struct.unpack_from(layout, self.data, offset)

    def dynamic_entries(self, offset, size):
        layout = "qQ" if self.is_64_bit else "iI"
        entry_size = struct.calcsize(layout)
        entries = []
        for position in range(offset, offset + size - entry_size + 1, entry_size):
            tag, value = self.unpack(layout, position)
            if tag == DT_NULL:
                break
            entries.append((tag, value))
        return entries

    def file_offset(self, address):
        """Where in the file the loaded address `address` comes from."""
        for start, file_offset, size in self.loads:
            if start <= address < start + size:
                return file_offset + address - start
        raise LibraryError(f"{self.path} refers to {address:#x}, outside its file")

    def string(self, table_offset, index):
        start = table_offset + index
        end = self.data.find(b"\0", start)
        if start >= len(self.data) or end < 0:
            raise LibraryError(f"{self.path} names a string outside its file")
        return self.data[start:end].decode("utf-8", errors="surrogateescape")

    def read_version_needs(self, offset, count, string_offset):
        for _ in range(count):
            _, aux_count, file_name, aux_offset, next_offset = self.unpack("HHIII", offset)
            library = self.string(string_offset, file_name)
            position = offset + aux_offset
            for _ in range(aux_count):
                _, _, _, name, next_aux = self.unpack("IHHII", position)
                self.version_needs.append((library, self.string(string_offset, name)))
                if next_aux == 0:
                    break
                position += next_aux
            if next_offset == 0:
                break
            offset += next_offset


def split_paths(value):
    return [path for path in value.split(":") if path]


def expand(path, loader):
    """`path` with $ORIGIN, or ${ORIGIN}, replaced by the folder of `loader`,
    the binary whose run path it is. Other substitutions ($LIB, $PLATFORM)
    depend on the machine, so they are an error."""
    origin = str(pathlib.Path(loader).parent)
    expanded = path.replace("${ORIGIN}", origin).replace("$ORIGIN", origin)
    if "$" in expanded:
        raise LibraryError(f"{loader} has the run path {path}, which cannot be expanded")
    return expanded


def search_folders(chain):
    """The run path folders the loader searches for a library needed by the
    last binary of `chain`, a list of (path, Elf) from the executable down.
    DT_RUNPATH, when present, is the only one used and covers that binary's
    own needs. Otherwise DT_RPATH is searched, of that binary and then of
    each binary that loaded it."""
    path, binary = chain[-1]
    if binary.run_paths is not None:
        return [expand(folder, path) for folder in binary.run_paths]
    folders = []
    for loader, loaded in reversed(chain):
        if loaded.run_paths is None and loaded.rpaths:
            folders.extend(expand(folder, loader) for folder in loaded.rpaths)
    return folders


def is_loadable(path, identity):
    """Whether the loader would load `path` for a binary of `identity`: an
    ELF file of the same class, byte order and machine. Any other file of
    the same name does not stop the search."""
    if not path.is_file() or not is_elf(path):
        return False
    try:
        return Elf(path).identity == identity
    except LibraryError:
        return False


def system_folders():
    """The libraries the loader's cache knows, by name, then its default
    folders, which it searches after the cache."""
    cached = {}
    ldconfig = shutil.which("ldconfig") or "/sbin/ldconfig"
    try:
        listing = subprocess.run(
            [ldconfig, "-p"], capture_output=True, text=True, check=True
        ).stdout
    except (OSError, subprocess.CalledProcessError):
        listing = ""
    for line in listing.splitlines()[1:]:
        name, _, target = line.strip().partition(" => ")
        if target:
            cached.setdefault(name.split(" ", 1)[0], []).append(target)
    multiarch = [path.name for path in sorted(pathlib.Path("/usr/lib").glob("*-linux-gnu*"))]
    defaults = [f"{prefix}/{name}" for name in multiarch for prefix in ("/lib", "/usr/lib")]
    defaults += ["/lib64", "/usr/lib64", "/lib", "/usr/lib"]
    return cached, defaults


def resolve(name, chain, system=None):
    """The file the loader loads for the needed library `name` of the last
    binary of `chain`, or None. With `system`, the loader's cache and its
    default folders are searched after the run paths."""
    loader, binary = chain[-1]
    if "/" in name:
        candidate = pathlib.Path(expand(name, loader))
        return candidate.resolve() if is_loadable(candidate, binary.identity) else None
    for folder in search_folders(chain):
        if not os.path.isabs(folder):
            continue
        candidate = pathlib.Path(folder) / name
        if is_loadable(candidate, binary.identity):
            return candidate.resolve()
    if system is None:
        return None
    cached, defaults = system
    for target in cached.get(name, []):
        candidate = pathlib.Path(target)
        if is_loadable(candidate, binary.identity):
            return candidate.resolve()
    for folder in defaults:
        candidate = pathlib.Path(folder) / name
        if is_loadable(candidate, binary.identity):
            return candidate.resolve()
    return None


def inside(path, root):
    try:
        pathlib.Path(path).resolve().relative_to(root)
        return True
    except ValueError:
        return False


def is_loaded_higher_up(target, chain):
    return any(pathlib.Path(path).resolve() == target for path, _ in chain)


def require_private_inode(path):
    if path.stat().st_nlink > 1:
        raise LibraryError(f"{path} is hard-linked; make a separate copy before bundling")


def set_run_path(path, run_path):
    """Gives `path` the DT_RUNPATH `run_path`, in place of any DT_RPATH."""
    require_private_inode(path)
    subprocess.run(
        ["patchelf", "--remove-rpath", str(path)], check=True, capture_output=True
    )
    subprocess.run(
        ["patchelf", "--set-rpath", run_path, str(path)],
        check=True,
        capture_output=True,
    )


def bundle(root, executable, libraries, run_path):
    root = root.resolve()
    executable = executable.resolve()
    if not inside(executable, root):
        raise LibraryError(f"{executable} is not under {root}")
    require_private_inode(executable)
    libraries.mkdir(parents=True, exist_ok=True)
    libraries = libraries.resolve()
    if not inside(libraries, root):
        raise LibraryError(f"{libraries} is not under {root}")
    system = system_folders()
    copied_from = {}
    packaged = {executable}
    pending = [(executable, [])]
    visited = set()
    steps = [0]
    while pending:
        path, chain = pending.pop()
        key = (str(path), tuple(str(loader) for loader, _ in chain))
        if key in visited:
            continue
        visited.add(key)
        count_step(steps)
        here = [*chain, (path, Elf(path))]
        for name in here[-1][1].needed:
            if pathlib.PurePath(name).name in BASE_SYSTEM_LIBRARIES:
                continue
            target = resolve(name, here, system)
            if target is None:
                raise LibraryError(f"{path} needs {name}, which was not found")
            if is_loaded_higher_up(target, here):
                continue
            if inside(target, root):
                packaged.add(target)
                pending.append((target, here))
                continue
            copy = libraries / pathlib.PurePath(name).name
            if copy.name in copied_from and copied_from[copy.name] != target:
                raise LibraryError(
                    f"two different libraries are named {copy.name}: "
                    f"{copied_from[copy.name]} and {target}"
                )
            if copy.name not in copied_from:
                # Replacing an entry must not follow its symlink or write
                # through a hard link into a file outside the package.
                copy.unlink(missing_ok=True)
                with target.open("rb") as source, copy.open("xb") as destination:
                    shutil.copyfileobj(source, destination)
                copy.chmod(0o755)
                set_run_path(copy, "$ORIGIN")
                copied_from[copy.name] = target
            packaged.add(copy)
            # The original is followed, since its run paths still lead to
            # the libraries it needs, which the copy finds once they are
            # copied beside it.
            pending.append((target, here))
    for path in packaged:
        require_private_inode(path)
    # Path-valued DT_NEEDED ignores run paths. Change only packaged files;
    # the originals above must retain their paths while their graph is read.
    for path in sorted(packaged):
        for name in Elf(path).needed:
            if "/" in name:
                subprocess.run(
                    ["patchelf", "--replace-needed", name, pathlib.PurePath(name).name, str(path)],
                    check=True, capture_output=True,
                )
    set_run_path(executable, run_path)


def elf_files(root):
    return sorted(
        path
        for path in root.rglob("*")
        if path.is_file() and not path.is_symlink() and is_elf(path)
    )


def verify(root, executable):
    """Every problem with the libraries under `root`, as text. Empty when
    every library resolves to a base system library or a file under
    `root`."""
    root = root.resolve()
    executable = executable.resolve()
    problems = []
    reached = set()
    seen = set()
    steps = [0]

    def walk(start, chain):
        pending = [(start, chain)]
        while pending:
            path, loaders = pending.pop()
            key = (str(path), tuple(str(loader) for loader, _ in loaders))
            if key in seen:
                continue
            seen.add(key)
            count_step(steps)
            reached.add(path)
            binary = Elf(path)
            for run_path in (binary.run_paths or []) + (binary.rpaths or []):
                folder = expand(run_path, path)
                if not (os.path.isabs(folder) and inside(folder, root)):
                    problems.append(f"{path} has the run path {run_path}, outside {root}")
            here = [*loaders, (path, binary)]
            for name in binary.needed:
                if name in BASE_SYSTEM_LIBRARIES:
                    continue
                target = resolve(name, here)
                if target is None:
                    problems.append(f"{path} needs {name}, which is not in {root}")
                    continue
                if is_loaded_higher_up(target, here):
                    continue
                if not inside(target, root):
                    problems.append(f"{path} loads {target} from outside {root}")
                    continue
                pending.append((target, here))

    walk(executable, [])
    for path in elf_files(root):
        if path.resolve() not in reached and Elf(path).file_type in (ET_EXEC, ET_DYN):
            walk(path.resolve(), [])
    return list(dict.fromkeys(problems))


def version_key(version):
    return tuple(int(part) for part in version.split("."))


def minimum_glibc(roots):
    """The newest GLIBC_ symbol version any ELF file under any of `roots`
    needs, or None when none needs one."""
    versions = []
    paths = [path for root in roots for path in elf_files(pathlib.Path(root).resolve())]
    for path in paths:
        for _, name in Elf(path).version_needs:
            match = GLIBC_VERSION.match(name)
            if match:
                versions.append(match["version"])
    return max(versions, key=version_key) if versions else None


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    minimum = commands.add_parser("minimum-glibc")
    minimum.add_argument("--root", required=True, type=pathlib.Path, action="append")
    for name in ("bundle", "verify"):
        command = commands.add_parser(name)
        command.add_argument("--root", required=True, type=pathlib.Path)
        command.add_argument("--executable", required=True, type=pathlib.Path)
        if name == "bundle":
            command.add_argument("--libraries", required=True, type=pathlib.Path)
            command.add_argument("--run-path", required=True)
    arguments = parser.parse_args()
    try:
        if arguments.command == "minimum-glibc":
            version = minimum_glibc(arguments.root)
            if version is None:
                folders = ", ".join(str(root) for root in arguments.root)
                print(f"error: nothing under {folders} needs glibc", file=sys.stderr)
                return 1
            print(version)
            return 0
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
