# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# /// script
# requires-python = ">=3.12"
# dependencies = ["numpy"]
# ///
"""Measures what lossy encoding does to the values the quality report reads.

    ./bench/download_quality_sources.sh
    uv run bench/quality_benchmark.py bench/data/quality [ENCODER ...]

Takes the lossless sources in DATA/sources, adds synthesized wideband
material, encodes everything at 96 to 320 kbit/s with LAME (MP3), FFmpeg's
AAC encoder, Apple's AAC encoder when `afconvert` exists (macOS), and the
reference Vorbis encoder when `oggenc` exists, makes transcodes from low bit
rates up to high ones, runs `euphoria-bench quality`
on all of it, and prints a Markdown table per encoding: the measured bit
rate, the cutoff, the verdict, and how much encoding error there is below
15 kHz. The quality thresholds in `crates/euphoria-core/src/quality` come
from that table.

Naming encoders (lame, ffmpeg-aac, apple-aac, oggenc) measures only those,
with the transcodes that use nothing else, to add one encoder's rows to the
table without encoding everything again.
"""

import shutil
import statistics
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy

RATE = 44100
SYNTH_SECONDS = 60
BITRATES = [96, 128, 192, 256, 320]
# oggenc is used as people use it, with a quality level. These levels have
# the nominal bit rates of BITRATES.
VORBIS_QUALITY = {96: 2, 128: 4, 192: 6, 256: 8, 320: 9}
ERROR_BAND_HZ = 15000
ALIGN_SECONDS = 20
LONGEST_DELAY = 8192


def run(command, **options):
    return subprocess.run(command, check=True, capture_output=True, **options)


def write_wav(path, stereo):
    pcm = numpy.clip(stereo, -1.0, 1.0)
    data = (pcm * 32767).astype("<i2").tobytes()
    run(["ffmpeg", "-loglevel", "error", "-y", "-f", "s16le", "-ar", str(RATE), "-ac", "2",
         "-i", "-", str(path)], input=data)


def synthesize(folder):
    """A wideband dance loop, a dark version of it, and a clipped version."""
    random = numpy.random.default_rng(20261111)
    time = numpy.arange(SYNTH_SECONDS * RATE) / RATE
    beat = 60 / 126
    phase = time % beat
    kick = numpy.sin(2 * numpy.pi * (50 + 120 * numpy.exp(-phase * 40)) * phase) * numpy.exp(-phase * 7)
    hat_phase = (time + beat / 2) % (beat / 2)
    hats = random.standard_normal(len(time)) * numpy.exp(-hat_phase * 60)
    hats = numpy.diff(hats, prepend=0.0)
    chord = sum(((time * frequency) % 1.0) * 2 - 1 for frequency in (220.0, 277.18, 329.63, 440.0)) / 4
    air = random.standard_normal(len(time)) * 0.02
    mono = 0.55 * kick + 0.12 * hats + 0.12 * chord + air
    stereo = numpy.stack([mono, numpy.roll(mono, 7)], axis=1) * 0.8
    write_wav(folder / "synth-wideband.wav", stereo)
    coefficient = numpy.exp(-2 * numpy.pi * 3000 / RATE)
    dark = numpy.empty_like(stereo)
    state = numpy.zeros(2)
    for index in range(len(stereo)):
        state = (1 - coefficient) * stereo[index] + coefficient * state
        dark[index] = state
    write_wav(folder / "synth-dark.wav", dark / numpy.abs(dark).max() * 0.8)
    write_wav(folder / "synth-clipped.wav", numpy.clip(stereo * 2.8, -1.0, 1.0))


def encode(source, target, kind, bitrate):
    if kind == "lame":
        run(["ffmpeg", "-loglevel", "error", "-y", "-i", str(source), "-c:a", "libmp3lame",
             "-b:a", f"{bitrate}k", str(target)])
    elif kind == "ffmpeg-aac":
        run(["ffmpeg", "-loglevel", "error", "-y", "-i", str(source), "-c:a", "aac",
             "-b:a", f"{bitrate}k", str(target)])
    elif kind == "apple-aac":
        run(["afconvert", "-f", "m4af", "-d", "aac", "-b", str(bitrate * 1000), str(source), str(target)])
    elif kind == "oggenc":
        if source.suffix != ".wav":
            decoded = target.with_suffix(".source.wav")
            run(["ffmpeg", "-loglevel", "error", "-y", "-i", str(source), str(decoded)])
            source = decoded
        run(["oggenc", "--quiet", "-q", str(VORBIS_QUALITY[bitrate]), "-o", str(target), str(source)])
    elif kind == "flac":
        run(["ffmpeg", "-loglevel", "error", "-y", "-i", str(source), "-c:a", "flac", str(target)])


EXTENSION = {"lame": "mp3", "ffmpeg-aac": "m4a", "apple-aac": "m4a", "oggenc": "ogg", "flac": "flac"}


def available_kinds():
    kinds = ["lame", "ffmpeg-aac"] + (["apple-aac"] if shutil.which("afconvert") else [])
    return kinds + (["oggenc"] if shutil.which("oggenc") else [])


def make_variants(sources, folder, kinds):
    variants = []
    for source in sources:
        name = source.stem
        variants.append(("lossless source", name, source))
        flac = folder / f"{name}.flac"
        encode(source, flac, "flac", None)
        variants.append(("flac", name, flac))
        for kind in kinds:
            for bitrate in BITRATES:
                target = folder / f"{name}.{kind}-{bitrate}.{EXTENSION[kind]}"
                encode(source, target, kind, bitrate)
                variants.append((f"{kind} {bitrate}", name, target))
        low = folder / f"{name}.lame-128.mp3"
        # Each transcode with the encoders it needs.
        transcodes = [
            ("lame 128 to lame 320", low, "lame", 320, {"lame"}),
            ("lame 128 to flac", low, "flac", None, {"lame"}),
            ("lame 192 to lame 320", folder / f"{name}.lame-192.mp3", "lame", 320, {"lame"}),
            ("ffmpeg-aac 128 to ffmpeg-aac 256", folder / f"{name}.ffmpeg-aac-128.m4a",
             "ffmpeg-aac", 256, {"ffmpeg-aac"}),
            ("lame 128 to apple-aac 256", low, "apple-aac", 256, {"lame", "apple-aac"}),
            ("apple-aac 128 to apple-aac 320", folder / f"{name}.apple-aac-128.m4a",
             "apple-aac", 320, {"apple-aac"}),
            ("oggenc 128 to oggenc 320", folder / f"{name}.oggenc-128.ogg", "oggenc", 320, {"oggenc"}),
            ("oggenc 128 to flac", folder / f"{name}.oggenc-128.ogg", "flac", None, {"oggenc"}),
            ("lame 128 to oggenc 320", low, "oggenc", 320, {"lame", "oggenc"}),
        ]
        transcodes = [
            (label, origin, kind, bitrate)
            for label, origin, kind, bitrate, needed in transcodes
            if needed <= set(kinds)
        ]
        for label, origin, kind, bitrate in transcodes:
            target = folder / f"{name}.{label.replace(' ', '-')}.{EXTENSION[kind]}"
            encode(origin, target, kind, bitrate)
            variants.append((label, name, target))
    return variants


def decode(path):
    raw = run(["ffmpeg", "-loglevel", "error", "-i", str(path), "-ac", "1", "-ar", str(RATE),
               "-f", "f32le", "-"]).stdout
    return numpy.frombuffer(raw, dtype=numpy.float32).astype(numpy.float64)


def band_limited(signal):
    spectrum = numpy.fft.rfft(signal)
    spectrum[int(ERROR_BAND_HZ / RATE * len(signal)):] = 0
    return numpy.fft.irfft(spectrum, len(signal))


def error_below_band(reference, encoded):
    """Encoding error below 15 kHz relative to the music, in dB, after the
    encoder's delay is found and removed."""
    length = ALIGN_SECONDS * RATE
    head = reference[:length]
    candidate = encoded[: length + LONGEST_DELAY]
    size = 1 << (len(candidate) + len(head)).bit_length()
    correlation = numpy.fft.irfft(numpy.fft.rfft(candidate, size) * numpy.conj(numpy.fft.rfft(head, size)), size)
    delay = int(numpy.argmax(correlation[:LONGEST_DELAY]))
    aligned = encoded[delay:]
    count = min(len(reference), len(aligned))
    music = band_limited(reference[:count])
    error = band_limited(aligned[:count]) - music
    return 10 * numpy.log10((error ** 2).sum() / (music ** 2).sum())


def main():
    data = Path(sys.argv[1]).resolve()
    kinds = sys.argv[2:] or available_kinds()
    unknown = set(kinds) - (set(EXTENSION) - {"flac"})
    if unknown:
        sys.exit(f"unknown encoders: {', '.join(sorted(unknown))}")
    sources = sorted((data / "sources").glob("*.wav"))
    work = data / "encoded"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    synthesize(work)
    sources += sorted(work.glob("synth-*.wav"))
    variants = make_variants(sources, work, kinds)

    listing = work / "list.tsv"
    listing.write_text("".join(f"{label}|{name}\t{path}\n" for label, name, path in variants))
    bench = Path(__file__).resolve().parent.parent
    table = run(["cargo", "run", "--quiet", "--release", "-p", "euphoria-bench", "--", "quality",
                 str(listing)], cwd=bench, text=True).stdout
    rows = [line.split("\t") for line in table.splitlines()]
    header, rows = rows[0], rows[1:]
    (data / "measurements.tsv").write_text(table)

    references = {name: decode(path) for label, name, path in variants if label == "lossless source"}
    errors = {}
    for label, name, path in variants:
        if label not in ("lossless source", "flac") and name in references:
            errors[(label, name)] = error_below_band(references[name], decode(path))

    column = {name: index for index, name in enumerate(header)}
    by_label = defaultdict(list)
    for row in rows:
        label, name = row[0].split("|")
        by_label[label].append((name, row))

    def cell(values, digits=0):
        values = [float(value) for value in values if value]
        return f"{statistics.median(values):.{digits}f}" if values else "-"

    print("| Encoding | Bit rate (kbit/s) | Cutoff, lowest to highest (kHz) | Full band files | Error below 15 kHz (dB) | Verdicts |")
    print("| --- | --- | --- | --- | --- | --- |")
    for label, entries in by_label.items():
        music = [(name, row) for name, row in entries if name not in ("synth-dark", "synth-clipped")]
        cutoffs = sorted(float(row[column["cutoff_hz"]]) / 1000 for _, row in music if row[column["cutoff_hz"]])
        full = sum(1 for _, row in music if not row[column["cutoff_hz"]])
        cutoff_text = f"{cutoffs[0]:.1f} to {cutoffs[-1]:.1f}" if cutoffs else "none"
        error_values = [errors[(label, name)] for name, _ in music if (label, name) in errors]
        error_text = f"{statistics.median(error_values):.1f}" if error_values else "-"
        verdicts = Counter(row[column["verdict"]] for _, row in music)
        verdict_text = ", ".join(f"{verdict} {count}" for verdict, count in verdicts.most_common())
        print(f"| {label} | {cell([row[column['bitrate_kbps']] for _, row in music])} | {cutoff_text} | {full} of {len(music)} | {error_text} | {verdict_text} |")

    print("\nControl material, per encoding (cutoff in kHz, clipped share, verdict):\n")
    print("| Encoding | Dark synth | Clipped synth |")
    print("| --- | --- | --- |")
    for label, entries in by_label.items():
        cells = []
        for control in ("synth-dark", "synth-clipped"):
            row = next((row for name, row in entries if name == control), None)
            if row is None:
                cells.append("-")
                continue
            cutoff = f"{float(row[column['cutoff_hz']]) / 1000:.1f}" if row[column["cutoff_hz"]] else "none"
            cells.append(f"{cutoff}, {float(row[column['clipped_share']]) * 100:.2f}%, {row[column['verdict']]}")
        print(f"| {label} | {cells[0]} | {cells[1]} |")

    clipped = [float(row[column["clipped_share"]]) for entries in by_label.values()
               for name, row in entries if name not in ("synth-clipped",)]
    print(f"\nClipped share over every file except the clipped synth: highest {max(clipped) * 100:.3f}%")
    decode_cpu = [float(row[column["decode_cpu_seconds"]]) for row in rows]
    quality_cpu = [float(row[column["decode_and_quality_cpu_seconds"]]) for row in rows]
    print(f"CPU per file: decode {statistics.mean(decode_cpu):.3f} s, decode with quality {statistics.mean(quality_cpu):.3f} s")


if __name__ == "__main__":
    main()
