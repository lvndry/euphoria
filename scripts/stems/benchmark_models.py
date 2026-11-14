"""Compare stem separation models on quality, speed and memory, CPU only.

Three subcommands share one model registry:

    evaluate   SDR per stem on a fixed subset of MUSDB18-HQ test tracks
    cost       wall time, processor time and peak memory on one audio file
    separate   write the stems and the acapella and instrumental of one file

Models are named on the command line:

    demucs:htdemucs            htdemucs, one pass
    demucs:htdemucs:shifts=2   htdemucs averaged over 2 random time shifts
    demucs:htdemucs_ft         the fine-tuned bag of four models
    roformer:bs:CONFIG:CHECKPOINT
    roformer:mel:CONFIG:CHECKPOINT

The RoFormer models use the network code of
https://github.com/ZFTurbo/Music-Source-Separation-Training, passed as
--mss-repo. MUSDB18-HQ is research-only data: point --musdb-root at a local
copy and never commit it. Install the packages in
scripts/stems/requirements-benchmark.txt into a scratch virtual environment.

    python scripts/stems/benchmark_models.py evaluate demucs:htdemucs \
        --musdb-root musdb/test --subset subset.json --output htdemucs.json
"""

import argparse
import json
import math
import os
import resource
import sys
import time
from pathlib import Path

import numpy as np
import soundfile
import torch
import yaml

SAMPLE_RATE = 44100
DEMUCS_STEMS = ["drums", "bass", "other", "vocals"]
ACCOMPANIMENT = "accompaniment"


class ConfigLoader(yaml.SafeLoader):
    """Safe YAML loader that also reads the python/tuple tag used by MSS configs."""


ConfigLoader.add_constructor(
    "tag:yaml.org,2002:python/tuple",
    lambda loader, node: tuple(loader.construct_sequence(node)),
)


class DemucsSeparator:
    def __init__(self, name: str, shifts: int, threads: int) -> None:
        from demucs.pretrained import get_model

        self.model = get_model(name)
        self.model.eval()
        self.shifts = shifts
        self.stem_names = list(self.model.sources)

    def separate(self, audio: torch.Tensor) -> dict[str, torch.Tensor]:
        from demucs.apply import apply_model

        reference = audio.mean(0)
        centred = audio - reference.mean()
        scale = reference.std()
        with torch.inference_mode():
            stems = apply_model(
                self.model,
                (centred / scale)[None],
                shifts=self.shifts,
                split=True,
                overlap=0.25,
                device="cpu",
                num_workers=0,
                progress=False,
            )[0]
        stems = stems * scale + reference.mean()
        return {name: stems[index] for index, name in enumerate(self.stem_names)}


class RoformerSeparator:
    def __init__(self, architecture: str, config_path: str, checkpoint_path: str, mss_repo: str) -> None:
        sys.path.insert(0, mss_repo)
        with open(config_path) as handle:
            config = yaml.load(handle, Loader=ConfigLoader)
        if architecture == "bs":
            from models.bs_roformer.bs_roformer import BSRoformer as Network
        else:
            from models.bs_roformer.mel_band_roformer import MelBandRoformer as Network
        self.network = Network(**dict(config["model"]))
        state = torch.load(checkpoint_path, map_location="cpu", weights_only=True)
        for wrapper in ("state_dict", "model"):
            if isinstance(state, dict) and wrapper in state and isinstance(state[wrapper], dict):
                state = state[wrapper]
        self.network.load_state_dict(state)
        self.network.eval()
        self.chunk_size = int(config["audio"]["chunk_size"])
        self.num_overlap = int(config["inference"]["num_overlap"])
        stems = config["training"]["instruments"]
        target = config["training"].get("target_instrument")
        self.stem_names = [target] if target else list(stems)

    def separate(self, audio: torch.Tensor) -> dict[str, torch.Tensor]:
        length = audio.shape[-1]
        step = self.chunk_size // self.num_overlap
        border = self.chunk_size - step
        mode = "reflect" if length > 2 * border else "constant"
        padded = torch.nn.functional.pad(audio, (border, border), mode=mode)
        padded_length = padded.shape[-1]
        starts = list(range(0, max(padded_length - self.chunk_size, 0) + 1, step))
        if starts[-1] + self.chunk_size < padded_length:
            starts.append(padded_length - self.chunk_size)
        window = torch.hann_window(self.chunk_size, periodic=True).clamp_min(1e-3)
        stem_count = len(self.stem_names)
        accumulated = torch.zeros(stem_count, 2, padded_length)
        weights = torch.zeros(padded_length)
        with torch.inference_mode():
            for start in starts:
                chunk = padded[:, start : start + self.chunk_size]
                if chunk.shape[-1] < self.chunk_size:
                    chunk = torch.nn.functional.pad(chunk, (0, self.chunk_size - chunk.shape[-1]))
                output = self.network(chunk[None])[0]
                if stem_count == 1:
                    output = output[None]
                accumulated[:, :, start : start + self.chunk_size] += output * window
                weights[start : start + self.chunk_size] += window
        stems = accumulated / weights.clamp_min(1e-8)
        stems = stems[:, :, border : border + length]
        return {name: stems[index] for index, name in enumerate(self.stem_names)}


def build_separator(spec: str, mss_repo: str | None, threads: int):
    torch.set_num_threads(threads)
    parts = spec.split(":")
    kind = parts[0]
    if kind == "demucs":
        shifts = 0
        for option in parts[2:]:
            key, value = option.split("=")
            if key == "shifts":
                shifts = int(value)
        torch.manual_seed(0)
        return DemucsSeparator(parts[1], shifts, threads)
    if kind == "roformer":
        if mss_repo is None:
            raise SystemExit("roformer models need --mss-repo")
        return RoformerSeparator(parts[1], parts[2], parts[3], mss_repo)
    raise SystemExit(f"unknown model spec {spec}")


def complete_stems(stems: dict[str, torch.Tensor], mixture: torch.Tensor) -> dict[str, torch.Tensor]:
    """Add the vocals and accompaniment views every model can be compared on."""
    result = dict(stems)
    if "drums" in stems and "bass" in stems and "other" in stems:
        result[ACCOMPANIMENT] = stems["drums"] + stems["bass"] + stems["other"]
    else:
        result[ACCOMPANIMENT] = mixture - stems["vocals"]
    return result


def read_excerpt(path: Path, offset_seconds: float, duration_seconds: float) -> np.ndarray:
    audio, rate = soundfile.read(
        path,
        start=int(offset_seconds * SAMPLE_RATE),
        frames=int(duration_seconds * SAMPLE_RATE),
        dtype="float32",
        always_2d=True,
    )
    if rate != SAMPLE_RATE:
        raise SystemExit(f"{path} is {rate} Hz, expected {SAMPLE_RATE}")
    return audio


def global_sdr(reference: np.ndarray, estimate: np.ndarray) -> float:
    signal = float(np.sum(reference**2))
    noise = float(np.sum((reference - estimate) ** 2))
    if signal < 1e-9:
        return float("nan")
    return 10 * math.log10(signal / max(noise, 1e-12))


def framewise_sdr(reference: np.ndarray, estimate: np.ndarray) -> float:
    import museval

    scores = museval.metrics.bss_eval(
        reference[None],
        estimate[None],
        window=SAMPLE_RATE,
        hop=SAMPLE_RATE,
        compute_permutation=False,
        framewise_filters=False,
        bsseval_sources_version=False,
    )[0]
    return float(np.nanmedian(scores[0]))


def median_or_nan(values: list[float]) -> float:
    finite = [value for value in values if not math.isnan(value)]
    return float(np.median(finite)) if finite else float("nan")


def command_evaluate(arguments: argparse.Namespace) -> None:
    subset = json.loads(Path(arguments.subset).read_text())
    separator = build_separator(arguments.model, arguments.mss_repo, arguments.threads)
    names = ["vocals", "drums", "bass", "other", ACCOMPANIMENT]
    per_track: dict[str, dict] = {}
    elapsed_total = 0.0
    audio_total = 0.0
    for track in subset:
        folder = Path(arguments.musdb_root) / track
        mixture = read_excerpt(folder / "mixture.wav", arguments.offset, arguments.seconds)
        truths = {
            name: read_excerpt(folder / f"{name}.wav", arguments.offset, arguments.seconds)
            for name in ["vocals", "drums", "bass", "other"]
        }
        truths[ACCOMPANIMENT] = truths["drums"] + truths["bass"] + truths["other"]
        mixture_tensor = torch.from_numpy(mixture.T.copy())
        started = time.perf_counter()
        stems = separator.separate(mixture_tensor)
        elapsed = time.perf_counter() - started
        elapsed_total += elapsed
        audio_total += mixture.shape[0] / SAMPLE_RATE
        stems = complete_stems(stems, mixture_tensor)
        scores = {}
        for name in names:
            if name not in stems:
                continue
            estimate = stems[name].numpy().T
            scores[name] = {
                "median_frame_sdr": framewise_sdr(truths[name], estimate),
                "global_sdr": global_sdr(truths[name], estimate),
            }
        per_track[track] = {"seconds": elapsed, "scores": scores}
        print(track, round(elapsed, 1), {key: round(value["median_frame_sdr"], 2) for key, value in scores.items()}, flush=True)
    summary = {}
    for name in names:
        rows = [entry["scores"][name] for entry in per_track.values() if name in entry["scores"]]
        if rows:
            summary[name] = {
                "median_frame_sdr": median_or_nan([row["median_frame_sdr"] for row in rows]),
                "mean_frame_sdr": float(np.nanmean([row["median_frame_sdr"] for row in rows])),
                "global_sdr_mean": float(np.nanmean([row["global_sdr"] for row in rows])),
            }
    report = {
        "model": arguments.model,
        "threads": arguments.threads,
        "tracks": len(subset),
        "seconds_per_track": arguments.seconds,
        "offset_seconds": arguments.offset,
        "separation_seconds_per_audio_minute": elapsed_total / audio_total * 60,
        "summary": summary,
        "per_track": per_track,
    }
    Path(arguments.output).write_text(json.dumps(report, indent=2))
    print(json.dumps({"model": arguments.model, "summary": summary, "seconds_per_minute": report["separation_seconds_per_audio_minute"]}, indent=2))


def command_cost(arguments: argparse.Namespace) -> None:
    separator = build_separator(arguments.model, arguments.mss_repo, arguments.threads)
    audio, _ = soundfile.read(arguments.input, dtype="float32", always_2d=True)
    audio_tensor = torch.from_numpy(audio.T.copy())
    minutes = audio.shape[0] / SAMPLE_RATE / 60
    cpu_before = time.process_time()
    started = time.perf_counter()
    separator.separate(audio_tensor)
    wall = time.perf_counter() - started
    cpu = time.process_time() - cpu_before
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    peak_bytes = peak if sys.platform == "darwin" else peak * 1024
    print(json.dumps({
        "model": arguments.model,
        "threads": arguments.threads,
        "audio_minutes": minutes,
        "wall_seconds_per_minute": wall / minutes,
        "cpu_seconds_per_minute": cpu / minutes,
        "peak_rss_gb": peak_bytes / 1e9,
        "load_average_start": os.getloadavg()[0],
    }))


def command_separate(arguments: argparse.Namespace) -> None:
    separator = build_separator(arguments.model, arguments.mss_repo, arguments.threads)
    audio, rate = soundfile.read(arguments.input, dtype="float32", always_2d=True)
    if rate != SAMPLE_RATE:
        raise SystemExit(f"{arguments.input} is {rate} Hz, expected {SAMPLE_RATE}")
    mixture = torch.from_numpy(audio.T.copy())
    stems = complete_stems(separator.separate(mixture), mixture)
    output_dir = Path(arguments.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    tag = arguments.tag
    for stem_name, filename in (("vocals", "acapella"), (ACCOMPANIMENT, "instrumental")):
        soundfile.write(output_dir / f"{filename}_{tag}.wav", stems[stem_name].numpy().T.clip(-1, 1), SAMPLE_RATE, subtype="PCM_16")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    subcommands = parser.add_subparsers(dest="command", required=True)
    for name, handler in (("evaluate", command_evaluate), ("cost", command_cost), ("separate", command_separate)):
        sub = subcommands.add_parser(name)
        sub.add_argument("model")
        sub.add_argument("--mss-repo")
        sub.add_argument("--threads", type=int, default=max((os.cpu_count() or 2) - 1, 1))
        sub.set_defaults(handler=handler)
        if name == "evaluate":
            sub.add_argument("--musdb-root", required=True)
            sub.add_argument("--subset", required=True)
            sub.add_argument("--offset", type=float, default=30.0)
            sub.add_argument("--seconds", type=float, default=60.0)
            sub.add_argument("--output", required=True)
        else:
            sub.add_argument("--input", required=True)
        if name == "separate":
            sub.add_argument("--output-dir", required=True)
            sub.add_argument("--tag", required=True)
    arguments = parser.parse_args()
    arguments.handler(arguments)


if __name__ == "__main__":
    main()
