# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Collect contract callers and Python resource sites without executing PR code."""

import argparse
import ast
import json
from pathlib import Path
import re
import subprocess
import tomllib


def git(root, *arguments):
    return subprocess.check_output(["git", "-C", str(root), *arguments])


def manifest(root, revision, path):
    try:
        source = git(root, "show", f"{revision}:{path}")
    except subprocess.CalledProcessError:
        return {}
    return tomllib.loads(source.decode())


def collect(root, base, head):
    merge_base = git(root, "merge-base", base, head).decode().strip()
    # Compare a moved manifest with its original path, not a nonexistent
    # merge-base file at the destination. NUL records preserve Unicode and
    # whitespace in both names.
    records = iter(git(root, "diff", "--name-status", "--find-renames", "-z",
                       f"{base}...{head}").split(b"\0")[:-1])
    previous_paths = {}
    for status in records:
        before = next(records).decode()
        after = next(records).decode() if status[:1] in {b"R", b"C"} else before
        previous_paths[after] = before
    changed = list(previous_paths)
    contracts = []
    for path in changed:
        if not path.endswith("Cargo.toml"):
            continue
        before = manifest(root, merge_base, previous_paths[path])
        after = manifest(root, head, path)
        old = before.get("package", {}).get("name")
        new = after.get("package", {}).get("name")
        if old and new and old != new:
            contracts.append({"kind": "package", "path": path, "old": old, "new": new})

    patterns = [(contract, re.compile(
        r"""(?<![\w-])(?:--package[\s"',\[\]=]+|-p[\s"',\[\]=]*)["']?"""
        + re.escape(contract["old"]) + r"(?![\w-])"
    )) for contract in contracts]
    callers = []
    packages = []
    resources = []
    for name in git(root, "ls-tree", "-r", "--name-only", "-z", head).split(b"\0"):
        if not name:
            continue
        path = name.decode()
        source = git(root, "show", f"{head}:{path}")
        if b"\0" in source:
            continue
        text = source.decode("utf-8", errors="replace")
        if path in changed and path.endswith(".py"):
            try:
                tree = ast.parse(text)
            except SyntaxError:
                tree = None
            if tree is not None:
                lines = text.splitlines()
                for node in ast.walk(tree):
                    if not isinstance(node, ast.Call):
                        continue
                    function = ast.unparse(node.func)
                    if function.rsplit(".", 1)[-1] in {
                        "TemporaryDirectory", "mkdtemp", "Popen", "run",
                        "signal", "send_signal", "wait", "kill", "terminate",
                        "execv", "rename", "replace", "unlink", "rmtree",
                        "copyfile", "copy2", "write_text", "write_bytes",
                    }:
                        resources.append({"path": path, "line": node.lineno,
                                          "call": function,
                                          "source": lines[node.lineno - 1]})
        if path.endswith("Cargo.toml"):
            try:
                parsed = tomllib.loads(text)
            except tomllib.TOMLDecodeError:
                parsed = {}
            package = parsed.get("package", {}).get("name")
            if package:
                packages.append({"path": path, "name": package,
                                 "explicit_binaries": [item.get("name") for item in parsed.get("bin", [])]})
        for contract, pattern in patterns:
            for match in pattern.finditer(text):
                line = text.count("\n", 0, match.start()) + 1
                callers.append({"path": path, "line": line, "changed": path in changed,
                                "contract": contract, "reference": match.group()})
    return {"base": base, "head": head, "merge_base": merge_base, "changed_files": changed,
            "renamed_contracts": contracts, "head_packages": packages,
            "affected_callers": callers,
            "resource_sites": sorted(resources, key=lambda item: (item["path"], item["line"]))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    args = parser.parse_args()
    print(json.dumps(collect(args.root, args.base, args.head), indent=2))


if __name__ == "__main__":
    main()
