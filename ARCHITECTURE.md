# Architecture

Euphoria is a Cargo workspace. All product logic lives in one library crate, and every front end is a thin crate on top of it.

```
crates/
  euphoria-core/    the library: decoding, detection, harmony
  euphoria-bench/   accuracy benchmark, not shipped to users
bench/              dataset download script, Essentia reference script, results
```

A command line tool (`euphoria-cli`) and a desktop app (`euphoria-app`) will join the workspace as separate crates.

## Rules between crates

- `euphoria-core` depends on no front end. It contains no command line parsing, no user interface code, and prints nothing.
- Front ends depend on `euphoria-core` and never on each other. They translate input into core calls and core results into output.
- A feature is built in the core first, then exposed by each front end, so the CLI and the app always give the same answer.

## euphoria-core

| Module | Responsibility |
| --- | --- |
| `audio` | Decodes MP3, M4A, FLAC, WAV and AIFF with Symphonia and mixes them to mono. |
| `key` | The `Key` type, Camelot codes and parsing of key names. |
| `detect` | The `KeyDetector` and `TempoDetector` traits and their implementations. |
| `export` | Writes analysis results for other DJ software, starting with rekordbox XML. |
| `error` | `EuphoriaError`, the single error type returned by the library. |

### Detectors

Key and tempo detection sit behind traits so implementations can be compared and replaced without touching callers.

- `KeyFinderDetector` calls libkeyfinder, a C++ library, through a small C wrapper in `src/detect/keyfinder_wrapper.cpp`. `build.rs` compiles the wrapper and finds libkeyfinder with pkg-config.
- `StratumDetector` uses the pure Rust `stratum-dsp` crate for both key and tempo.

Which detector ships is decided by measurement. `euphoria-bench` scores every detector on the GiantSteps key and tempo datasets, and `bench/essentia_reference.py` scores Essentia on the same tracks as a reference point.

## Licensing

libkeyfinder is GPL-3.0-or-later, so Euphoria is too. `deny.toml` lists the dependency licenses that are compatible, and CI rejects anything else.
