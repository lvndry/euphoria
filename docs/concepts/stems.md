---
title: Stems
description: What stem separation is, how Euphoria separates a track into vocals, drums, bass and the rest, how close it comes to the reference model, what it costs in time and memory, and where it falls short.
---

# Stems

A stem is one part of a mix: the vocals, the drums, the bass, or everything else. Euphoria separates a finished track into those four stems, so you can mute the vocals for an instrumental, solo the drums to check a groove, or take an acapella into a mix.

```
euphoria stems track.mp3 --output stems
```

This writes `vocals.wav`, `drums.wav`, `bass.wav` and `other.wav` into `stems/`. In the desktop app, the track detail has a button that separates the selected track, and the player switches between the full track, the acapella and the instrumental. The [command reference](../reference/cli.md#euphoria-stems) lists the options.

## How it works

Euphoria runs [Demucs](https://github.com/adefossez/demucs) htdemucs, the hybrid transformer model by Rouard, Massa and Défossez, on your computer through ONNX Runtime. Nothing is uploaded.

1. The track is decoded to stereo at 44.1 kHz, the rate the model was trained at.
2. It is cut into segments of 7.8 seconds that overlap by a quarter.
3. Each segment goes through the network, which looks at it twice, as a spectrogram and as a waveform, and returns four stems.
4. The segments are blended back with triangular weights, so that no seam is heard where they meet, and each blended stretch is written to disk as soon as no later segment overlaps it.

The model's weights, 84 MB, are downloaded once from the Demucs author's published files, pinned to one revision and checked against their SHA-256. Their origin and licence are recorded in [the model's provenance notes](https://github.com/lvndry/euphoria/blob/main/crates/euphoria-core/models/README.md).

Stems are saved by the content of the file, so separating the same track again is instant, even after it is renamed or moved. The saved stems take at most 5 GB by default, after which the stems used longest ago are removed first.

## How close it is to Demucs

Euphoria's stems were compared with the reference Python Demucs package (version 4.1.0, the same weights, without random shifts, the setting Euphoria matches) on a 60 second excerpt of ["Let's Pretend"](https://archive.org/details/josh-woodward-lets-pretend) by Josh Woodward, licensed CC BY 4.0, from the Internet Archive. The comparison is made by [`scripts/stems/compare_with_demucs.py`](https://github.com/lvndry/euphoria/blob/main/scripts/stems/compare_with_demucs.py).

| Stem | SDR against Demucs | Correlation |
| --- | --- | --- |
| Drums | 76.8 dB | 1.000 |
| Bass | 80.6 dB | 1.000 |
| Other | 76.7 dB | 1.000 |
| Vocals | 82.3 dB | 1.000 |

An SDR (signal to distortion ratio) near 80 dB means the difference from Demucs is about 80 dB below the stem itself, around the level of the 16 bit rounding of the WAV files, so Euphoria gives the same stems as Demucs.

The song is also published without its vocals, so the mix minus the instrumental gives a rough reference for the vocals. Against it both Euphoria and Demucs score 8.4 dB SDR. This is the model's own quality on one song, not a benchmark, and the reference is rough because the two files are separate MP3 encodings.

## Requirements and cost

Measured with [`scripts/stems/measure_cost.py`](https://github.com/lvndry/euphoria/blob/main/scripts/stems/measure_cost.py) on a 6 minute track (two copies of the song above), on a MacBook with an Apple M5 (10 cores) and 24 GB of memory, shared with other work while it ran.

| Threads | Time per minute of audio | Processor time per minute of audio | Peak memory |
| --- | --- | --- | --- |
| 9 (default: every core but one) | 18 to 25 s | 72 to 75 s | 2.9 GB |
| 4 | 22 s | 54 s | 3.1 GB |
| 2 | 30 s | 44 s | 3.1 GB |

- **Memory does not grow with the track.** Stems are written to disk segment by segment, so a 60 second clip peaked at 3.1 GB and the 6 minute track at about 3 GB. Almost all of it is ONNX Runtime working on one 7.8 second segment.
- **The app stays responsive.** One core is left free by default. While a separation ran, a test thread asking to wake every 5 ms was late by at most 12 ms (5 ms in 99 cases out of 100), the same as with no separation running. Set `EUPHORIA_STEMS_THREADS` to use fewer cores.
- **One separation at a time.** A second separation, from another window or command, waits for the first instead of loading the model twice.
- **Disk:** the model is 84 MB; stems take about 42 MB per minute of audio (four 16 bit stereo WAV files at 44.1 kHz), so the 5 GB cache holds about two hours of separated music.

### On an 8 GB laptop

Separation needs about 3 GB of free memory on top of what is already open. On an 8 GB Mac with a browser and DJ software open it will still finish, but macOS will compress or swap other apps' memory while it runs, so close what you can, separate tracks while preparing rather than during a set, and let it run one track at a time. Expect roughly 30 seconds per minute of audio on a recent Apple Silicon laptop; this estimate is not measured on such a machine.

### Other computers

- Intel Macs and Windows: no release is built for them yet, and stems need ONNX Runtime builds that Euphoria does not have for Intel Macs.
- Linux (64-bit Intel or AMD): stems are built in and tested in CI. Speed on an older processor has not been measured; expect it to be several times slower than above, since separation is processor bound.

## Limits

- **Separation is never perfect.** Expect a little bleed: a hi-hat in the vocals, a breath in the drums, reverb tails that land in the wrong stem. It is worst where instruments share a range, such as a synth bass under a kick, or backing vocals inside a pad.
- **Four stems only.** Guitar, piano, synths and everything else that is not vocals, drums or bass all land in "other".
- **The stems add up to the track only approximately.** Played together they sound like the original, with small differences where the model was unsure.
- **44.1 kHz, 16 bit.** Stems are written at the model's rate, whatever the rate of the file. A sample beyond full scale, which is rare, is clipped.
- **No random shifts.** The Demucs command line can average several separations of the track shifted in time, which gains a little quality for several times the work. Euphoria separates once.
- **The weights' licence is not settled.** The Demucs code is MIT licensed. Its author notes that the licence of the trained weights is unclear, because part of the training data was published for research only. Euphoria downloads them from the author's own repository rather than shipping them.
