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
import time
from pathlib import Path

from PIL import Image

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

# The work folder. Its path shows in the inspector's Location, so it is a
# short neutral one rather than a temporary folder named after the user.
WORK = Path("/tmp/Euphoria")
WORK_MARKER = ".made-by-make_screenshots"

# Virtual key codes, and the keys pressed before each shot.
KEY_A = 0
KEY_T = 17
KEY_DOWN = 125
SELECT_A_TAGGED_TRACK = [(KEY_DOWN, "")] * 2
SELECT_A_TRACK_SORTED_BY_KEY = [(KEY_DOWN, "")] * 10
OPEN_WRITE_TAGS = [(KEY_A, "cmd"), (KEY_T, "cmd,shift")]


def main():
    if WORK.exists() and not (WORK / WORK_MARKER).exists():
        sys.exit(f"{WORK} exists and was not made by this script; move it away first")
    shutil.rmtree(WORK, ignore_errors=True)
    WORK.mkdir()
    (WORK / WORK_MARKER).touch()
    work = WORK
    try:
        window_tool = compile_window_tool(work)
        app = app_binary()
        library = build_library(work / "Music" / "DJ tracks")
        house = library / "house"
        expanded = [library, house, library / "techno"]

        cache_folder = work / "data" / "cache"
        warm_cache(app, window_tool, cache_folder, library, track_count(library))

        shots = [
            ("main-window", {"theme": "slate", "selected": house}, SELECT_A_TAGGED_TRACK),
            (
                "main-window-paper",
                {"theme": "paper", "selected": house},
                SELECT_A_TAGGED_TRACK,
            ),
            ("write-tags-preview", {"theme": "slate", "selected": house}, OPEN_WRITE_TAGS),
            (
                "key-first",
                {"theme": "key-first", "selected": None},
                SELECT_A_TRACK_SORTED_BY_KEY,
            ),
        ]
        for name, choices, keys in shots:
            data = work / "data" / name
            data.mkdir(parents=True)
            # The database and its write ahead log, which holds the latest
            # results until SQLite folds it into the database.
            for part in cache_folder.glob("analysis-cache.sqlite3*"):
                shutil.copy(part, data)
            write_settings(data, library, expanded, **choices)
            capture(app, window_tool, data, work / f"{name}.png", keys)
            publish(work / f"{name}.png", IMAGES / f"{name}.png")
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
        ["cargo", "build", "--release", "-p", "euphoria-app"], cwd=REPOSITORY, check=True
    )
    return REPOSITORY / "target/release/euphoria-app"


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


def capture(app, window_tool, data, image, keys):
    process = launch(app, data)
    try:
        window_number(window_tool, process)
        # Time for the table to fill from the cache.
        time.sleep(6.0)
        for code, flags in keys:
            subprocess.run([str(window_tool), "key", str(process.pid), str(code), flags])
            time.sleep(0.5)
        # Time for the selected track's waveform and energy curve to be
        # worked out, or for the write preview to be planned.
        time.sleep(5.0)
        number = window_number(window_tool, process)
        subprocess.run(["screencapture", "-x", "-o", "-l", number, str(image)], check=True)
    finally:
        quit_app(process)


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
