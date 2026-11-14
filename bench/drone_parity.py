# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Compare Euphoria's Rust drone pipeline with drone's Python package on the same files.

Checks two things: given the exact PCM drone decodes, do the Rust features and
key probabilities match Python's? And from the original file, through
Euphoria's own decoder, does Rust pick the same key?

Build the example, then run this from a checkout of https://github.com/lvndry/drone
with ffmpeg installed:

    cargo build --release -p euphoria-core --example drone_features
    uv run --all-extras python /path/to/euphoria/bench/drone_parity.py \
        --audio /path/to/euphoria/bench/data/key/audio \
        --weights weights/drone-v2.pt \
        --example /path/to/euphoria/target/release/examples/drone_features \
        --out parity.json
"""

from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
from pathlib import Path

import numpy as np
import torch

from drone.checkpoints import load_checkpoint
from drone.datasets import Track, prepare
from drone.features import SR


def python_reading(model: torch.nn.Module, audio_file: Path, workdir: Path) -> tuple[np.ndarray, np.ndarray]:
    features, _ = prepare(Track("track", str(audio_file), ("C", "major")), str(workdir))
    with torch.inference_mode():
        probabilities = torch.softmax(model(torch.from_numpy(features[None])), 1)[0].numpy()
    return features, probabilities


def rust_readings(example: Path, requests: list[tuple[str, Path, Path]]) -> list[dict]:
    lines = "".join(f"{mode}\t{audio}\t{features}\n" for mode, audio, features in requests)
    output = subprocess.run([str(example)], input=lines, capture_output=True, text=True, check=True)
    return [json.loads(line) for line in output.stdout.splitlines()]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--audio", required=True, help="directory of audio files")
    parser.add_argument("--weights", required=True)
    parser.add_argument("--example", required=True, help="built drone_features example binary")
    parser.add_argument("--out", required=True)
    arguments = parser.parse_args()
    torch.set_num_threads(1)
    model, _ = load_checkpoint(arguments.weights)
    scratch = Path(tempfile.mkdtemp(prefix="drone-parity-"))
    audio_files = sorted(Path(arguments.audio).glob("*.mp3"))
    python_results = {}
    requests = []
    for audio_file in audio_files:
        wav_file = scratch / f"{audio_file.stem}.wav"
        subprocess.run(
            ["ffmpeg", "-loglevel", "error", "-y", "-i", str(audio_file), "-ac", "1", "-ar", str(SR), str(wav_file)],
            check=True,
        )
        python_results[audio_file.stem] = python_reading(model, audio_file, scratch / "cache")
        requests.append(("model-input", wav_file, scratch / f"{audio_file.stem}.same-pcm.f32"))
        requests.append(("decoded", audio_file, scratch / f"{audio_file.stem}.decoded.f32"))
    readings = rust_readings(Path(arguments.example), requests)
    rows = []
    for index, audio_file in enumerate(audio_files):
        python_features, python_probabilities = python_results[audio_file.stem]
        row = {"track": audio_file.stem, "python_key_class": int(python_probabilities.argmax())}
        for offset, mode in enumerate(("same_pcm", "decoded")):
            reading = readings[2 * index + offset]
            rust_probabilities = np.array(reading["probabilities"], dtype=np.float32)
            rust_features = np.fromfile(requests[2 * index + offset][2], dtype="<f4").reshape(192, -1)
            frames = min(rust_features.shape[1], python_features.shape[1])
            row[mode] = {
                "key_class": int(rust_probabilities.argmax()),
                "frames_rust": int(rust_features.shape[1]),
                "frames_python": int(python_features.shape[1]),
                "feature_max_abs_difference": float(np.abs(rust_features[:, :frames] - python_features[:, :frames]).max()),
                "probability_max_abs_difference": float(np.abs(rust_probabilities - python_probabilities).max()),
            }
        rows.append(row)
    Path(arguments.out).write_text(json.dumps(rows, indent=1))
    for mode in ("same_pcm", "decoded"):
        same_key = sum(row[mode]["key_class"] == row["python_key_class"] for row in rows)
        worst_feature = max(row[mode]["feature_max_abs_difference"] for row in rows)
        worst_probability = max(row[mode]["probability_max_abs_difference"] for row in rows)
        print(
            f"{mode}: same key {same_key}/{len(rows)}, worst feature difference {worst_feature:.2e}, "
            f"worst probability difference {worst_probability:.2e}"
        )


if __name__ == "__main__":
    main()
