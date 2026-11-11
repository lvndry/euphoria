# Audio quality results

Measured on 11 November 2026 on an Apple M5 (10 cores), macOS 27.2, by `uv run bench/quality_benchmark.py bench/data/quality`, with FFmpeg 8 (LAME and FFmpeg's AAC encoder) and Apple's AAC encoder through `afconvert`. The thresholds in `crates/euphoria-core/src/quality/mod.rs` come from this table; [audio quality](../../docs/concepts/audio-quality.md) explains them.

## Sources

- Six tracks from *Lithium Toolbox* by John Lithium (Argali Records, ARGREC14), 16 bit 44.1 kHz WAV files from the Internet Archive under Creative Commons BY-SA 3.0: electronic music, 2.5 to 4 minutes each. None of them shows a cutoff, so they were not made from lossy files.
- Three synthesized tracks of 60 seconds: a dance loop with a kick, noise hats, a saw chord and noise, which fills the spectrum to 22 kHz; the same loop through a gentle one pole lowpass at 3 kHz (a dark track with no cliff in its spectrum); and the loop driven 9 dB into hard clipping.

Each source was encoded at 96, 128, 192, 256 and 320 kbit/s with each encoder, stored losslessly as FLAC, and transcoded: lossy files at a low bit rate were decoded and encoded again at a high one.

## Encodings of music

The six tracks and the wideband synthesized loop, seven files per row. The bit rate is the median of the measured averages. The cutoff is the lowest and the highest found, and "full band" counts the files with no cutoff. The error is the median difference between the decoded file and the source below 15 kHz, relative to the music, after the encoder's delay is removed. The verdicts use the thresholds as they are now.

| Encoding | Bit rate (kbit/s) | Cutoff, lowest to highest (kHz) | Full band | Error below 15 kHz (dB) | Verdicts |
| --- | --- | --- | --- | --- | --- |
| lossless source | 1411 | none | 7 of 7 | - | Lossless 7 |
| flac | 387 | none | 7 of 7 | - | Lossless 7 |
| lame 96 | 96 | 15.2 to 15.4 | 0 of 7 | -24.1 | Weak on a big system 7 |
| lame 128 | 128 | 16.0 to 16.8 | 0 of 7 | -25.6 | Weak on a big system 7 |
| lame 192 | 192 | 18.6 to 18.9 | 0 of 7 | -30.3 | Acceptable 7 |
| lame 256 | 256 | 19.3 to 19.6 | 0 of 7 | -46.7 | Good 7 |
| lame 320 | 320 | 20.1 to 20.3 | 0 of 7 | -52.1 | Good 7 |
| ffmpeg-aac 96 | 97 | 15.9 to 16.7 | 0 of 7 | -25.5 | Weak on a big system 7 |
| ffmpeg-aac 128 | 129 | 17.3 to 17.3 | 0 of 7 | -31.0 | Weak on a big system 7 |
| ffmpeg-aac 192 | 194 | 19.3 to 19.4 | 0 of 7 | -36.0 | Acceptable 5, Good 2 |
| ffmpeg-aac 256 | 257 | none | 7 of 7 | -42.4 | Good 7 |
| ffmpeg-aac 320 | 283 | none | 7 of 7 | -44.1 | Good 7 |
| apple-aac 96 | 92 | 15.6 to 15.6 | 1 of 7 | -29.4 | Weak on a big system 7 |
| apple-aac 128 | 120 | 17.1 to 18.7 | 2 of 7 | -32.6 | Weak on a big system 7 |
| apple-aac 192 | 176 | 16.4 to 19.7 | 2 of 7 | -39.7 | Acceptable 7 |
| apple-aac 256 | 245 | 18.6 to 18.6 | 6 of 7 | -42.2 | Good 7 |
| apple-aac 320 | 317 | none | 7 of 7 | -43.4 | Good 7 |
| lame 128 to lame 320 | 320 | 16.0 to 16.8 | 0 of 7 | -25.6 | Likely re-encoded 7 |
| lame 128 to flac | 1918 | 16.0 to 16.8 | 0 of 7 | -25.6 | Likely re-encoded 7 |
| lame 192 to lame 320 | 320 | 18.6 to 18.9 | 1 of 7 | -30.3 | Likely re-encoded 6, Good 1 |
| ffmpeg-aac 128 to ffmpeg-aac 256 | 223 | 17.2 to 17.3 | 0 of 7 | -30.9 | Likely re-encoded 7 |
| lame 128 to apple-aac 256 | 245 | 16.0 to 16.9 | 0 of 7 | -25.5 | Likely re-encoded 7 |
| apple-aac 128 to apple-aac 320 | 309 | 17.1 to 18.5 | 2 of 7 | -32.5 | Likely re-encoded 5, Good 2 |

What the table shows:

- Each encoder keeps a cutoff that rises with the bit rate. LAME's is the most regular. Apple's encoder decides per track: at 128 and 192 kbit/s it kept the whole spectrum for some tracks, and at 192 kbit/s it cut one track at 16.4 kHz.
- The measured average bit rate of a variable bit rate file can sit far below its setting: Apple's encoder set to 256 kbit/s averaged 208 kbit/s on the quietest track, and FFmpeg's set to 320 averaged 283.
- The encoding error falls with the bit rate, with the largest step between 192 and 256 kbit/s for LAME and between 128 and 192 kbit/s for the AAC encoders.
- A transcode keeps the cutoff of its source. Of the 42 transcodes of music, 39 are recognised. The 3 that are not show no clear fall after re-encoding: twice because Apple's encoder had kept the whole spectrum at 128 kbit/s, once because LAME at 320 kbit/s put content back above the 192 kbit/s cutoff.

## Control material

The dark loop and the clipped loop, per encoding: the cutoff in kHz, the share of clipped samples, and the verdict.

| Encoding | Dark synth | Clipped synth |
| --- | --- | --- |
| lossless source | none, 0.00%, Lossless | none, 0.96%, Lossless |
| flac | none, 0.00%, Lossless | none, 0.96%, Lossless |
| lame 96 | 15.4, 0.00%, Weak on a big system | 15.4, 0.03%, Weak on a big system |
| lame 128 | 16.8, 0.00%, Weak on a big system | 16.9, 0.00%, Weak on a big system |
| lame 192 | 18.9, 0.00%, Acceptable | 18.9, 0.01%, Acceptable |
| lame 256 | 19.5, 0.00%, Good | 19.6, 0.09%, Good |
| lame 320 | 20.3, 0.00%, Good | 20.3, 0.08%, Good |
| ffmpeg-aac 96 | 16.7, 0.00%, Weak on a big system | 16.7, 0.05%, Weak on a big system |
| ffmpeg-aac 128 | 17.3, 0.00%, Weak on a big system | 17.4, 0.04%, Weak on a big system |
| ffmpeg-aac 192 | 19.4, 0.00%, Good | 19.4, 0.04%, Good |
| ffmpeg-aac 256 | none, 0.00%, Good | none, 0.11%, Good |
| ffmpeg-aac 320 | none, 0.00%, Good | none, 0.09%, Good |
| apple-aac 96 | 15.6, 0.00%, Weak on a big system | 15.6, 0.19%, Weak on a big system |
| apple-aac 128 | 18.7, 0.00%, Weak on a big system | 18.7, 0.13%, Weak on a big system |
| apple-aac 192 | 19.6, 0.00%, Acceptable | 19.6, 0.09%, Acceptable |
| apple-aac 256 | none, 0.00%, Good | none, 0.10%, Good |
| apple-aac 320 | none, 0.00%, Good | none, 0.11%, Good |
| lame 128 to lame 320 | 16.8, 0.00%, Likely re-encoded | 16.9, 0.00%, Likely re-encoded |
| lame 128 to flac | 16.8, 0.00%, Likely re-encoded | none, 0.00%, Lossless |
| lame 192 to lame 320 | 18.9, 0.00%, Likely re-encoded | 18.9, 0.01%, Likely re-encoded |
| ffmpeg-aac 128 to ffmpeg-aac 256 | 17.3, 0.00%, Likely re-encoded | 17.3, 0.01%, Likely re-encoded |
| lame 128 to apple-aac 256 | 16.9, 0.00%, Likely re-encoded | none, 0.00%, Good |
| apple-aac 128 to apple-aac 320 | 18.7, 0.00%, Likely re-encoded | 18.6, 0.13%, Likely re-encoded |

What the controls show:

- The dark loop, which has no content above a few kilohertz worth speaking of but no cliff either, is never given a cutoff as a lossless file. Its encodings get the cutoff of their encoder.
- Hard clipping is plain in a lossless file (0.96% of samples) and mostly hidden after lossy encoding (0% to 0.19%). No other file measured shows any clipping.
- Converting a decoded lossy file to 16 bit clips its overshoots. For the clipped loop that put new content above the cutoff, so its lossless and Apple AAC transcodes are not recognised.

## Cost

Measured on 30 tracks of a DJ library (AAC, 6.6 minutes on average) and 30 GiantSteps previews (MP3, 2 minutes), `analyze_file` on main against `analyze_file` with the quality report, one after the other on the same tracks:

| Tracks | Before | With quality | Increase |
| --- | --- | --- | --- |
| Full length AAC | 3.49 CPU s | 3.77 CPU s | 8% |
| Two minute MP3 previews | 1.03 CPU s | 1.13 CPU s | 9% |

The report reuses the decode and the loudness that the energy rating measures. A first version that measured loudness and true peak with the `ebur128` crate on every channel cost three times as much.

## Not measured

- Encoders other than LAME, FFmpeg's AAC encoder and Apple's AAC encoder, such as Fraunhofer's or those of stores and streaming services.
- Sample rates other than 44.1 kHz.
- Whether listeners hear the difference between the verdicts on a club system. The error column is a plain measure of how much the encoder changed, not a listening test.
