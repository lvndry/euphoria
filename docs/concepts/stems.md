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

QUALITY_SECTION

## Requirements and cost

COST_SECTION

## Limits

- **Separation is never perfect.** Expect a little bleed: a hi-hat in the vocals, a breath in the drums, reverb tails that land in the wrong stem. It is worst where instruments share a range, such as a synth bass under a kick, or backing vocals inside a pad.
- **Four stems only.** Guitar, piano, synths and everything else that is not vocals, drums or bass all land in "other".
- **The stems add up to the track only approximately.** Played together they sound like the original, with small differences where the model was unsure.
- **44.1 kHz, 16 bit.** Stems are written at the model's rate, whatever the rate of the file. A sample beyond full scale, which is rare, is clipped.
- **No random shifts.** The Demucs command line can average several separations of the track shifted in time, which gains a little quality for several times the work. Euphoria separates once.
- **The weights' licence is not settled.** The Demucs code is MIT licensed. Its author notes that the licence of the trained weights is unclear, because part of the training data was published for research only. Euphoria downloads them from the author's own repository rather than shipping them.
