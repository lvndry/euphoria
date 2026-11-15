# Quality fixtures

Fixtures that prove a quality-report line end to end but are not part of the
ten-file scanned by the tests in `tests/decode.rs` and the CLI tests, which
hard-code the fixtures directory's count.

| File | Format | Provenance |
| --- | --- | --- |
| `sine-440hz-mono-alac-24.m4a` | ALAC in MP4, 24-bit | ffmpeg 9.0.2, `-c:a alac` from `../fixtures/sine-440hz-mono.wav` |
| `noise-128k.mp3` | MP3, 128 kbit/s, stereo, 4 s of pink noise tagged "Harbour Lights", "Low Tide Dub" | ffmpeg 9.0.2 with LAME, the command below |

The 24-bit file is made from the 16-bit WAV source, not from a 24-bit source,
so the encoder adds no priming and the duration stays exactly one second
within the `tests/decode.rs` tolerance.

The MP3 is the low bit rate file of the `euphoria quality` tests in
`apps/euphoria-cli/tests/quality.rs`, which generate their lossless files.
Run this from this folder to regenerate it:

```sh
ffmpeg -f lavfi -i "anoisesrc=color=pink:sample_rate=44100:amplitude=0.3:seed=7:duration=4" -ac 2 -c:a libmp3lame -b:a 128k -metadata "artist=Harbour Lights" -metadata "title=Low Tide Dub" noise-128k.mp3
```
