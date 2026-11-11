#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Builds the labels files that `euphoria-bench energy` reads from DEAM.

Each line of a labels file is an audio path, relative to the labels file, a
tab, and the reference rating: the mean arousal that listeners gave the song
on a scale from 1 to 9.

The songs are split once, by a hash of the song number, into a tuning set
(about 70%) and a held out set (about 30%). Models are fitted on the tuning
set only, and the held out set is used to judge them. The songs tagged as
electronic are also written to files of their own, with the same split.

When the GiantSteps Key previews are present next to the dataset, they are
listed in a labels file without references, which the benchmark reads to
report how ratings of dance music are distributed.
"""

import csv
import hashlib
import sys
from pathlib import Path

HELD_OUT_PERCENT = 30
SONG_LEVEL = Path("annotations") / "annotations averaged per song" / "song_level"
STATIC_ANNOTATIONS = [
    "static_annotations_averaged_songs_1_2000.csv",
    "static_annotations_averaged_songs_2000_2058.csv",
]
METADATA = ["metadata_2013.csv", "metadata_2014.csv", "metadata_2015.csv"]


def read_rows(path):
    with open(path, encoding="utf-8", errors="replace", newline="") as handle:
        reader = csv.reader(handle)
        header = [name.strip().lower() for name in next(reader)]
        for row in reader:
            if row:
                yield header, [cell.strip() for cell in row]


def arousal_by_song(dataset_directory):
    arousal = {}
    for name in STATIC_ANNOTATIONS:
        for header, row in read_rows(dataset_directory / SONG_LEVEL / name):
            arousal[int(row[header.index("song_id")])] = float(row[header.index("arousal_mean")])
    return arousal


def electronic_songs(dataset_directory):
    songs = set()
    for name in METADATA:
        for header, row in read_rows(dataset_directory / "metadata" / name):
            song_column = header.index("song_id") if "song_id" in header else header.index("id")
            genre = row[header.index("genre")].strip().lower()
            if "electronic" in genre:
                songs.add(int(row[song_column]))
    return songs


def is_held_out(song):
    digest = hashlib.sha256(str(song).encode()).digest()
    return digest[0] * 100 // 256 < HELD_OUT_PERCENT


def write_labels(path, songs, arousal, audio_directory):
    with open(path, "w", encoding="utf-8") as handle:
        for song in songs:
            relative_audio = (audio_directory / f"{song}.mp3").relative_to(path.parent)
            handle.write(f"{relative_audio}\t{arousal[song]}\n")
    print(f"{path.name}: {len(songs)} songs")


def write_giantsteps_list(energy_directory):
    audio_directory = energy_directory.parent / "key" / "audio"
    if not audio_directory.is_dir():
        print("GiantSteps Key is not downloaded, so giantsteps.tsv is not written", file=sys.stderr)
        return
    previews = sorted(audio_directory.glob("*.mp3"))
    path = energy_directory / "giantsteps.tsv"
    with open(path, "w", encoding="utf-8") as handle:
        for preview in previews:
            handle.write(f"../key/audio/{preview.name}\n")
    print(f"{path.name}: {len(previews)} previews")


def main():
    dataset_directory = Path(sys.argv[1]).resolve()
    audio_directory = dataset_directory / "MEMD_audio"
    arousal = arousal_by_song(dataset_directory)
    electronic = electronic_songs(dataset_directory)
    songs = sorted(song for song in arousal if (audio_directory / f"{song}.mp3").is_file())
    missing = len(arousal) - len(songs)
    if missing:
        print(f"{missing} annotated songs have no audio file and are left out", file=sys.stderr)

    tuning = [song for song in songs if not is_held_out(song)]
    held_out = [song for song in songs if is_held_out(song)]
    subsets = {
        "labels.tsv": songs,
        "labels-tuning.tsv": tuning,
        "labels-held-out.tsv": held_out,
        "labels-electronic.tsv": [song for song in songs if song in electronic],
        "labels-electronic-tuning.tsv": [song for song in tuning if song in electronic],
        "labels-electronic-held-out.tsv": [song for song in held_out if song in electronic],
    }
    for name, subset in subsets.items():
        write_labels(dataset_directory / name, subset, arousal, audio_directory)
    write_giantsteps_list(dataset_directory.parent)


if __name__ == "__main__":
    main()
