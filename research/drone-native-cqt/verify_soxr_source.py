# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Verify the actual extracted source before the research soxr build."""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path
from typing import Any

PREFIX = "dependencies/soxr-1.1.0/"


def verify_source(root: Path, files: dict[str, Any]) -> None:
    expected = {name: record for name, record in files.items() if name.startswith(PREFIX)}
    if not expected:
        raise ValueError("The pinned soxr source inventory is empty")
    source = root / PREFIX
    if not source.resolve().is_relative_to(root.resolve()):
        raise ValueError("The soxr source escapes the experiment root")
    entries = list(source.rglob("*"))
    for path in entries:
        if path.is_symlink() and (
            not path.resolve(strict=True).is_relative_to(source.resolve())
            or not path.resolve().is_file()
        ):
            raise ValueError("The soxr source contains an escaping or directory symlink")
    actual = {path.relative_to(root).as_posix() for path in entries if path.is_file()}
    if actual != expected.keys():
        raise ValueError(
            f"The soxr source inventory changed: missing={sorted(expected.keys() - actual)}, "
            f"extra={sorted(actual - expected.keys())}"
        )
    for name, record in expected.items():
        raw = (root / name).read_bytes()
        if len(raw) != record["bytes"] or hashlib.sha256(raw).hexdigest() != record["sha256"]:
            raise ValueError(f"The pinned soxr source changed: {name}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: verify_soxr_source.py EXPERIMENT_ROOT")
    inventory = Path(__file__).resolve().parent / "evidence/build-evidence-v2.json"
    try:
        verify_source(Path(sys.argv[1]), json.loads(inventory.read_bytes())["files"])
    except (OSError, ValueError) as error:
        raise SystemExit(str(error)) from error
