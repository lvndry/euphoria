# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""A valid archive must not authorize a changed extracted build input."""

import hashlib
import tempfile
import unittest
from pathlib import Path
from typing import Any

from verify_soxr_source import PREFIX, verify_source


class SoxrSourceTests(unittest.TestCase):
    def fixture(self, root: Path) -> tuple[Path, dict[str, Any]]:
        path = root / PREFIX / "libsoxr/CMakeLists.txt"
        path.parent.mkdir(parents=True)
        raw = b"original pinned build input\n"
        path.write_bytes(raw)
        inventory = {
            f"{PREFIX}libsoxr/CMakeLists.txt": {
                "bytes": len(raw),
                "sha256": hashlib.sha256(raw).hexdigest(),
            }
        }
        return path, inventory

    def test_original_tree_passes_and_same_size_modified_source_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path, inventory = self.fixture(root)
            verify_source(root, inventory)
            path.write_bytes(b"modified pinned build input\n")
            with self.assertRaisesRegex(ValueError, "source changed"):
                verify_source(root, inventory)

    def test_missing_and_extra_build_inputs_fail(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path, inventory = self.fixture(root)
            extra = path.parent / "unexpected.c"
            extra.write_bytes(b"extra source")
            with self.assertRaisesRegex(ValueError, "inventory changed"):
                verify_source(root, inventory)
            extra.unlink()
            path.unlink()
            with self.assertRaisesRegex(ValueError, "inventory changed"):
                verify_source(root, inventory)

    def test_symlink_to_unchanged_source_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path, inventory = self.fixture(root)
            target = root / "outside-source"
            path.rename(target)
            path.symlink_to(target)
            with self.assertRaisesRegex(ValueError, "symlink"):
                verify_source(root, inventory)

    def test_pinned_internal_file_symlink_passes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path, inventory = self.fixture(root)
            alias = path.parent / "internal-alias"
            alias.symlink_to(path.name)
            inventory[f"{PREFIX}libsoxr/internal-alias"] = inventory[
                f"{PREFIX}libsoxr/CMakeLists.txt"
            ]
            verify_source(root, inventory)


if __name__ == "__main__":
    unittest.main()
