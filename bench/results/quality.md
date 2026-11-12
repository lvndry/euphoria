# Audio quality results

Measured on 11 November 2026 on an Apple M5 (10 cores), macOS 27.2, by `uv run bench/quality_benchmark.py bench/data/quality`, with FFmpeg 8 (LAME and FFmpeg's AAC encoder) and Apple's AAC encoder through `afconvert`. The thresholds in `crates/euphoria-core/src/quality/mod.rs` come from this table; [audio quality](../../docs/concepts/audio-quality.md) explains them.

The cutoffs below combine the power of each channel before any mixdown, so opposite-phase stereo content cannot cancel. The thresholds were first set with the spectrum of the mono mixdown; the same encoded files were measured again with the per-channel method, with the thresholds unchanged. [Mono mixdown against per-channel cutoffs](#mono-mixdown-against-per-channel-cutoffs) lists what changed.

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
| lame 192 | 192 | 18.6 to 18.9 | 1 of 7 | -30.3 | Acceptable 7 |
| lame 256 | 256 | 19.3 to 19.6 | 0 of 7 | -46.7 | Good 7 |
| lame 320 | 320 | 20.1 to 20.3 | 0 of 7 | -52.1 | Good 7 |
| ffmpeg-aac 96 | 97 | 15.9 to 16.7 | 0 of 7 | -25.5 | Weak on a big system 7 |
| ffmpeg-aac 128 | 129 | 17.3 to 17.3 | 0 of 7 | -31.0 | Weak on a big system 7 |
| ffmpeg-aac 192 | 194 | 19.3 to 19.4 | 0 of 7 | -36.0 | Acceptable 5, Good 2 |
| ffmpeg-aac 256 | 257 | none | 7 of 7 | -42.4 | Good 7 |
| ffmpeg-aac 320 | 283 | none | 7 of 7 | -44.1 | Good 7 |
| apple-aac 96 | 92 | 15.6 to 15.8 | 1 of 7 | -29.4 | Weak on a big system 7 |
| apple-aac 128 | 120 | 17.3 to 18.5 | 3 of 7 | -32.6 | Weak on a big system 7 |
| apple-aac 192 | 176 | 19.4 to 19.7 | 3 of 7 | -39.7 | Acceptable 7 |
| apple-aac 256 | 245 | none | 7 of 7 | -42.2 | Good 7 |
| apple-aac 320 | 317 | none | 7 of 7 | -43.4 | Good 7 |
| lame 128 to lame 320 | 320 | 16.0 to 16.8 | 0 of 7 | -25.6 | Likely re-encoded 7 |
| lame 128 to flac | 1918 | 16.0 to 16.8 | 0 of 7 | -25.6 | Likely re-encoded 7 |
| lame 192 to lame 320 | 320 | 18.6 to 18.9 | 1 of 7 | -30.3 | Likely re-encoded 6, Good 1 |
| ffmpeg-aac 128 to ffmpeg-aac 256 | 223 | 17.2 to 17.3 | 0 of 7 | -30.9 | Likely re-encoded 7 |
| lame 128 to apple-aac 256 | 245 | 16.0 to 16.8 | 0 of 7 | -25.5 | Likely re-encoded 7 |
| apple-aac 128 to apple-aac 320 | 309 | 17.3 to 18.5 | 3 of 7 | -32.5 | Likely re-encoded 4, Good 3 |

What the table shows:

- Each encoder keeps a cutoff that rises with the bit rate. LAME's is the most regular. Apple's encoder decides per track: at 128 and 192 kbit/s it kept the whole spectrum for some tracks.
- The measured average bit rate of a variable bit rate file can sit far below its setting: Apple's encoder set to 256 kbit/s averaged 208 kbit/s on the quietest track, and FFmpeg's set to 320 averaged 283.
- The encoding error falls with the bit rate, with the largest step between 192 and 256 kbit/s for LAME and between 128 and 192 kbit/s for the AAC encoders.
- A transcode keeps the cutoff of its source. Of the 42 transcodes of music, 38 are recognised. The 4 that are not show no clear fall after re-encoding: three times because Apple's encoder had kept the whole spectrum at 128 kbit/s, once because LAME at 320 kbit/s put content back above the 192 kbit/s cutoff.

## Control material

The dark loop and the clipped loop, per encoding: the cutoff in kHz, the share of clipped samples, and the verdict.

| Encoding | Dark synth | Clipped synth |
| --- | --- | --- |
| lossless source | none, 0.00%, Lossless | none, 0.96%, Lossless |
| flac | none, 0.00%, Lossless | none, 0.96%, Lossless |
| lame 96 | 15.4, 0.00%, Weak on a big system | 15.4, 0.03%, Weak on a big system |
| lame 128 | 16.8, 0.00%, Weak on a big system | 16.8, 0.00%, Weak on a big system |
| lame 192 | 18.9, 0.00%, Acceptable | 18.9, 0.01%, Acceptable |
| lame 256 | 19.5, 0.00%, Good | 19.6, 0.09%, Good |
| lame 320 | 20.3, 0.00%, Good | 20.3, 0.08%, Good |
| ffmpeg-aac 96 | 16.7, 0.00%, Weak on a big system | 16.7, 0.05%, Weak on a big system |
| ffmpeg-aac 128 | 17.3, 0.00%, Weak on a big system | 17.3, 0.04%, Weak on a big system |
| ffmpeg-aac 192 | 19.4, 0.00%, Good | 19.4, 0.04%, Good |
| ffmpeg-aac 256 | none, 0.00%, Good | none, 0.11%, Good |
| ffmpeg-aac 320 | none, 0.00%, Good | none, 0.09%, Good |
| apple-aac 96 | 15.8, 0.00%, Weak on a big system | 15.8, 0.19%, Weak on a big system |
| apple-aac 128 | 18.7, 0.00%, Weak on a big system | 18.5, 0.13%, Weak on a big system |
| apple-aac 192 | 19.6, 0.00%, Acceptable | 19.6, 0.09%, Acceptable |
| apple-aac 256 | none, 0.00%, Good | none, 0.10%, Good |
| apple-aac 320 | none, 0.00%, Good | none, 0.11%, Good |
| lame 128 to lame 320 | 16.8, 0.00%, Likely re-encoded | 16.8, 0.00%, Likely re-encoded |
| lame 128 to flac | 16.8, 0.00%, Likely re-encoded | 16.7, 0.00%, Likely re-encoded |
| lame 192 to lame 320 | 18.9, 0.00%, Likely re-encoded | 18.9, 0.01%, Likely re-encoded |
| ffmpeg-aac 128 to ffmpeg-aac 256 | 17.3, 0.00%, Likely re-encoded | 17.3, 0.01%, Likely re-encoded |
| lame 128 to apple-aac 256 | 16.7, 0.00%, Likely re-encoded | 16.7, 0.00%, Likely re-encoded |
| apple-aac 128 to apple-aac 320 | 18.5, 0.00%, Likely re-encoded | 18.5, 0.13%, Likely re-encoded |

What the controls show:

- The dark loop, which has no content above a few kilohertz worth speaking of but no cliff either, is never given a cutoff as a lossless file. Its encodings get the cutoff of their encoder.
- Hard clipping is plain in a lossless file (0.96% of samples) and mostly hidden after lossy encoding (0% to 0.19%). No other file measured shows any clipping.
- Converting a decoded lossy file to 16 bit clips its overshoots. Every transcode of both loops is recognised.

## Cost

The first version, with the cutoff on the mono mixdown, was measured on 30 tracks of a DJ library (AAC, 6.6 minutes on average) and 30 GiantSteps previews (MP3, 2 minutes), `analyze_file` on main against `analyze_file` with the quality report, one after the other on the same tracks:

| Tracks | Before | With quality | Increase |
| --- | --- | --- | --- |
| Full length AAC | 3.49 CPU s | 3.77 CPU s | 8% |
| Two minute MP3 previews | 1.03 CPU s | 1.13 CPU s | 9% |

The report reuses the decode and the loudness that the energy rating measures. A first version that measured loudness and true peak with the `ebur128` crate on every channel cost three times as much.

### Cost of the per-channel cutoff

Measured on the same Apple M5 and macOS 27.2 with release builds of `euphoria analyze --json --no-cache` on 12 full length tracks of this benchmark (the six *Lithium Toolbox* tracks as Apple AAC 256 and as LAME 320), process CPU time, median of 11 runs after one warmup, the four builds taking turns:

| Build | CPU s for 12 tracks | Against no quality report |
| --- | --- | --- |
| No quality report (`c6c4f6b`) | 18.03 | |
| Cutoff on the mono mixdown (`19759cc`) | 21.05 | +16.8% |
| Cutoff per channel, one transform per channel (`7e370e1`) | 21.89 | +21.4% |
| Cutoff per channel, two channels per transform | 21.26 | +17.9% |

A complex transform of two real channels, one in the real part and one in the imaginary part, gives the sum of their powers from each bin and its mirror, so a stereo file costs one transform per frame, as the mono mixdown did. Its cutoffs and verdicts are the same as with one transform per channel on all 207 files. The whole report costs more on these tracks than on the library above; the runs vary by about 1 CPU s, so differences under 3% between builds are within noise.

## Mono mixdown against per-channel cutoffs

The same 207 encoded files, measured with the cutoff on the mono mixdown and with the cutoff per channel. Of the 139 files with a cutoff both times, 49 moved, by at most 172 Hz. These changed whether a cutoff was found:

| Encoding | Track | Bit rate (kbit/s) | Mono mixdown | Per channel |
| --- | --- | --- | --- | --- |
| lame 192 | Lithium Toolbox 3 | 192 | 18.6 kHz, Acceptable | none, Acceptable |
| apple-aac 192 | Lithium Toolbox 6 | 155 | 16.4 kHz, Acceptable | none, Acceptable |
| apple-aac 128 | Lithium Toolbox 14 | 120 | 17.1 kHz, Weak on a big system | none, Weak on a big system |
| apple-aac 256 | Lithium Toolbox 14 | 238 | 18.6 kHz, Good | none, Good |
| apple-aac 128 to apple-aac 320 | Lithium Toolbox 14 | 297 | 17.1 kHz, Likely re-encoded | none, Good |
| lame 128 to flac | clipped synth | 2232 | none, Lossless | 16.7 kHz, Likely re-encoded |
| lame 128 to apple-aac 256 | clipped synth | 250 | none, Good | 16.7 kHz, Likely re-encoded |

- The synthesized loops have their right channel 7 samples behind the left. Their mono mixdown is a comb filter with a notch at 15.75 kHz, just under the 16.7 kHz cutoff of a 128 kbit/s MP3, and the notch hid the cutoff of two transcodes. Measured per channel, both are found.
- In five encodings of music the mixdown fell steeply where the separate channels did not: content above the fall was out of phase between the channels and cancelled. None of them was a lossless or direct encode wrongly called likely re-encoded, but one transcode was caught only because of that cancellation, so 38 of 42 music transcodes are now recognised instead of 39.
- No file encoded once from a lossless source is called likely re-encoded with either method.

## Ogg Vorbis

Measured on 11 November 2026 on the same Apple M5 and macOS 27.2, by `uv run bench/quality_benchmark.py bench/data/quality lame oggenc`, with `oggenc` from vorbis-tools 1.4.3 (libvorbis 1.3.7). People encode Vorbis at a quality level rather than a bit rate, so each row is the quality level whose nominal bit rate is in its name: quality 2 for 96, 4 for 128, 6 for 192, 8 for 256 and 9 for 320. LAME was measured again alongside and matched the table above. The same seven music files per row as above:

| Encoding | Bit rate (kbit/s) | Cutoff, lowest to highest (kHz) | Full band | Error below 15 kHz (dB) | Verdicts with the MP3 and AAC lines | Verdicts with the Vorbis lines |
| --- | --- | --- | --- | --- | --- | --- |
| oggenc 96 (quality 2) | 71 | 16.7 to 16.9 | 0 of 7 | -21.8 | Weak on a big system 7 | Weak on a big system 7 |
| oggenc 128 (quality 4) | 89 | 17.1 to 19.7 | 0 of 7 | -23.7 | Weak on a big system 7 | Weak on a big system 6, Acceptable 1 |
| oggenc 192 (quality 6) | 131 | 18.9 to 20.6 | 4 of 7 | -28.9 | Weak on a big system 5, Acceptable 1, Good 1 | Acceptable 5, Good 2 |
| oggenc 256 (quality 8) | 175 | 19.6 to 20.8 | 5 of 7 | -31.6 | Acceptable 5, Good 2 | Good 7 |
| oggenc 320 (quality 9) | 231 | 20.8 to 20.8 | 6 of 7 | -32.6 | Good 7 | Good 7 |
| oggenc 128 to oggenc 320 | 224 | 17.4 to 19.7 | 0 of 7 | -23.1 | Good 6, Acceptable 1 | Likely re-encoded 3, Good 4 |
| oggenc 128 to flac | 1001 | 17.0 to 19.7 | 0 of 7 | -23.7 | Lossless 4, Likely re-encoded 3 | Lossless 4, Likely re-encoded 3 |
| lame 128 to oggenc 320 | 228 | 16.7 to 18.4 | 0 of 7 | -24.9 | Likely re-encoded 6, Good 1 | Likely re-encoded 7 |

The bit rates of the music files alone, lowest to highest: quality 4 measured 69 to 104 kbit/s, quality 6 116 to 162, quality 8 148 to 221, quality 9 201 to 273. The synthesized loops ran higher at every level.

What the table shows:

- Vorbis averages far below the nominal rate of its quality level, about 70% of it on this music. Judged by the MP3 and AAC lines, five of seven quality 6 files, which is the level most people use, came out weak on a big system, the verdict of a 128 kbit/s MP3.
- The reference encoder keeps a higher cutoff than LAME at each level, and from quality 6 often none at all. Its lowest cutoff was 18.9 kHz at quality 6 and 19.6 kHz at quality 8.
- A Vorbis file made from a 128 kbit/s MP3 keeps the MP3's cutoff, 16.7 to 18.4 kHz, and is recognised. A Vorbis file made from a low quality Vorbis file is caught only when the first encoding cut low, because the encoder cuts high to begin with.

So Vorbis has lines of its own in `crates/euphoria-core/src/quality/mod.rs`:

- **Good** from 145 kbit/s: every quality 8 file (148 and up) and the busiest quality 6 file.
- **Acceptable** from 110 kbit/s: every quality 6 file (116 and up), and no quality 4 track of music (104 at most); the wideband loop at quality 4 measured 122.
- **Likely re-encoded** when the cutoff is below 18.5 kHz from 145 kbit/s, or below 18 kHz from 110 kbit/s: under the lowest cutoff of a direct encoding at those rates, with a margin.
- A lossless file made from Vorbis is judged by the lossless line, 19 kHz, which catches it only when the Vorbis file cut lower than that.

The error column falls more slowly with the level than LAME's and stays above LAME 256's. It is a plain measure of change, not of what is heard, and Vorbis shapes its noise differently, so the lines follow what each quality level measures rather than this column.

## Not measured

- Encoders other than LAME, FFmpeg's AAC encoder, Apple's AAC encoder and the reference Vorbis encoder, such as Fraunhofer's, aoTuV's Vorbis encoder, or those of stores and streaming services.
- Vorbis set to a bit rate rather than a quality level, and Opus.
- Sample rates other than 44.1 kHz.
- Whether listeners hear the difference between the verdicts on a club system. The error column is a plain measure of how much the encoder changed, not a listening test.
