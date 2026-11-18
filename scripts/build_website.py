# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Builds the website in website/ into a folder ready to serve.

    python3 scripts/build_website.py
    python3 scripts/build_website.py --output _site

The pages and their assets are copied as they are. Every image the pages
name as images/... is copied from docs/images, so the screenshots and
figures have one source shared with the documentation, and favicon.svg comes
from assets/logo. The build fails, naming each one, when a page names an
image or a page that does not exist. The default output is target/website.
"""

import argparse
import re
import shutil
import sys
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parent.parent
SOURCE = REPOSITORY / "website"
IMAGES = REPOSITORY / "docs" / "images"
FAVICON = REPOSITORY / "assets" / "logo" / "favicon.svg"
DEFAULT_OUTPUT = REPOSITORY / "target" / "website"

# Files in website/ that describe or check the site rather than serve it.
NOT_PUBLISHED = ("README.md", "tsconfig.json")
TEXT_SUFFIXES = {".html", ".css", ".js"}
IMAGE_REFERENCE = re.compile(r"images/[A-Za-z0-9_./-]+\.(?:png|svg|jpg|webp)")
PAGE_REFERENCE = re.compile(r"""href="([A-Za-z0-9_-]+\.html)(?:#[^"]*)?\"""")


def references(source):
    """The images and pages named by the text files of `source`."""
    images = set()
    pages = set()
    for path in sorted(source.rglob("*")):
        if path.suffix not in TEXT_SUFFIXES:
            continue
        text = path.read_text(encoding="utf-8")
        images.update(IMAGE_REFERENCE.findall(text))
        pages.update(PAGE_REFERENCE.findall(text))
    return images, pages


def build(source, images_folder, favicon, output):
    """Copies the site into `output` and returns the paths it names that do
    not exist, which is empty when the build is complete."""
    images, pages = references(source)
    missing = [page for page in sorted(pages) if not (source / page).is_file()]
    missing += [image for image in sorted(images) if not (images_folder / image.removeprefix("images/")).is_file()]
    if not favicon.is_file():
        missing.append(str(favicon))
    if missing:
        return missing

    if output.exists():
        shutil.rmtree(output)
    shutil.copytree(source, output, ignore=shutil.ignore_patterns(*NOT_PUBLISHED))
    for image in sorted(images):
        destination = output / image
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(images_folder / image.removeprefix("images/"), destination)
    shutil.copy2(favicon, output / "favicon.svg")
    return []


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    arguments = parser.parse_args()
    output = arguments.output.resolve()
    missing = build(SOURCE, IMAGES, FAVICON, output)
    if missing:
        for path in missing:
            print(f"missing: {path}", file=sys.stderr)
        return 1
    print(f"built {output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
