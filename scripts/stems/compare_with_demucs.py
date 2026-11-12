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

SOURCES = ["drums", "bass", "other", "vocals"]


def signal_to_distortion(reference: numpy.ndarray, estimate: numpy.ndarray) -> float:
    noise = numpy.sum((reference - estimate) ** 2)
    signal = numpy.sum(reference**2)
    return float(10 * numpy.log10((signal + 1e-12) / (noise + 1e-12)))


def correlation(reference: numpy.ndarray, estimate: numpy.ndarray) -> float:
    return float(numpy.corrcoef(reference.ravel(), estimate.ravel())[0, 1])


def reference_separation(weights: Path, mix: numpy.ndarray) -> numpy.ndarray:
    import torch
    from demucs.apply import apply_model
    from demucs.hf import load_safetensors_model

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


def read_stereo(path: Path, frames: int | None = None) -> numpy.ndarray:
    audio, rate = soundfile.read(path, dtype="float32", always_2d=True)
    if rate != 44100 or audio.shape[1] != 2:
        raise SystemExit(f"{path} must be stereo at 44.1 kHz")
    if frames is not None and len(audio) != frames:
        raise SystemExit(f"{path} has {len(audio)} frames; expected exactly {frames}")
    return audio


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--weights", type=Path, required=True)
    parser.add_argument("--mix", type=Path, required=True)
    parser.add_argument("--stems", type=Path, required=True)
    parser.add_argument("--instrumental", type=Path)
    parser.add_argument("--save-reference", type=Path)
    arguments = parser.parse_args()

    mix = read_stereo(arguments.mix)
    reference = reference_separation(arguments.weights, mix)
    if reference.shape != (len(SOURCES), 2, len(mix)):
        raise SystemExit("the reference separation must match the mix's channels and frame count")
    report = {}
    vocals_estimate = None
    for index, source in enumerate(SOURCES):
        estimate = read_stereo(arguments.stems / f"{source}.wav", len(mix))
        if source == "vocals":
            vocals_estimate = estimate
        expected = reference[index].T
        report[source] = {
            "sdr_against_reference_db": round(signal_to_distortion(expected, estimate), 2),
            "correlation_with_reference": round(correlation(expected, estimate), 6),
        }
        if arguments.save_reference:
            arguments.save_reference.mkdir(parents=True, exist_ok=True)
            soundfile.write(arguments.save_reference / f"{source}.wav", expected, 44100, subtype="FLOAT")

    if arguments.instrumental:
        instrumental = read_stereo(arguments.instrumental, len(mix))
        vocals_truth = mix - instrumental
        vocals_index = SOURCES.index("vocals")
        euphoria_vocals = vocals_estimate
        report["vocals_against_mix_minus_instrumental"] = {
            "euphoria_sdr_db": round(signal_to_distortion(vocals_truth, euphoria_vocals), 2),
            "demucs_sdr_db": round(signal_to_distortion(vocals_truth, reference[vocals_index].T), 2),
        }
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
