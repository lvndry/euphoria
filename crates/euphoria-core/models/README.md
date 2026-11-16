# HTDemucs model provenance

`htdemucs.onnx` contains the network graph without learned weights. It is
embedded in the binary and exported by
[`export_htdemucs_onnx.py`](../../../scripts/stems/export_htdemucs_onnx.py).
Its SHA-256 is `b69d128b976899d71a1f3293130adafb6ed0b2b25adc02804105ef6819e1147d`.
The export computes the cross transformer's attention one head at a
time, which gives the same output up to float rounding and needs less
memory; two exports with the pinned environment below give this exact
file, and a test checks the embedded graph against this hash.

The graph and Rust transforms are adapted from
[Demucs](https://github.com/adefossez/demucs). Its code is MIT licensed; the
complete notice and the distinction below are distributed in
[`demucs-NOTICE.txt`](../../../licenses/demucs-NOTICE.txt).
The recorded export environment used Demucs 4.1.0, PyTorch 2.15.0, ONNX 1.23.2,
NumPy 2.5.4, safetensors 0.8.0 and soundfile 0.14.0. These direct dependencies
are pinned in [`requirements.txt`](../../../scripts/stems/requirements.txt).
The original Demucs installation has no recorded VCS commit; no commit is
inferred from the release version.

The separately downloaded weights are `955717e8.safetensors` from
[adefossez/HTDemucs at revision cbc8a9b1a87023b7fd74e7b3412e6321c0eab003](https://huggingface.co/adefossez/HTDemucs/tree/cbc8a9b1a87023b7fd74e7b3412e6321c0eab003).
The expected size is 84,025,440 bytes and the SHA-256 is
`d9fa14133cfcc034a6758923bb3a8ca9f8dfd0b582134643bbf83f72c17576dd`.
The downloader verifies that hash on every load.

The code's MIT license does **not** establish a license for the pretrained
weights. [Upstream describes their license as unclear because of the training
data](https://github.com/adefossez/demucs#license); the pinned Hugging Face model
card declares no separate license. Euphoria does not redistribute the weights
or claim a license grant for them.

Inference runs in segments of 7.8 seconds and writes the stems to disk
as each segment is blended, so memory does not grow with the length of
the track. Quality against the reference Python Demucs, speed and peak
memory are measured in [the stems page](../../../docs/concepts/stems.md).

# SCNet XL model provenance

`scnet-xl.onnx` contains the network graph of the Advanced stems mode without
learned weights. It is embedded in the binary and exported by
[`export_scnet_xl_onnx.py`](../../../scripts/stems/export_scnet_xl_onnx.py)
for a segment of 242,550 samples (5.5 seconds at 44.1 kHz): the input is the
spectrum `[1, 4, 2049, 238]` and the output `[1, 4, 4, 2049, 238]`. Its SHA-256
is `44989b3f92a8db183e2ab9ca29340c0d578963fa4f4ce203765c4eb582e5ad18`. Two
exports with the pinned environment give this exact file, and a test checks the
embedded graph against this hash.

The network is SCNet, adapted from
[Music-Source-Separation-Training](https://github.com/ZFTurbo/Music-Source-Separation-Training)
at commit `1f700c386e0dd763fe0a57242bb9420dce987354`, whose model code comes from
[SCNet](https://github.com/starrytong/SCNet). Both are MIT licensed; the complete
notice and the distinction below are distributed in
[`scnet-NOTICE.txt`](../../../licenses/scnet-NOTICE.txt). The export replaces
the real FFT along time in the separation network by the equivalent
multiplication by cosine and sine matrices, because ONNX has no such operator
and a segment always has the same number of frames; the replaced network matches
the original to 2e-7 and the graph with its weights to 2e-3 on a random segment.
The recorded export environment is the one for the htdemucs graph, plus ONNX
Runtime 1.32.0 and PyYAML 6.0.3, pinned in
[`requirements.txt`](../../../scripts/stems/requirements.txt).

The separately downloaded weights are `model_scnet_ep_36_sdr_10.0891.ckpt` from
[the v1.0.15 release of Music-Source-Separation-Training](https://github.com/ZFTurbo/Music-Source-Separation-Training/releases/tag/v1.0.15).
The expected size is 214,063,778 bytes and the SHA-256 is
`ac25975f0f5704f3d1a3c3c251505b7a0f417a22eafe82773440ee4f7e14b74f`. The
downloader verifies that hash on every load. The file is a PyTorch checkpoint:
Euphoria reads its 510 tensors with a reader that never runs anything in the
file (it accepts three named pickle globals, float32 tensors and contiguous
layouts, and nothing else).

The code's MIT license does **not** establish a license for the pretrained
weights. Their publisher states none, and the SCNet authors say their own
checkpoint was trained on the MUSDB dataset, whose audio is licensed for
research use. Euphoria does not redistribute the weights or claim a license
grant for them. Because the release is hosted by a third party, the download
fails, and the Advanced mode is unavailable, if that file is ever removed.

Segments of 5.5 seconds overlap by a quarter and are blended with triangular
weights, and the stems are written to disk as each segment is blended, so memory
does not grow with the length of the track. The audio is not normalised before
the network, which is how this checkpoint was configured (`normalize: false`).

# drone-v2 model provenance

The weights of drone-v2, the key detection network that `euphoria-core` runs
with its `drone` feature. They are compiled into the library with
`include_bytes!`, so nothing is downloaded at run time.

| | |
| --- | --- |
| File | `drone-v2.safetensors`, 441,456 bytes |
| SHA-256 | `f3badc870fd1f8c79fc0cabb4e1819d55e38a3b724ea0158a63d5b781765af97` |
| Source checkpoint | `drone-v2.pt` from [lvndry/drone](https://github.com/lvndry/drone) and [Hugging Face](https://huggingface.co/lvndry/drone), SHA-256 `f069fc37004062e5f7ac931dcf5f7414b2da805eadaa2c1b28b82f32eef6acb5` |
| Training run | Seed 42 of drone's shift12 recipe, 120 epochs, checkpoint chosen at epoch 88 on a held-back tenth of the training data |
| Contracts | feature spec `cqt24-8oct-c1-pad4-hop4096-v2`, label schema `tonic-times-two-plus-minor-v1` (both stored in the file's metadata and checked when it loads) |
| Parameters | 109,128, float32, plus the BatchNorm running statistics |
| Licence | MIT, as the drone code and model card state |

The file is drone's own release artifact, made from the checkpoint with its
exporter, which copies every tensor unchanged:

```bash
uv run python -m drone.safetensors weights/drone-v2.pt drone-v2.safetensors
```

To check a copy: `shasum -a 256 drone-v2.safetensors`.

## Training data

The network was trained on 4,988 songs of many genres from FMAKv2 (Free Music
Archive clips with key labels, CC BY 4.0) and 1,275 Beatport dance-music
previews from GiantSteps-MTG-Key, half from their labels and half from the
predictions of three earlier networks, with every excerpt shifted by up to 12
semitones. The audio is not distributed, here or by drone. No track of the
GiantSteps Key dataset (604 previews) was used to train the model; GiantSteps
results did guide drone's development. How it was trained and chosen is in
drone's [frontier study](https://github.com/lvndry/drone/blob/main/experiments/studies/frontier-distillation/REPORT.md)
and [release evaluation](https://github.com/lvndry/drone/blob/main/experiments/studies/drone-v2-release/REPORT.md).
