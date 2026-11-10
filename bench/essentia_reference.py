# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# /// script
# requires-python = ">=3.12,<3.14"
# dependencies = ["essentia", "numpy"]
# ///
"""Reference scores from Essentia on the GiantSteps datasets.

Prints a Markdown report with the same scoring rules as the Rust benchmark.
Run with: uv run bench/essentia_reference.py bench/data
"""

import sys
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

KEY_PROFILES = ["edma", "bgate", "shaath", "krumhansl"]
NATURAL_PITCH_CLASSES = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}
ACCIDENTAL_OFFSETS = {"": 0, "b": -1, "#": 1}
TEMPO_TOLERANCE = 0.04
ACCURACY_2_FACTORS = [1.0, 2.0, 0.5, 3.0, 1.0 / 3.0]


def pitch_class(note_name: str) -> int:
    natural = NATURAL_PITCH_CLASSES[note_name[0].upper()]
    offset = ACCIDENTAL_OFFSETS[note_name[1:]]
    return (natural + offset) % 12


def parse_annotated_key(text: str) -> tuple[int, str]:
    note_name, mode = text.split()[:2]
    return pitch_class(note_name), mode.lower()


def classify_key(reference: tuple[int, str], estimate: tuple[int, str]) -> str:
    reference_tonic, reference_mode = reference
    estimate_tonic, estimate_mode = estimate
    interval = (estimate_tonic - reference_tonic) % 12
    same_mode = reference_mode == estimate_mode
    relative_interval = 9 if reference_mode == "major" else 3
    if same_mode and interval == 0:
        return "exact"
    if same_mode and interval in (5, 7):
        return "fifth"
    if not same_mode and interval == relative_interval:
        return "relative"
    if not same_mode and interval == 0:
        return "parallel"
    return "other"


def detect_keys(audio_path: str) -> dict[str, tuple[int, str]]:
    import essentia

    essentia.log.warningActive = False
    import essentia.standard as standard

    audio = standard.MonoLoader(filename=audio_path, sampleRate=44100)()
    estimates = {}
    for profile in KEY_PROFILES:
        extractor = standard.KeyExtractor(profileType=profile)
        note_name, scale, _strength = extractor(audio)
        estimates[profile] = (pitch_class(note_name), scale)
    return estimates


def detect_tempo(audio_path: str) -> float:
    import essentia

    essentia.log.warningActive = False
    import essentia.standard as standard

    audio = standard.MonoLoader(filename=audio_path, sampleRate=44100)()
    extractor = standard.RhythmExtractor2013(method="multifeature")
    beats_per_minute = extractor(audio)[0]
    return float(beats_per_minute)


def load_tracks(dataset_directory: Path, annotation_subdirectory: str) -> list[tuple[str, str]]:
    tracks = []
    for annotation_path in sorted((dataset_directory / annotation_subdirectory).iterdir()):
        audio_path = dataset_directory / "audio" / f"{annotation_path.stem}.mp3"
        if audio_path.is_file():
            tracks.append((str(audio_path), annotation_path.read_text().strip()))
    return tracks


def percent(count: int, total: int) -> str:
    return f"{100.0 * count / total:.1f}%"


def report_keys(data_directory: Path) -> None:
    tracks = load_tracks(data_directory / "key", "annotations/key")
    audio_paths = [audio_path for audio_path, _annotation in tracks]
    with ProcessPoolExecutor() as executor:
        all_estimates = list(executor.map(detect_keys, audio_paths, chunksize=8))

    print("## Key detection, GiantSteps Key (Essentia reference)\n")
    print(f"{len(tracks)} tracks.\n")
    print("| Detector | Exact | Fifth | Relative | Parallel | Other | MIREX score |")
    print("| --- | --- | --- | --- | --- | --- | --- |")
    for profile in KEY_PROFILES:
        counts = {"exact": 0, "fifth": 0, "relative": 0, "parallel": 0, "other": 0}
        for (_audio_path, annotation), estimates in zip(tracks, all_estimates):
            category = classify_key(parse_annotated_key(annotation), estimates[profile])
            counts[category] += 1
        total = len(tracks)
        weighted = (
            counts["exact"]
            + 0.5 * counts["fifth"]
            + 0.3 * counts["relative"]
            + 0.2 * counts["parallel"]
        )
        print(
            f"| Essentia, {profile} profile "
            f"| {percent(counts['exact'], total)} "
            f"| {percent(counts['fifth'], total)} "
            f"| {percent(counts['relative'], total)} "
            f"| {percent(counts['parallel'], total)} "
            f"| {percent(counts['other'], total)} "
            f"| {100.0 * weighted / total:.1f}% |"
        )
    print()


def report_tempo(data_directory: Path) -> None:
    tracks = load_tracks(data_directory / "tempo", "annotations_v2/tempo")
    audio_paths = [audio_path for audio_path, _annotation in tracks]
    with ProcessPoolExecutor() as executor:
        estimates = list(executor.map(detect_tempo, audio_paths, chunksize=8))

    accurate = 0
    accurate_with_octaves = 0
    for (_audio_path, annotation), estimate in zip(tracks, estimates):
        reference = float(annotation.split()[0])
        if abs(estimate - reference) <= TEMPO_TOLERANCE * reference:
            accurate += 1
        if any(
            abs(estimate - reference * factor) <= TEMPO_TOLERANCE * reference * factor
            for factor in ACCURACY_2_FACTORS
        ):
            accurate_with_octaves += 1

    total = len(tracks)
    print("## Tempo detection, GiantSteps Tempo (Essentia reference)\n")
    print(f"{total} tracks.\n")
    print("| Detector | Accuracy 1 | Accuracy 2 |")
    print("| --- | --- | --- |")
    print(
        f"| Essentia, RhythmExtractor2013 "
        f"| {percent(accurate, total)} "
        f"| {percent(accurate_with_octaves, total)} |"
    )
    print()


def main() -> None:
    data_directory = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("bench/data")
    report_keys(data_directory)
    report_tempo(data_directory)


if __name__ == "__main__":
    main()
