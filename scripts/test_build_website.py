"""Checks that the website builds completely and that a broken reference fails the build."""

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import build_website  # noqa: E402


class BuildWebsiteTest(unittest.TestCase):
    def test_the_website_builds_with_every_image_it_names(self):
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder) / "site"
            missing = build_website.build(
                build_website.SOURCE, build_website.IMAGES, build_website.FAVICON, output
            )
            self.assertEqual(missing, [])
            for page in ["index.html", "features.html", "mixing.html", "favicon.svg"]:
                self.assertTrue((output / page).is_file(), page)
            images, _ = build_website.references(build_website.SOURCE)
            self.assertTrue(images)
            for image in images:
                self.assertTrue((output / image).is_file(), image)
            for page in ["features.html", "mixing.html"]:
                self.assertIn(page, build_website.references(build_website.SOURCE)[1])
            for unpublished in build_website.NOT_PUBLISHED:
                self.assertFalse((output / unpublished).exists(), unpublished)

    def test_a_missing_image_or_page_fails_the_build_and_writes_nothing(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / "website"
            source.mkdir()
            (source / "index.html").write_text(
                '<img src="images/gone/shot.png"><a href="nowhere.html#top">x</a>', encoding="utf-8"
            )
            images = root / "images"
            images.mkdir()
            favicon = root / "favicon.svg"
            favicon.write_text("<svg/>", encoding="utf-8")
            output = root / "site"
            missing = build_website.build(source, images, favicon, output)
            self.assertEqual(missing, ["nowhere.html", "images/gone/shot.png"])
            self.assertFalse(output.exists())


class OutputSafetyTest(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        root = Path(self.folder.name)
        self.repository = root / "repository"
        self.source = self.repository / "website"
        self.source.mkdir(parents=True)
        (self.source / "index.html").write_text("<p>site</p>", encoding="utf-8")
        self.images = self.repository / "docs" / "images"
        self.images.mkdir(parents=True)
        self.favicon = self.repository / "favicon.svg"
        self.favicon.write_text("<svg/>", encoding="utf-8")
        self.sentinel = self.repository / "keep.txt"
        self.sentinel.write_text("untouched", encoding="utf-8")

    def tearDown(self):
        self.folder.cleanup()

    def build(self, output):
        return build_website.build(self.source, self.images, self.favicon, output)

    def assert_refused_and_untouched(self, output):
        with self.assertRaises(build_website.UnsafeOutput):
            self.build(output)
        self.assertEqual(self.sentinel.read_text(encoding="utf-8"), "untouched")
        self.assertTrue((self.source / "index.html").is_file())

    def test_the_source_folder_is_refused(self):
        self.assert_refused_and_untouched(self.source)

    def test_a_folder_containing_the_inputs_is_refused(self):
        self.assert_refused_and_untouched(self.repository)

    def test_a_folder_inside_the_source_is_refused(self):
        self.assert_refused_and_untouched(self.source / "built")

    def test_a_folder_the_build_did_not_make_is_refused(self):
        other = Path(self.folder.name) / "documents"
        other.mkdir()
        (other / "notes.txt").write_text("mine", encoding="utf-8")
        with self.assertRaises(build_website.UnsafeOutput):
            self.build(other)
        self.assertEqual((other / "notes.txt").read_text(encoding="utf-8"), "mine")

    def test_its_own_earlier_build_is_replaced(self):
        output = Path(self.folder.name) / "site"
        self.assertEqual(self.build(output), [])
        (output / "stale.html").write_text("old", encoding="utf-8")
        self.assertEqual(self.build(output), [])
        self.assertFalse((output / "stale.html").exists())
        self.assertTrue((output / build_website.BUILD_MARKER).is_file())


if __name__ == "__main__":
    unittest.main()
