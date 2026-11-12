# HTDemucs model provenance

`htdemucs.onnx` contains the network graph without learned weights. It is
embedded in the binary and exported by
[`export_htdemucs_onnx.py`](../../../scripts/stems/export_htdemucs_onnx.py).
Its SHA-256 is `7f195e29fdf759bbe6b3a0301daa619e3c8f35f53498154865ed1bb98ebd7d86`.

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

Inference runs in segments, but the current API holds the decoded track,
normalised channels and all four stems in memory. It is not bounded to one
segment of working audio. End-to-end quality, speed and peak memory with the
pretrained weights have not been measured here.

# drone-v1 model provenance

The weights of drone-v1, the key detection network that `euphoria-core` runs
with its `drone` feature. They are compiled into the library with
`include_bytes!`, so nothing is downloaded at run time.

| | |
| --- | --- |
| File | `drone-v1.safetensors`, 441,456 bytes |
| SHA-256 | `aa64343b43e64bdd7b1fb5603cc3c90718d6d6354bd0ff293cf91fc940c43140` |
| Source checkpoint | `drone-v1.pt` from [lvndry/drone](https://github.com/lvndry/drone) and [Hugging Face](https://huggingface.co/lvndry/drone), SHA-256 `dcf8a7a292b7c12076ffff63c8c6550cb09a6c41ce4d000fd7949fc6bb4b452c` |
| Training run | `w1.0-seed13-20261111-185528-lr7fvv4k`: seed 13, 120 epochs, checkpoint chosen at epoch 39 on the validation split |
| Contracts | feature spec `cqt24-8oct-c1-pad4-hop4096-v2`, label schema `tonic-times-two-plus-minor-v1` (both stored in the file's metadata and checked when it loads) |
| Parameters | 109,128, float32, plus the BatchNorm running statistics |
| Licence | MIT, as the drone code and model card state |

The file was made from the checkpoint with drone's exporter, which copies
every tensor unchanged:

```bash
uv run python -m drone.safetensors weights/drone-v1.pt drone-v1.safetensors
```

To check a copy: `shasum -a 256 drone-v1.safetensors`.

## Training data

The network was trained on 1,237 two minute previews from the Beatport EDM
Key dataset and chosen on 126 more from it, whose annotations are published under CC BY-SA 4.0. The audio is
not distributed, here or by drone. The GiantSteps Key dataset (604 previews)
was held out for testing and never used to train or choose the model.
