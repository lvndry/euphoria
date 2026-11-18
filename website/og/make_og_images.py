#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Renders the website's social preview images, one 1200 by 630 PNG per page.

Each image is card.html in this folder, opened for one page in headless Chrome
and captured at its exact size. The pages name them in their og:image and
twitter:image tags. Needs Google Chrome, and the network for the card's font.

    python3 website/og/make_og_images.py

CHROME names the Chrome binary when it is not the usual macOS one.
"""

import os
import subprocess
import sys
from pathlib import Path

FOLDER = Path(__file__).resolve().parent
CARD = FOLDER / "card.html"
PAGES = ["home", "features", "mixing"]
WIDTH = 1200
HEIGHT = 630
DEFAULT_CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
# Time Chrome lets the page load its font and draw before the capture.
SETTLE_MILLISECONDS = 6000


def render(chrome, page, output):
    subprocess.run(
        [
            chrome,
            "--headless=new",
            "--disable-gpu",
            "--hide-scrollbars",
            "--allow-file-access-from-files",
            f"--window-size={WIDTH},{HEIGHT}",
            "--force-device-scale-factor=1",
            f"--virtual-time-budget={SETTLE_MILLISECONDS}",
            f"--screenshot={output}",
            f"{CARD.as_uri()}?page={page}",
        ],
        check=True,
        capture_output=True,
    )


def main():
    chrome = os.environ.get("CHROME", DEFAULT_CHROME)
    for page in PAGES:
        output = FOLDER / f"{page}.png"
        render(chrome, page, output)
        print(f"wrote {output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
