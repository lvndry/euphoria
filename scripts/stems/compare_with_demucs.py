"""Compare Euphoria's stems with the reference Demucs separation of a file.

Runs the Python Demucs package on the same weights Euphoria downloads,
without random shifts so that the reference is deterministic, then reports
for every stem the signal to distortion ratio of Euphoria's output against
the reference and their correlation.

    python scripts/stems/compare_with_demucs.py \
        --weights 955717e8.safetensors --mix clip.wav --stems euphoria-stems/

With --instrumental, a file holding the same mix without its vocals, it
also reports the SDR of both separations against the vocals that the
difference of the two files leaves, a rough ground truth.
"""

import argparse
import json
from pathlib import Path

import numpy
import soundfile
import torch
from demucs.apply import apply_model
from demucs.hf import load_safetensors_model

SOURCES = ["drums", "bass", "other", "vocals"]


def signal_to_distortion(reference: numpy.ndarray, estimate: numpy.ndarray) -> float:
    noise = numpy.sum((reference - estimate) ** 2)
    signal = numpy.sum(reference**2)
    return float(10 * numpy.log10((signal + 1e-12) / (noise + 1e-12)))


def correlation(reference: numpy.ndarray, estimate: numpy.ndarray) -> float:
    return float(numpy.corrcoef(reference.ravel(), estimate.ravel())[0, 1])


def reference_separation(weights: Path, mix: numpy.ndarray) -> numpy.ndarray:
    model = load_safetensors_model(weights)
    model.eval()
    waveform = torch.from_numpy(mix.T.copy())
    mono = waveform.mean(0)
    mean = mono.mean()
    deviation = mono.std()
    normalised = (waveform - mean) / deviation
    with torch.no_grad():
        sources = apply_model(model, normalised[None], shifts=0, split=True, overlap=0.25)[0]
    sources = sources * deviation + mean
    return sources.numpy()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--weights", type=Path, required=True)
    parser.add_argument("--mix", type=Path, required=True)
    parser.add_argument("--stems", type=Path, required=True)
    parser.add_argument("--instrumental", type=Path)
    parser.add_argument("--save-reference", type=Path)
    arguments = parser.parse_args()

    mix, sample_rate = soundfile.read(arguments.mix, dtype="float32", always_2d=True)
    if sample_rate != 44100 or mix.shape[1] != 2:
        raise SystemExit("the mix must be stereo at 44.1 kHz")
    reference = reference_separation(arguments.weights, mix)
    report = {}
    for index, source in enumerate(SOURCES):
        estimate, _ = soundfile.read(arguments.stems / f"{source}.wav", dtype="float32", always_2d=True)
        expected = reference[index].T
        length = min(len(estimate), len(expected))
        report[source] = {
            "sdr_against_reference_db": round(signal_to_distortion(expected[:length], estimate[:length]), 2),
            "correlation_with_reference": round(correlation(expected[:length], estimate[:length]), 6),
        }
        if arguments.save_reference:
            arguments.save_reference.mkdir(parents=True, exist_ok=True)
            soundfile.write(arguments.save_reference / f"{source}.wav", expected, 44100, subtype="FLOAT")

    if arguments.instrumental:
        instrumental, _ = soundfile.read(arguments.instrumental, dtype="float32", always_2d=True)
        length = min(len(instrumental), len(mix))
        vocals_truth = mix[:length] - instrumental[:length]
        vocals_index = SOURCES.index("vocals")
        euphoria_vocals, _ = soundfile.read(arguments.stems / "vocals.wav", dtype="float32", always_2d=True)
        report["vocals_against_mix_minus_instrumental"] = {
            "euphoria_sdr_db": round(signal_to_distortion(vocals_truth, euphoria_vocals[:length]), 2),
            "demucs_sdr_db": round(signal_to_distortion(vocals_truth, reference[vocals_index].T[:length]), 2),
        }
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
