---
title: Stems
description: What stem separation is, how Euphoria separates a track into vocals, drums, bass and the rest, how close it comes to the reference model, what it costs in time and memory, and where it falls short.
---

# Stems

A stem is one part of a mix: the vocals, the drums, the bass, or everything else. Euphoria separates a finished track into those four stems, so you can mute the vocals for an instrumental, solo the drums to check a groove, or take an acapella into a mix.

```
euphoria stems track.mp3 --output stems
```

This writes `vocals.wav`, `drums.wav`, `bass.wav` and `other.wav` into `stems/`. In the desktop app, the track detail has a button that separates the selected track, the player switches between the full track, the acapella and the instrumental, and the stems, the acapella and the instrumental can be saved as WAV files. The [command reference](../reference/cli.md#euphoria-stems) lists the options.

## How it works

Euphoria runs [Demucs](https://github.com/adefossez/demucs) htdemucs, the hybrid transformer model by Rouard, Massa and Défossez, on your computer through ONNX Runtime. Nothing is uploaded.

1. The track is decoded to stereo at 44.1 kHz, the rate the model was trained at.
2. It is cut into segments of 7.8 seconds that overlap by a quarter.
3. Each segment goes through the network, which looks at it twice, as a spectrogram and as a waveform, and returns four stems.
4. The segments are blended back with triangular weights, so that no seam is heard where they meet, and each blended stretch is written to disk as soon as no later segment overlaps it.

The model's weights, 84 MB, are downloaded once from the Demucs author's published files, pinned to one revision and checked against their SHA-256. Their origin and licence are recorded in [the model's provenance notes](https://github.com/lvndry/euphoria/blob/main/crates/euphoria-core/models/README.md).

Stems are saved by the content of the file, so separating the same track again is instant, even after it is renamed or moved. The saved stems take at most 5 GB by default, after which the stems used longest ago are removed first.

## How the app plays stems

Once a track is separated, the track detail offers **Full track**, **Acapella** and **Instrumental**.

- **Full track** plays the file itself, exactly as before the separation, at its own sample rate and with its own channels. The stems are not used, so nothing of the separation can be heard in it.
- **Acapella** plays the vocals stem. **Instrumental** plays the drums, bass and other stems added together. The stems are read side by side, in step, and mixed as 32 bit floating point samples, so adding them never clips or rounds. They are played at 44.1 kHz, the rate they were written at, and the system's audio output converts that to the rate of the device, as it does for any file.
- The position is kept in seconds, so a seek or a switch between the three lands at the same moment of the track whatever its sample rate. Switching between Acapella and Instrumental fades the stems in and out without a restart; switching to or from Full track starts the file or the stems again from the current position.
- A key shift applies to the stems as it does to the file.

Acapella and Instrumental added together give back the track, apart from the model's own mistakes: on generated test stems at 44.1 kHz, 48 kHz and in mono, their sum matches the track to about 76 dB, the limit of rounding each stem to 16 bits, and lasts as long as the track to within a millisecond.

**Remove stems**, under the three choices, deletes the saved stems of the track. The track detail then offers to separate it again. Stems are also removed on their own when the saved stems pass the size limit below.

## Saving stems

Once a track is separated, the track detail offers **Export stems…**, **Export acapella…** and **Export instrumental…**.

**Export stems…** opens a folder picker and writes the four stems into the chosen folder as 16 bit stereo WAV files at 44.1 kHz, one per stem, named after the track's title, which is the title in the tags or the file name without its extension: `<title> - vocals.wav`, `<title> - drums.wav`, `<title> - bass.wav` and `<title> - other.wav`. Characters file systems refuse ( / \ : * ? " < > | ) become underscores. The files are the separated stems exactly, written by the same code as `euphoria stems`: all four are staged first, and if any fails the others are removed and a file that was replaced is restored. If any of the four names is already taken in the folder, nothing is written and a strip at the bottom of the window asks `<file> and N more files already exist. Replace them?` with **Replace** and **Cancel**.

**Export acapella…** and **Export instrumental…** open a save dialog, which asks about replacing a file itself. The default names are `<title> (Acapella).wav` and `<title> (Instrumental).wav`. The acapella is the vocals stem as it is. The instrumental is the drums, bass and other stems added together in 32 bit floating point, and written out as a 16 bit stereo WAV file at 44.1 kHz. When the sum goes past full scale it is scaled down as a whole so its peak is exactly full scale, and the strip at the bottom says "The sum would have clipped, so it was scaled down by X dB".

The exports run off the main thread. The strip at the bottom shows "Saving …" while one runs, and then what was saved, or why it failed.

When the track has a key and the key shift in the **Change key** section is not zero, a second row of three buttons appears: **Export stems in <new key>…**, **Export acapella in <new key>…** and **Export instrumental in <new key>…**. Each stem is shifted by the same pitch shifter as "Export shifted copy…" (tempo unchanged, length unchanged), so the file is what is heard with the shift on, and the instrumental is summed after the shift. The files are tagged with the new key, and keep the tempo and energy, like the shifted copy. The names carry the shift: `<title> - vocals (+2).wav`, `<title> (Acapella, +2).wav`.

The command line writes a single file the same way: `euphoria stems <FILE> -o <DIR> --mix acapella` or `--mix instrumental` writes the acapella or the instrumental into the folder instead of the four stems; see the [command reference](../reference/cli.md#euphoria-stems).

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

Measured with [`scripts/stems/measure_cost.py`](https://github.com/lvndry/euphoria/blob/main/scripts/stems/measure_cost.py) on a 6 minute track (two copies of the song above), on a MacBook with an Apple M5 (10 cores) and 24 GB of memory. Before and after are the same runs back to back; before is the graph that computed all eight attention heads at once, after is the current graph, which computes them one at a time.

| Threads | Graph | Time per minute of audio | Processor time per minute of audio | Peak memory |
| --- | --- | --- | --- | --- |
| 9 (default: every core but one) | before | 17.2 s | 72 s | 2.99 GB |
| 9 (default: every core but one) | after | 15.4 s | 66 s | 2.53 GB |
| 2 | before | 19.8 s | 34 s | 2.86 GB |
| 2 | after | 18.9 s | 31 s | 2.48 GB |

The stems are the same either way: against Demucs both graphs score the SDR shown above on every stem.

- **Memory does not grow with the track.** Stems are written to disk segment by segment, so a 60 second clip peaked at 2.4 GB and the 6 minute track at 2.5 GB. Almost all of it is ONNX Runtime working on one 7.8 second segment and the model's weights.
- **The app stays responsive.** One core is left free by default. While a separation ran, a test thread asking to wake every 5 ms was late by at most 12 ms (5 ms in 99 cases out of 100), the same as with no separation running. Set `EUPHORIA_STEMS_THREADS` to use fewer cores.
- **One separation at a time.** A second separation, from another window or command, waits for the first instead of loading the model twice.
- **Disk:** the model is 84 MB; stems take about 42 MB per minute of audio (four 16 bit stereo WAV files at 44.1 kHz), so the 5 GB cache holds about two hours of separated music.

### On an 8 GB laptop

Separation needs about 2.5 GB of free memory on top of what is already open. On an 8 GB Mac with a browser and DJ software open it will still finish, but macOS will compress or swap other apps' memory while it runs, so close what you can, separate tracks while preparing rather than during a set, and let it run one track at a time. Expect roughly 20 to 30 seconds per minute of audio on a recent Apple Silicon laptop; this estimate is not measured on such a machine.

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
