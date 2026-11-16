"""Export the SCNet XL IHF network to an ONNX graph that holds no weights.

The graph covers what SCNet.forward does between the short-time Fourier
transform and the inverse transform: the sub-band encoders, the dual-path
separation network and the decoders. The transforms and the overlap-add of
chunks stay in Rust, because ONNX has no complex tensors.

Every learned parameter is left out of the file as an external initializer
named after its key in the official checkpoint, so the committed graph is
small and the weights come only from the pinned download.

The dual-path network applies a real FFT along time. ONNX cannot export
that operator, and a chunk has a fixed number of frames, so the export
replaces it with the equivalent multiplication by cosine and sine matrices
(they stay in the graph as small constants).

Needs a checkout of https://github.com/ZFTurbo/Music-Source-Separation-Training
for the model definition, and the packages from scripts/stems/requirements.txt
plus PyYAML and onnxruntime:

    python scripts/stems/export_scnet_xl_onnx.py \
        --source-directory path/to/Music-Source-Separation-Training \
        --config config_musdb18_scnet_xl_more_wide_v5.yaml \
        --checkpoint model_scnet_ep_36_sdr_10.0891.ckpt \
        --output crates/euphoria-core/models/scnet-xl.onnx
"""

import argparse
import collections
import hashlib
import sys
from pathlib import Path

import numpy
import onnx
import onnxruntime
import torch
import yaml
from onnx import numpy_helper

EXTERNAL_LOCATION = "scnet-xl.weights"
WRAPPER_PREFIX = "network."
CHUNK_SAMPLES = 242550


class SeparationNetwork(torch.nn.Module):
    """SCNet.forward from the spectrum to the per-source spectra.

    The input has shape [1, 4, bins, frames]: the real and imaginary parts
    of the left and the right channel. The output has shape
    [1, sources, 4, bins, frames] in the same layout."""

    def __init__(self, network: torch.nn.Module) -> None:
        super().__init__()
        self.network = network

    def forward(self, spectrum: torch.Tensor) -> torch.Tensor:
        network = self.network
        batch, _, bins, frames = spectrum.shape
        skips = collections.deque()
        lengths = collections.deque()
        original_lengths = collections.deque()
        features = spectrum
        for encoder_layer in network.encoder:
            features, skip, layer_lengths, layer_original_lengths = encoder_layer(features)
            skips.append(skip)
            lengths.append(layer_lengths)
            original_lengths.append(layer_original_lengths)
        features = network.separation_net(features)
        for fusion_layer, upsample_layer in network.decoder:
            features = fusion_layer(features, skips.pop())
            features = upsample_layer(features, lengths.pop(), original_lengths.pop())
        return features.view(batch, network.dims[0], -1, bins, frames)


def padding_for(network: torch.nn.Module, samples: int) -> int:
    """The zeros SCNet.forward appends to a chunk of `samples`."""
    padding = network.hop_length - samples % network.hop_length
    if (samples + padding) // network.hop_length % 2 == 0:
        padding += network.hop_length
    return padding


def to_spectrum(network: torch.nn.Module, chunk: torch.Tensor):
    """The model input for `chunk` of shape [1, 2, samples], and its padding."""
    padding = padding_for(network, chunk.shape[-1])
    padded = torch.nn.functional.pad(chunk, (0, padding))
    flat = padded.reshape(-1, padded.shape[-1])
    spectrum = torch.view_as_real(torch.stft(flat, **network.stft_config, return_complex=True))
    return spectrum.permute(0, 3, 1, 2).reshape(chunk.shape[0], 4, spectrum.shape[1], spectrum.shape[2]), padding


def from_spectrum(network: torch.nn.Module, features: torch.Tensor, padding: int) -> torch.Tensor:
    """The waveform of every source from the model output."""
    batch, sources, _, bins, frames = features.shape
    pairs = features.reshape(-1, 2, bins, frames).permute(0, 2, 3, 1)
    waveform = torch.istft(torch.view_as_complex(pairs.contiguous()), **network.stft_config)
    return waveform.reshape(batch, sources, network.audio_channels, -1)[:, :, :, :-padding]


def real_dft_matrices(length: int):
    """The forward and inverse real DFT with orthonormal scaling as matrices."""
    positions = torch.arange(length, dtype=torch.float64)
    bins = torch.arange(length // 2 + 1, dtype=torch.float64)
    angles = 2 * torch.pi * bins[:, None] * positions[None, :] / length
    scale = 1.0 / length**0.5
    forward_cosine = (torch.cos(angles) * scale).T.float()
    forward_sine = (-torch.sin(angles) * scale).T.float()
    weights = torch.full((length // 2 + 1,), 2.0, dtype=torch.float64)
    weights[0] = 1.0
    if length % 2 == 0:
        weights[-1] = 1.0
    inverse_cosine = (weights[:, None] * torch.cos(angles) * scale).float()
    inverse_sine = (-weights[:, None] * torch.sin(angles) * scale).float()
    return forward_cosine, forward_sine, inverse_cosine, inverse_sine


def replace_fft_with_matrices(feature_conversion_class, network: torch.nn.Module, frames: int) -> int:
    """Replaces the real FFT of every FeatureConversion by matrices. Returns how many."""
    forward_cosine, forward_sine, inverse_cosine, inverse_sine = real_dft_matrices(frames)

    def make_forward(module):
        def forward(features):
            if module.inverse:
                real = features[:, : module.channels // 2]
                imaginary = features[:, module.channels // 2 :]
                return real @ inverse_cosine + imaginary @ inverse_sine
            return torch.cat([features @ forward_cosine, features @ forward_sine], dim=1)

        return forward

    replaced = 0
    for module in network.modules():
        if isinstance(module, feature_conversion_class):
            module.forward = make_forward(module)
            replaced += 1
    return replaced


def externalize(graph_path: Path, weights: dict) -> list:
    """Leaves every weight out of the graph file, as an external initializer
    named after its key in the checkpoint. Returns the names, in graph order."""
    model_proto = onnx.load(str(graph_path))
    renamed = {}
    for initializer in model_proto.graph.initializer:
        key = initializer.name.removeprefix(WRAPPER_PREFIX)
        if key in weights and key != initializer.name:
            renamed[initializer.name] = key
            initializer.name = key
    for node in model_proto.graph.node:
        for index, name in enumerate(node.input):
            if name in renamed:
                node.input[index] = renamed[name]
    external = []
    kept_bytes = 0
    for initializer in model_proto.graph.initializer:
        if initializer.name in weights:
            if list(initializer.dims) != list(weights[initializer.name].shape):
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
            kept_bytes += numpy_helper.to_array(initializer).nbytes
    missing = sorted(set(weights) - set(external))
    if missing:
        raise SystemExit(f"weights not used by the graph: {missing[:10]} ({len(missing)} in all)")
    onnx.save(model_proto, str(graph_path))
    print(f"{len(external)} external initializers, constants kept: {kept_bytes} bytes")
    return external


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-directory", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--chunk-samples", type=int, default=CHUNK_SAMPLES)
    arguments = parser.parse_args()

    sys.path.insert(0, str(arguments.source_directory))
    from models.scnet.scnet import SCNet
    from models.scnet.separation import FeatureConversion

    config = yaml.load(arguments.config.read_text(), Loader=yaml.FullLoader)
    network = SCNet(**config["model"]).eval()
    weights = torch.load(arguments.checkpoint, map_location="cpu", weights_only=True)
    network.load_state_dict(weights)

    generator = torch.Generator().manual_seed(7)
    chunk = torch.randn(1, 2, arguments.chunk_samples, generator=generator) * 0.1
    with torch.no_grad():
        expected = network(chunk)
        spectrum, padding = to_spectrum(network, chunk)
        frames = spectrum.shape[-1]
        replaced = replace_fft_with_matrices(FeatureConversion, network, frames)
        separation = SeparationNetwork(network).eval()
        features = separation(spectrum)
        rebuilt = from_spectrum(network, features, padding)
    difference = (rebuilt - expected).abs().max().item()
    print(f"{replaced} FFTs replaced by matrices; split network against SCNet.forward: max difference {difference:.3e}")
    if difference > 1e-4:
        raise SystemExit("the split network does not reproduce SCNet.forward")

    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    with torch.no_grad():
        torch.onnx.export(
            separation,
            (spectrum,),
            str(arguments.output),
            input_names=["spectrum"],
            output_names=["features"],
            opset_version=17,
            dynamo=False,
            do_constant_folding=False,
            export_params=True,
            keep_initializers_as_inputs=False,
        )
    externalize(arguments.output, weights)

    options = onnxruntime.SessionOptions()
    names = list(weights)
    values = [onnxruntime.OrtValue.ortvalue_from_numpy(weights[name].numpy()) for name in names]
    options.add_external_initializers(names, values)
    session = onnxruntime.InferenceSession(str(arguments.output), options, providers=["CPUExecutionProvider"])
    from_graph = session.run(None, {"spectrum": spectrum.numpy()})[0]
    graph_difference = float(numpy.abs(from_graph - features.numpy()).max())
    print(f"graph with external weights against PyTorch: max difference {graph_difference:.3e}")
    if graph_difference > 5e-3:
        raise SystemExit("the exported graph does not reproduce the network")
    print(f"input shape {tuple(spectrum.shape)}, padding {padding}, graph {arguments.output.stat().st_size} bytes")
    print(f"graph SHA-256 {hashlib.sha256(arguments.output.read_bytes()).hexdigest()}")


if __name__ == "__main__":
    main()
