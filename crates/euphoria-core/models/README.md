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
