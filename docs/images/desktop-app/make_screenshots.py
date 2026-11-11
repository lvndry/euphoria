#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Regenerates the screenshots of the desktop app in this folder.

Builds a small demo library from the GiantSteps Key previews that
bench/download_datasets.sh fetches, openly published for research, sorted
into two level folders by the genre the dataset gives each preview. The
user's own music is never read. The app runs with a scratch data folder and
scratch settings, at its usual window size, and only the window of the
instance this script started is captured; that instance is quit after each
shot.

Needs macOS, swiftc, Python 3 with Pillow, and the previews. Run it from
anywhere:

    python3 docs/images/desktop-app/make_screenshots.py

EUPHORIA_APP names an app binary already built; otherwise the release build
is made with cargo. EUPHORIA_PREVIEWS names the folder of previews when it is
not bench/data/key/audio; the genre annotations are read from the dataset
folder beside it.
"""

import json
import os
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from PIL import Image, ImageChops, ImageStat

IMAGES = Path(__file__).resolve().parent
REPOSITORY = IMAGES.parents[2]
PREVIEWS = Path(os.environ.get("EUPHORIA_PREVIEWS", REPOSITORY / "bench/data/key/audio"))
GENRES = PREVIEWS.parent / "annotations" / "genre"

# Folder of the demo library, and the dataset genre whose previews go in it.
FOLDERS = [
    ("house/deep", "deep-house", 4),
    ("house/tech", "tech-house", 4),
    ("house/progressive", "progressive-house", 3),
    ("techno/peak", "techno", 4),
    ("techno/minimal", "minimal", 3),
]

# Width of the published images. The window is captured at the screen's
# scale and reduced to this.
PUBLISHED_WIDTH = 1600

# Where each run makes its own private work folder. Its path shows in the
# inspector's Location, so it is a short neutral one under /tmp, such as
# /tmp/Euphoria-x1y2z3, rather than a temporary folder whose path names the
# user.
WORK_PARENT = "/tmp"
WORK_PREFIX = "Euphoria-"

# How long to wait for the window to settle, and how often to look.
SETTLE_PATIENCE_SECONDS = 60.0
SETTLE_INTERVAL_SECONDS = 0.75
# Mean difference per channel, out of 255, under which two looks at the
# window count as the same.
SETTLED_DIFFERENCE = 0.05
# Mean difference in the inspector that shows another track was selected.
SELECTION_CHANGED_DIFFERENCE = 1.0
# How much darker the sidebar must get when a panel covers the window.
PANEL_DIMMING = 0.8

# Virtual key codes, and the keys pressed before each shot.
KEY_A = 0
KEY_T = 17
KEY_DOWN = 125
KEY_HOME = 115
# Home goes to the first row whichever track the app selected on its own,
# so the same row is reached on every run.
SELECT_A_TAGGED_TRACK = [(KEY_HOME, "")] + [(KEY_DOWN, "")] * 2
SELECT_A_TRACK_SORTED_BY_KEY = [(KEY_HOME, "")] + [(KEY_DOWN, "")] * 10
OPEN_WRITE_TAGS = [(KEY_A, "cmd"), (KEY_T, "cmd,shift")]

# What each set of keys must visibly do before its shot is taken.
SELECTS_ANOTHER_TRACK = "selects another track"
OPENS_A_PANEL = "opens a panel"


def main():
    # A folder of its own, made private by mkdtemp, so a second run or
    # someone else's files in /tmp are never touched; only it is deleted.
    work = Path(tempfile.mkdtemp(prefix=WORK_PREFIX, dir=WORK_PARENT))
    try:
        window_tool = compile_window_tool(work)
        app = app_binary()
        library = build_library(work / "Music" / "DJ tracks")
        house = library / "house"
        expanded = [library, house, library / "techno"]

        cache_folder = work / "data" / "cache"
        warm_cache(app, window_tool, cache_folder, library, track_count(library))

        shots = [
            (
                "main-window",
                {"theme": "slate", "selected": house},
                SELECT_A_TAGGED_TRACK,
                SELECTS_ANOTHER_TRACK,
            ),
            (
                "main-window-paper",
                {"theme": "paper", "selected": house},
                SELECT_A_TAGGED_TRACK,
                SELECTS_ANOTHER_TRACK,
            ),
            (
                "write-tags-preview",
                {"theme": "slate", "selected": house},
                OPEN_WRITE_TAGS,
                OPENS_A_PANEL,
            ),
            (
                "key-first",
                {"theme": "key-first", "selected": None},
                SELECT_A_TRACK_SORTED_BY_KEY,
                SELECTS_ANOTHER_TRACK,
            ),
        ]
        for name, choices, keys, expected in shots:
            data = work / "data" / name
            data.mkdir(parents=True)
            # The database and its write ahead log, which holds the latest
            # results until SQLite folds it into the database.
            for part in cache_folder.glob("analysis-cache.sqlite3*"):
                shutil.copy(part, data)
            write_settings(data, library, expanded, **choices)
            capture(app, window_tool, data, work / name, keys, expected)
            publish(work / name / "shot.png", IMAGES / f"{name}.png")
            print(f"wrote {IMAGES / f'{name}.png'}")
    finally:
        shutil.rmtree(work, ignore_errors=True)


def compile_window_tool(work):
    tool = work / "window"
    subprocess.run(["swiftc", "-O", str(IMAGES / "window.swift"), "-o", str(tool)], check=True)
    return tool


def app_binary():
    chosen = os.environ.get("EUPHORIA_APP")
    if chosen:
        return Path(chosen)
    subprocess.run(
        ["cargo", "build", "--release", "-p", "euphoria"], cwd=REPOSITORY, check=True
    )
    return REPOSITORY / "target/release/euphoria-desktop"


def build_library(root):
    """Copies the first previews of each genre, by name, into their folder."""
    by_genre = {}
    for genre_file in sorted(GENRES.glob("*.genre")):
        genre = genre_file.read_text().strip()
        by_genre.setdefault(genre, []).append(genre_file.name.removesuffix(".genre"))
    for folder, genre, count in FOLDERS:
        destination = root / folder
        destination.mkdir(parents=True)
        for name in by_genre[genre][:count]:
            preview = PREVIEWS / f"{name}.mp3"
            number = name.split(".")[0]
            shutil.copy(preview, destination / f"GiantSteps {number}.mp3")
    return root


def track_count(library):
    return sum(1 for _ in library.rglob("*.mp3"))


def write_settings(data, library, expanded, theme, selected):
    settings = {
        "folders": [str(library)],
        "theme": theme,
        "folder_tree": {
            "expanded": [str(folder) for folder in expanded],
            "selected": str(selected) if selected else None,
        },
        "tags": {"program": "rekordbox"},
    }
    (data / "settings.json").write_text(json.dumps(settings))


def launch(app, data):
    environment = dict(os.environ, EUPHORIA_DATA_DIR=str(data))
    environment.pop("EUPHORIA_DEMO_TRACKS", None)
    return subprocess.Popen(
        [str(app)], env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
    )


def quit_app(process):
    process.send_signal(signal.SIGTERM)
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def window_number(window_tool, process, patience=60.0):
    """The number of the app's window once it is on screen."""
    deadline = time.monotonic() + patience
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"the app exited with status {process.returncode}")
        result = subprocess.run(
            [str(window_tool), "id", str(process.pid)], capture_output=True, text=True
        )
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout.strip()
        time.sleep(1.0)
    raise RuntimeError("the app's window never came on screen")


def warm_cache(app, window_tool, data, library, expected):
    """Runs the app once so every track is analyzed and kept in the cache."""
    data.mkdir(parents=True)
    write_settings(data, library, [], theme="slate", selected=None)
    process = launch(app, data)
    try:
        window_number(window_tool, process)
        database = data / "analysis-cache.sqlite3"
        deadline = time.monotonic() + 600
        while time.monotonic() < deadline:
            if database.exists() and analysis_count(database) >= expected:
                break
            time.sleep(2.0)
        else:
            raise RuntimeError("the tracks were not all analyzed in time")
    finally:
        quit_app(process)


def analysis_count(database):
    try:
        with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as connection:
            return connection.execute("SELECT COUNT(*) FROM analyses").fetchone()[0]
    except sqlite3.Error:
        return 0


def capture(app, window_tool, data, folder, keys, expected):
    """Waits for the window to settle, presses `keys`, waits for it to
    settle again, checks that the keys did what was `expected`, and keeps the
    last look at the window as the shot. Fails rather than keep a shot of
    the wrong state."""
    folder.mkdir()
    process = launch(app, data)
    try:
        before = wait_until_settled(window_tool, process, folder / "before.png")
        for code, flags in keys:
            subprocess.run(
                [str(window_tool), "key", str(process.pid), str(code), flags], check=True
            )
            time.sleep(0.3)
        after = wait_until_settled(window_tool, process, folder / "shot.png")
        check_expected(before, after, expected, folder.name)
    finally:
        quit_app(process)


def look(window_tool, process, image, patience=30.0):
    """Captures the app's window, and only it, into `image`. A window that
    is briefly off screen, as while the screen is locked, is waited for;
    one that stays away fails."""
    deadline = time.monotonic() + patience
    while True:
        number = window_number(window_tool, process)
        result = subprocess.run(["screencapture", "-x", "-o", "-l", number, str(image)])
        if result.returncode == 0 and image.exists():
            with Image.open(image) as picture:
                return picture.convert("RGB")
        if time.monotonic() > deadline:
            raise RuntimeError(f"the window could not be captured: status {result.returncode}")
        time.sleep(1.0)


def difference(first, second):
    """Mean difference per channel between two looks, out of 255."""
    if first.size != second.size:
        return 255.0
    return sum(ImageStat.Stat(ImageChops.difference(first, second)).mean) / 3.0


def wait_until_settled(window_tool, process, image):
    """Looks at the window until two looks in a row are the same: the
    table has filled, the waveform and energy curve are drawn, or the panel
    has been worked out. Fails after SETTLE_PATIENCE_SECONDS."""
    deadline = time.monotonic() + SETTLE_PATIENCE_SECONDS
    previous = look(window_tool, process, image)
    while time.monotonic() < deadline:
        time.sleep(SETTLE_INTERVAL_SECONDS)
        current = look(window_tool, process, image)
        if difference(previous, current) < SETTLED_DIFFERENCE:
            return current
        previous = current
    raise RuntimeError("the window kept changing and never settled")


def region(picture, left, top, right, bottom):
    """The part of `picture` between the given fractions of its size."""
    width, height = picture.size
    return picture.crop((int(left * width), int(top * height), int(right * width), int(bottom * height)))


def brightness(picture):
    return sum(ImageStat.Stat(picture).mean) / 3.0


def check_expected(before, after, expected, name):
    if expected == SELECTS_ANOTHER_TRACK:
        # The title, the statistics and the waveform at the top of the
        # inspector change with the track.
        inspector = (0.8, 0.05, 1.0, 0.4)
        changed = difference(region(before, *inspector), region(after, *inspector))
        if changed < SELECTION_CHANGED_DIFFERENCE:
            raise RuntimeError(f"{name}: the keys did not select another track")
    elif expected == OPENS_A_PANEL:
        # A panel dims the whole window behind it, the sidebar included.
        sidebar = (0.0, 0.1, 0.12, 0.5)
        dimmed = brightness(region(after, *sidebar)) / max(brightness(region(before, *sidebar)), 1.0)
        if dimmed > PANEL_DIMMING:
            raise RuntimeError(f"{name}: the keys did not open the panel")
    else:
        raise ValueError(expected)


def publish(captured, published):
    """Reduces the capture to the published width and to a palette of 256
    colours, which keeps interface text crisp at a fraction of the size."""
    with Image.open(captured) as picture:
        picture = picture.convert("RGB")
        height = round(picture.height * PUBLISHED_WIDTH / picture.width)
        picture = picture.resize((PUBLISHED_WIDTH, height), Image.Resampling.LANCZOS)
        reduced = picture.quantize(
            colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE
        )
        reduced.save(published, optimize=True)


if __name__ == "__main__":
    sys.exit(main())
