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

The output folder is deleted and made again, so the build refuses one that is,
contains or lies inside its inputs (`--output .` or `--output website`), and
only replaces a folder it made itself, which it marks with BUILD_MARKER.
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
PAGE_REFERENCE = re.compile(r'href="([A-Za-z0-9_-]+\.html)(?:#[^"]*)?"')
# Written into every build, so a later build knows the folder is one it may replace.
BUILD_MARKER = ".euphoria-website-build"


class UnsafeOutput(ValueError):
    """The output folder would delete an input or a folder the build did not make."""


def check_output(output, inputs):
    """Raises UnsafeOutput unless `output` can be deleted and rebuilt without
    touching `inputs` or anything the build did not make."""
    output = output.resolve()
    for path in inputs:
        path = path.resolve()
        if output == path or output in path.parents or path in output.parents:
            raise UnsafeOutput(f"{output} overlaps the input {path}")
    if output.exists() and not output.is_dir():
        raise UnsafeOutput(f"{output} is a file, not a folder")
    if output.is_dir() and any(output.iterdir()) and not (output / BUILD_MARKER).is_file():
        raise UnsafeOutput(f"{output} is not empty and was not made by this build")


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
    not exist, which is empty when the build is complete. Raises UnsafeOutput,
    before deleting anything, when `output` is not safe to replace."""
    check_output(output, [source, images_folder, favicon])
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
    (output / BUILD_MARKER).write_text("Made by scripts/build_website.py\n", encoding="utf-8")
    return []


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    arguments = parser.parse_args()
    output = arguments.output.resolve()
    try:
        missing = build(SOURCE, IMAGES, FAVICON, output)
    except UnsafeOutput as error:
        print(f"refusing to build: {error}", file=sys.stderr)
        return 1
    if missing:
        for path in missing:
            print(f"missing: {path}", file=sys.stderr)
        return 1
    print(f"built {output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
