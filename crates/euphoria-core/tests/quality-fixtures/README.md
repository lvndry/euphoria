# Quality fixtures

Fixtures that prove a quality-report line end to end but are not part of the
ten-file scanned by the tests in `tests/decode.rs` and the CLI tests, which
hard-code the fixtures directory's count.

| File | Format | Provenance |
| --- | --- | --- |
| `sine-440hz-mono-alac-24.m4a` | ALAC in MP4, 24-bit | ffmpeg 9.0.2, `-c:a alac` from `../fixtures/sine-440hz-mono.wav` |

The 24-bit file is made from the 16-bit WAV source, not from a 24-bit source,
so the encoder adds no priming and the duration stays exactly one second
within the `tests/decode.rs` tolerance.
