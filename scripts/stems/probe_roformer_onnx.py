"""Probe whether a RoFormer separator can run through ONNX Runtime the way htdemucs does.

The short-time Fourier transforms stay outside the graph, as for htdemucs.
The exported part is everything between the real-valued spectrogram and the
raw mask estimator output: band split, the time and frequency transformers,
and the mask heads. The script exports that part, lists the ONNX operators
it uses, runs it once in ONNX Runtime on a random spectrogram, compares it
with PyTorch, and prints the time and the peak resident memory.

    python scripts/stems/probe_roformer_onnx.py mel CONFIG CHECKPOINT \
        --mss-repo path/to/Music-Source-Separation-Training --output core.onnx
"""

import argparse
import json
import resource
import sys
import time
from collections import Counter
from pathlib import Path

import numpy as np
import torch
import yaml
from einops import rearrange, pack, unpack


class ConfigLoader(yaml.SafeLoader):
    """Safe YAML loader that also reads the python/tuple tag used by MSS configs."""


ConfigLoader.add_constructor(
    "tag:yaml.org,2002:python/tuple",
    lambda loader, node: tuple(loader.construct_sequence(node)),
)


class RoformerCore(torch.nn.Module):
    def __init__(self, network: torch.nn.Module, architecture: str) -> None:
        super().__init__()
        self.network = network
        self.architecture = architecture

    def forward(self, spectrogram: torch.Tensor) -> torch.Tensor:
        network = self.network
        if self.architecture == "mel":
            batch_index = torch.arange(spectrogram.shape[0])[..., None]
            spectrogram = spectrogram[batch_index, network.freq_indices]
        x = rearrange(spectrogram, "b f t c -> b t (f c)")
        x = network.band_split(x)
        for block in network.layers:
            if len(block) == 3:
                linear_transformer, time_transformer, freq_transformer = block
                x, packing = pack([x], "b * d")
                x = linear_transformer(x)
                (x,) = unpack(x, packing, "b * d")
            else:
                time_transformer, freq_transformer = block
            x = rearrange(x, "b t f d -> b f t d")
            x, packing = pack([x], "* t d")
            x = time_transformer(x)
            (x,) = unpack(x, packing, "* t d")
            x = rearrange(x, "b f t d -> b t f d")
            x, packing = pack([x], "* f d")
            x = freq_transformer(x)
            (x,) = unpack(x, packing, "* f d")
        if self.architecture == "bs":
            x = network.final_norm(x)
        return torch.stack([head(x) for head in network.mask_estimators], dim=1)


def load_network(architecture: str, config_path: str, checkpoint_path: str, mss_repo: str):
    sys.path.insert(0, mss_repo)
    with open(config_path) as handle:
        config = yaml.load(handle, Loader=ConfigLoader)
    if architecture == "bs":
        from models.bs_roformer.bs_roformer import BSRoformer as Network
    else:
        from models.bs_roformer.mel_band_roformer import MelBandRoformer as Network
    network = Network(**dict(config["model"]))
    state = torch.load(checkpoint_path, map_location="cpu", weights_only=True)
    network.load_state_dict(state)
    network.eval()
    return network, config


def run_only(arguments: argparse.Namespace) -> None:
    import onnxruntime

    with open(arguments.config) as handle:
        config = yaml.load(handle, Loader=ConfigLoader)
    frames = int(config["audio"]["chunk_size"]) // int(config["model"]["stft_hop_length"]) + 1
    frequencies = int(config["model"]["dim_freqs_in"]) * 2
    spectrogram = np.random.default_rng(0).standard_normal((1, frequencies, frames, 2)).astype(np.float32)
    options = onnxruntime.SessionOptions()
    options.intra_op_num_threads = arguments.threads
    session = onnxruntime.InferenceSession(arguments.output, options, providers=["CPUExecutionProvider"])
    session.run(None, {"spectrogram": spectrogram})
    started = time.perf_counter()
    session.run(None, {"spectrogram": spectrogram})
    seconds = time.perf_counter() - started
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    print(json.dumps({
        "seconds_per_chunk": seconds,
        "chunk_seconds": int(config["audio"]["chunk_size"]) / 44100,
        "peak_rss_gb": (peak if sys.platform == "darwin" else peak * 1024) / 1e9,
    }))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("architecture", choices=["bs", "mel"])
    parser.add_argument("config")
    parser.add_argument("checkpoint")
    parser.add_argument("--mss-repo", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--opset", type=int, default=17)
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--run-only", action="store_true", help="only time the existing --output graph, in a process without PyTorch weights")
    arguments = parser.parse_args()

    if arguments.run_only:
        run_only(arguments)
        return

    torch.set_num_threads(arguments.threads)
    network, config = load_network(arguments.architecture, arguments.config, arguments.checkpoint, arguments.mss_repo)
    core = RoformerCore(network, arguments.architecture).eval()
    hop = int(config["model"]["stft_hop_length"])
    frames = int(config["audio"]["chunk_size"]) // hop + 1
    frequencies = int(config["model"]["dim_freqs_in"]) * 2
    torch.manual_seed(0)
    spectrogram = torch.randn(1, frequencies, frames, 2)

    with torch.inference_mode():
        expected = core(spectrogram)
    export_report = {"architecture": arguments.architecture, "frames": frames, "export": "ok"}
    try:
        torch.onnx.export(
            core,
            (spectrogram,),
            arguments.output,
            input_names=["spectrogram"],
            output_names=["masks"],
            opset_version=arguments.opset,
            dynamo=False,
        )
    except Exception as error:
        export_report["export"] = f"failed: {type(error).__name__}: {str(error)[:600]}"
        print(json.dumps(export_report, indent=2))
        return

    import onnx

    model = onnx.load(arguments.output)
    export_report["operators"] = dict(Counter(node.op_type for node in model.graph.node).most_common())
    export_report["file_megabytes"] = Path(arguments.output).stat().st_size / 1e6

    import onnxruntime

    options = onnxruntime.SessionOptions()
    options.intra_op_num_threads = arguments.threads
    session = onnxruntime.InferenceSession(arguments.output, options, providers=["CPUExecutionProvider"])
    started = time.perf_counter()
    actual = session.run(None, {"spectrogram": spectrogram.numpy()})[0]
    first_seconds = time.perf_counter() - started
    started = time.perf_counter()
    session.run(None, {"spectrogram": spectrogram.numpy()})
    second_seconds = time.perf_counter() - started
    difference = actual - expected.numpy()
    export_report["max_abs_difference"] = float(np.abs(difference).max())
    export_report["relative_error"] = float(np.linalg.norm(difference) / np.linalg.norm(expected.numpy()))
    export_report["onnxruntime_seconds_per_chunk"] = second_seconds
    export_report["onnxruntime_first_run_seconds"] = first_seconds
    export_report["chunk_seconds"] = int(config["audio"]["chunk_size"]) / 44100
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    export_report["peak_rss_gb_including_torch_and_onnxruntime"] = (peak if sys.platform == "darwin" else peak * 1024) / 1e9
    print(json.dumps(export_report, indent=2))


if __name__ == "__main__":
    main()
