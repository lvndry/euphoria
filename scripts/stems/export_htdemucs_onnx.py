"""Export the htdemucs network to an ONNX graph that holds no weights.

The graph covers what HTDemucs.forward does between the spectrogram and
the inverse spectrogram: normalisation, the two encoders, the cross
transformer, the two decoders and the denormalisation. The short-time
Fourier transforms and the overlap-add of segments stay in Rust, because
ONNX has no complex tensors.

Every learned parameter is left out of the file as an external
initializer named after its key in the official safetensors file, so the
committed graph is small and the weights come only from the pinned
download.

Run with the packages from scripts/stems/requirements.txt:

    python scripts/stems/export_htdemucs_onnx.py \
        --weights 955717e8.safetensors \
        --output crates/euphoria-core/models/htdemucs.onnx
"""

import argparse
from pathlib import Path

import onnx
import torch
from demucs.hf import load_safetensors_model
from demucs.htdemucs import HTDemucs
from onnx import numpy_helper
from safetensors import safe_open

EXTERNAL_LOCATION = "htdemucs.weights"
WRAPPER_PREFIX = "model."
SEGMENT_SAMPLES = 343980


class SeparationNetwork(torch.nn.Module):
    """HTDemucs.forward from the normalised inputs to the denormalised branches."""

    def __init__(self, model: HTDemucs) -> None:
        super().__init__()
        self.model = model

    def forward(self, spectrogram: torch.Tensor, waveform: torch.Tensor):
        model = self.model
        batch, _, frequencies, frames = spectrogram.shape
        mean = spectrogram.mean(dim=(1, 2, 3), keepdim=True)
        std = spectrogram.std(dim=(1, 2, 3), keepdim=True)
        frequency_input = (spectrogram - mean) / (1e-5 + std)
        mean_time = waveform.mean(dim=(1, 2), keepdim=True)
        std_time = waveform.std(dim=(1, 2), keepdim=True)
        time_input = (waveform - mean_time) / (1e-5 + std_time)

        frequency_branch = frequency_input
        time_branch = time_input
        saved = []
        saved_time = []
        lengths = []
        lengths_time = []
        for index, encode in enumerate(model.encoder):
            lengths.append(frequency_branch.shape[-1])
            inject = None
            if index < len(model.tencoder):
                lengths_time.append(time_branch.shape[-1])
                time_encoder = model.tencoder[index]
                time_branch = time_encoder(time_branch)
                if not time_encoder.empty:
                    saved_time.append(time_branch)
                else:
                    inject = time_branch
            frequency_branch = encode(frequency_branch, inject)
            if index == 0 and model.freq_emb is not None:
                positions = torch.arange(frequency_branch.shape[-2])
                embedding = model.freq_emb(positions).t()[None, :, :, None]
                frequency_branch = frequency_branch + model.freq_emb_scale * embedding.expand_as(
                    frequency_branch
                )
            saved.append(frequency_branch)

        if model.bottom_channels:
            _, channels_bottom, height, width = frequency_branch.shape
            frequency_branch = frequency_branch.reshape(batch, channels_bottom, height * width)
            frequency_branch = model.channel_upsampler(frequency_branch)
            frequency_branch = frequency_branch.reshape(batch, -1, height, width)
            time_branch = model.channel_upsampler_t(time_branch)
        frequency_branch, time_branch = model.crosstransformer(frequency_branch, time_branch)
        if model.bottom_channels:
            _, channels_bottom, height, width = frequency_branch.shape
            frequency_branch = frequency_branch.reshape(batch, channels_bottom, height * width)
            frequency_branch = model.channel_downsampler(frequency_branch)
            frequency_branch = frequency_branch.reshape(batch, -1, height, width)
            time_branch = model.channel_downsampler_t(time_branch)

        for index, decode in enumerate(model.decoder):
            skip = saved.pop(-1)
            frequency_branch, before_last = decode(frequency_branch, skip, lengths.pop(-1))
            offset = model.depth - len(model.tdecoder)
            if index >= offset:
                time_decoder = model.tdecoder[index - offset]
                length_time = lengths_time.pop(-1)
                if time_decoder.empty:
                    before_last = before_last[:, :, 0]
                    time_branch, _ = time_decoder(before_last, None, length_time)
                else:
                    skip = saved_time.pop(-1)
                    time_branch, _ = time_decoder(time_branch, skip, length_time)

        sources = len(model.sources)
        frequency_branch = frequency_branch.view(batch, sources, -1, frequencies, frames)
        frequency_branch = frequency_branch * std[:, None] + mean[:, None]
        time_branch = time_branch.view(batch, sources, -1, waveform.shape[-1])
        time_branch = time_branch * std_time[:, None] + mean_time[:, None]
        return frequency_branch, time_branch


def reference_forward(model: HTDemucs, waveform: torch.Tensor):
    spectrum = model._spec(waveform)
    spectrogram = model._magnitude(spectrum)
    return spectrogram, model(waveform)


def externalize(graph_path: Path, weights_path: Path) -> None:
    model_proto = onnx.load(str(graph_path))
    with safe_open(str(weights_path), "pt") as weights:
        keys = set(weights.keys())
        shapes = {key: list(weights.get_slice(key).get_shape()) for key in keys}
    renamed = {}
    for initializer in model_proto.graph.initializer:
        key = initializer.name.removeprefix(WRAPPER_PREFIX)
        if key in keys and key != initializer.name:
            renamed[initializer.name] = key
            initializer.name = key
    for node in model_proto.graph.node:
        for index, name in enumerate(node.input):
            if name in renamed:
                node.input[index] = renamed[name]
    external = []
    kept = []
    for initializer in model_proto.graph.initializer:
        if initializer.name in keys:
            if list(initializer.dims) != shapes[initializer.name]:
                raise SystemExit(f"shape mismatch for {initializer.name}")
            if initializer.data_type != onnx.TensorProto.FLOAT:
                raise SystemExit(f"{initializer.name} is not float32")
            initializer.ClearField("raw_data")
            initializer.ClearField("float_data")
            initializer.data_location = onnx.TensorProto.EXTERNAL
            del initializer.external_data[:]
            entry = initializer.external_data.add()
            entry.key = "location"
            entry.value = EXTERNAL_LOCATION
            external.append(initializer.name)
        else:
            kept.append((initializer.name, numpy_helper.to_array(initializer).nbytes))
    missing = sorted(keys - set(external))
    if missing:
        raise SystemExit(f"weights not used by the graph: {missing[:10]}")
    onnx.save(model_proto, str(graph_path))
    kept_bytes = sum(size for _, size in kept)
    print(f"{len(external)} external initializers, {len(kept)} constants kept ({kept_bytes} bytes)")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--weights", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()

    model = load_safetensors_model(arguments.weights)
    model.eval()
    model.float()
    network = SeparationNetwork(model)
    network.eval()

    generator = torch.Generator().manual_seed(7)
    waveform = torch.randn(1, 2, SEGMENT_SAMPLES, generator=generator) * 0.1
    with torch.no_grad():
        spectrogram, expected = reference_forward(model, waveform)
        frequency_branch, time_branch = network(spectrogram, waveform)
        spectrum = model._mask(None, frequency_branch)
        rebuilt = model._ispec(spectrum, SEGMENT_SAMPLES) + time_branch
    difference = (rebuilt - expected).abs().max().item()
    print(f"split network against HTDemucs.forward: max difference {difference:.3e}")
    if difference > 1e-4:
        raise SystemExit("the split network does not reproduce HTDemucs.forward")

    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    torch.backends.mha.set_fastpath_enabled(False)
    with torch.no_grad():
        torch.onnx.export(
            network,
            (spectrogram, waveform),
            str(arguments.output),
            input_names=["spectrogram", "waveform"],
            output_names=["frequency_branch", "time_branch"],
            opset_version=17,
            dynamo=False,
            do_constant_folding=False,
            export_params=True,
            keep_initializers_as_inputs=False,
        )
    externalize(arguments.output, arguments.weights)
    print(f"graph written to {arguments.output} ({arguments.output.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
