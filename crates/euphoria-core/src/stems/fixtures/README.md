<!-- Copyright 2026 The Euphoria contributors -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

`zero_stems.onnx` is a synthetic, weight-free ONNX fixture for exercising the
real separation and cache path without downloading or running Demucs. It has
Demucs' two input shapes and returns correctly shaped zero frequency/time
branches. Its two nodes are `ConstantOfShape`, with INT64 shape initializers
`[1, 4, 4, 2048, 336]` and `[1, 4, 2, 343980]`. It uses opset 17 and IR version 8.
