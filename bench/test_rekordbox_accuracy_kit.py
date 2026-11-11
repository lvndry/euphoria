# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Regression tests: python3 -m unittest discover -s bench -p 'test_rekordbox_accuracy_kit.py'."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("rekordbox_accuracy_kit.py")


class VerificationTests(unittest.TestCase):
    def verify(self, data, kit):
        return subprocess.run(
            [sys.executable, str(SCRIPT), "--data", str(data), "--verify", str(kit)],
            capture_output=True, text=True, check=False,
        )

    def write_previews(self, data, kit):
        for dataset in ("key", "tempo"):
            source = data / dataset / "audio" / "preview.mp3"
            target = kit / dataset / source.name
            source.parent.mkdir(parents=True)
            target.parent.mkdir(parents=True)
            source.write_bytes(b"made-up preview contents")
            target.write_bytes(source.read_bytes())

    def test_missing_or_empty_source_dataset_cannot_certify_a_kit(self):
        for missing in ("both", "key", "tempo"):
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as folder:
                data, kit = Path(folder) / "data", Path(folder) / "kit"
                if missing != "both":
                    self.write_previews(data, kit)
                    (data / missing / "audio" / "preview.mp3").unlink()
                result = self.verify(data, kit)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("no previews in", result.stderr)
                self.assertNotIn("match their source", result.stdout)

    def test_matching_sources_in_both_datasets_are_verified(self):
        with tempfile.TemporaryDirectory() as folder:
            data, kit = Path(folder) / "data", Path(folder) / "kit"
            self.write_previews(data, kit)
            result = self.verify(data, kit)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("All 2 previews match their source byte for byte.", result.stdout)

    def test_changed_or_missing_kit_preview_fails_verification(self):
        for changed in (False, True):
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as folder:
                data, kit = Path(folder) / "data", Path(folder) / "kit"
                self.write_previews(data, kit)
                target = kit / "tempo" / "preview.mp3"
                if changed:
                    target.write_bytes(b"different contents")
                else:
                    target.unlink()
                result = self.verify(data, kit)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("tempo/preview.mp3", result.stdout)
                self.assertIn("1 of 2 previews are missing or differ", result.stdout)


if __name__ == "__main__":
    unittest.main()
