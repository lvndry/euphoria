# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Checks vendor updates without network requests or changing the checkout."""

import contextlib
import importlib.util
import io
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "vendor_signalsmith", Path(__file__).with_name("vendor_signalsmith.py")
)
VENDOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VENDOR)


class VendorUpdateTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.root = Path(self.folder.name) / "vendor"
        self.root.mkdir()
        (self.root / "keep").write_bytes(b"old source")
        (self.root / "VENDORED.md").write_bytes(b"old manifest")
        self.enterContext(patch.object(VENDOR, "VENDOR", self.root))
        self.enterContext(patch.object(VENDOR, "RECORD", self.root / "VENDORED.md"))
        self.enterContext(contextlib.redirect_stdout(io.StringIO()))
        self.downloads = []
        self.enterContext(patch.object(VENDOR, "extract", self.extract))

    def extract(self, repository, reference, destination):
        self.downloads.append((repository, reference))
        destination.mkdir(parents=True)
        (destination / "LICENSE.txt").write_text("upstream license")
        if repository == VENDOR.STRETCH:
            (destination / "signalsmith-stretch.h").write_text("stretch source")
            (destination / "CMakeLists.txt").write_text(
                "GIT_REPOSITORY https://github.com/Signalsmith-Audio/linear.git\n"
                "GIT_TAG 0.6.4\n"
            )
        else:
            (destination / "linear.h").write_text("linear source")

    def test_metadata_failure_preserves_the_existing_sources_and_manifest(self):
        for failed_repository in (VENDOR.STRETCH, VENDOR.LINEAR):
            with self.subTest(repository=failed_repository):
                def lookup(repository, _tag):
                    if repository == failed_repository:
                        raise OSError("GitHub API rate limit")
                    return "a" * 40

                with patch.object(VENDOR, "commit_of", lookup):
                    with self.assertRaises(OSError):
                        VENDOR.update("1.4.0")
                self.assertEqual((self.root / "keep").read_bytes(), b"old source")
                self.assertEqual(VENDOR.RECORD.read_bytes(), b"old manifest")

    def test_downloads_are_pinned_to_the_commits_written_in_the_manifest(self):
        commits = {VENDOR.STRETCH: "a" * 40, VENDOR.LINEAR: "b" * 40}
        with patch.object(VENDOR, "commit_of", lambda repository, _tag: commits[repository]):
            VENDOR.update("1.4.0")
        self.assertEqual(self.downloads, list(commits.items()))
        for commit in commits.values():
            self.assertIn(commit, VENDOR.RECORD.read_text())
        VENDOR.check()


if __name__ == "__main__":
    unittest.main()
