---
title: Architecture
description: How the Euphoria code base is organized.
---

# Architecture

Euphoria is a Cargo workspace. All product logic lives in one library crate, and every front end is a thin crate on top of it. `apps/` holds what people run, and `crates/` holds the libraries the apps share and the developer tools built on them.

```
apps/
  euphoria-cli/        the `euphoria` command line tool
  euphoria-desktop/    the desktop app
crates/
  euphoria-core/       the library: decoding, detection, energy, library scanning, the analysis cache
  euphoria-bench/      accuracy benchmark, not shipped to users
bench/              dataset download script, Essentia reference script, results
```

A new app gets its own folder in `apps/`. Logic that two apps need moves into a library in `crates/`, never into one app for the other to depend on.

![Three crates, euphoria-cli and euphoria-desktop in apps/ and euphoria-bench in crates/, each depend on euphoria-core and none on another. euphoria-core holds every module of product logic, grouped as finding and decoding files, analyzing them, and keeping and sharing the results, and builds on matrixmultiply for the drone-v2 key network, stratum-dsp for tempo, Symphonia for decoding, rusqlite and blake3 for the cache, and ebur128 and rustfft for loudness and spectra.](../images/architecture/crates.svg)

## Rules between crates

- `euphoria-core` depends on no front end. It contains no command line parsing, no user interface code, and prints nothing.
- Front ends depend on `euphoria-core` and never on each other. They translate input into core calls and core results into output.
- A feature is built in the core first, then exposed by each front end, so the CLI and the app always give the same answer.

## euphoria-core

| Module | Responsibility |
| --- | --- |
| `library` | Expands files and folders into the audio files to analyze, skipping hidden files. |
| `analysis` | Analyzes one file, or a batch in parallel with progress callbacks and one result per file. The `_observed` variants also tell an `AnalysisObserver` the stage each file is in and how far decoding has come; the plain functions pass `Unobserved`, whose methods do nothing. |
| `cache` | Remembers analyses in a SQLite database keyed by a hash of each file's content, and analyzes only the files it does not know, as a batch or one file at a time. |
| `audio` | Decodes MP3, AAC and ALAC in MP4, FLAC, Ogg Vorbis, WAV and AIFF with Symphonia and mixes them to mono. |
| `audio_stream` | Decodes the same formats a packet at a time with their channels kept, with seeking, for playback. |
| `key` | The `Key` type, Camelot and Open Key codes, and parsing of key names and codes. |
| `harmony` | How two keys relate (`Relation`) and the ranking of the tracks that mix into another (`mixes_into`). |
| `detect` | The `KeyDetector` and `TempoDetector` traits and their implementations, the tuning measurement and the key over time. |
| `confidence` | The `Confidence` levels of a key or tempo and the margins they are set from. |
| `tempo_range` | `TempoRange`, the range tempos are reported in, and the half or double tempo offered as an alternative. |
| `energy` | The energy rating from 1 to 10, the features behind it and the energy curve. |
| `track_name` | Reads the title and artist stored in an audio file. |
| `track_cues` | The cues of a track as a DJ uses them, with names and hot cue pads, and `TrackCues::of`, which gives the user's edited set when there is one and the automatic cues otherwise. The cache keeps edited sets in their own table, like corrections. |
| `export` | Builds a library from analysis results, mirrors folders as a playlist tree, and writes it for other DJ software, starting with rekordbox XML. |
| `error` | `EuphoriaError`, the single error type returned by the library. |

### Detectors

Key and tempo detection sit behind traits so implementations can be compared and replaced without touching callers.

- `DroneDetector`, behind the `drone` feature (on by default), runs drone-v2, a convolutional network of 109,128 parameters, in plain Rust with no inference library. Its weights, `models/drone-v2.safetensors`, are compiled in with `include_bytes!`. The front end in `src/detect/drone/` brings decoded audio to 22,050 Hz and computes the constant-Q spectrogram drone was trained on, in segments so memory does not grow with the length of the track; the network multiplies matrices with the pure Rust `matrixmultiply` crate. It reports a probability for every key, so its runner up and margin come for free. The network averages its features over time before one linear layer, so it also scores every column of about 1.5 seconds; `src/detect/drone/timeline.rs` follows the key over time from windows of five columns. A track measured at least 20 cents off concert pitch is first brought back to it, by telling the resampler a sample rate that moves every note by the offset. Audio shorter than the 1.5 seconds the network reads at least is read followed by silence.
- `note_spectrum` measures how strongly each of the 72 notes from C1 to B6 sounds in a track, on the scale of its measured tuning, after taking off the level that sounds without pitch, and folds it into the 12 notes of the pitch-class profile. It is pure Rust, on `rustfft`, and runs in every build.
- `measure_tuning` is pure Rust and measures how far a track is tuned from A = 440 Hz. It runs in every build.
- `StratumDetector` uses the pure Rust `stratum-dsp` crate for both key and tempo. Its tempo detection runs only the tempogram estimate, on at most the middle ten minutes of a track, reports the result between 90 and 180 BPM, and costs a fraction of its key detection.

`detect::read_key` measures the tuning and has drone-v2 decide the key and follow it over time. The cache records the detector that decided each key, since each detector's confidence has its own scale; analyses stored by versions that used libkeyfinder say so and are rated on its scale. Tempo comes from `stratum-dsp`, whose key analysis runs only in builds without the `drone` feature. Those builds report the tuning without correcting for it, and no key over time.

## euphoria-cli

Parses arguments with clap, runs `cache::analyze_files_cached` (or `analysis::analyze_files` with `--no-cache`) on a Rayon pool sized by `--jobs`, shows a line per file with its stage under a bar that counts files on a terminal (`stages.rs`), and formats the results: the table that fits the terminal and the detailed view (`view.rs`), the plain table and JSON for pipes, and every message in one `error:`, `warning:`, `hint:` shape (`output.rs`). `style.rs` decides when to colour, from `--color`, `NO_COLOR`, `CLICOLOR_FORCE` and whether the stream is a terminal. `analyze` and `export` share that batch code in `batch.rs`. `export rekordbox` passes the results to `export::assemble_library` and `export::rekordbox::write_xml_file`. The crate contains no analysis or export logic.

## euphoria-desktop

The `euphoria-desktop` package lives in `apps/euphoria-desktop` and builds the `euphoria-desktop` executable. The CLI package remains `euphoria-cli` and builds the `euphoria` command.

The desktop app, built with GPUI and the gpui-component library. It asks the core for the audio files under each added folder, fills in the ones `cache::CachedAnalyzer` already knows, analyzes the rest through it on a Rayon pool of at most 4 workers so that each row fills in as its file finishes, and asks `harmony` which tracks mix with the selected one. To play a track it reads an `AudioStream` from the core a packet at a time, on a thread that decodes about a second ahead, and hands the decoded samples to the system audio output through rodio. The first sound never waits for the whole file, and the audio output never waits for the disk or the decoder. State that needs no window (the library, sorting and searching, progress, the player, waveform bands, settings, themes) is in plain modules with unit tests; the views only draw it. It contains no analysis logic and no harmonic rules.

## Detector choice

Which detector ships is decided by measurement. `euphoria-bench` scores every detector on the GiantSteps key and tempo datasets, and `bench/essentia_reference.py` scores Essentia on the same tracks as a reference point.

## Licensing

Euphoria is GPL-3.0-or-later. `deny.toml` lists the dependency licenses that are compatible, and CI rejects anything else.
