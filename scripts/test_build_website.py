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
            self.assertFalse((output / "README.md").exists())

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


if __name__ == "__main__":
    unittest.main()
