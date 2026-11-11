---
title: Building from source
description: Build, test and benchmark Euphoria from a source checkout.
---

# Building from source

## Requirements

- Rust 1.89 or newer
- CMake and pkg-config
- libkeyfinder

macOS:

```
brew install rust cmake libkeyfinder
```

Debian and Ubuntu do not package libkeyfinder, so a script builds it from source:

```
sudo apt-get install cargo cmake pkg-config libfftw3-dev
./.github/scripts/install-libkeyfinder.sh
```

## Build and run

```
git clone https://github.com/lvndry/euphoria.git
cd euphoria
cargo build --release
./target/release/euphoria analyze ~/Music
```

## Tests

```
cargo test --workspace
```

## Without libkeyfinder

libkeyfinder is a default Cargo feature of `euphoria-core`. Without it the core builds with no native libraries and uses stratum-dsp for key detection too, which is less accurate:

```
cargo test -p euphoria-core --no-default-features
```

## Accuracy benchmark

Download the datasets (about 1.7 GB, into `bench/data`, which git ignores) and run the benchmark:

```
./bench/download_datasets.sh
cargo run --release -p euphoria-bench
```

`--only key` or `--only tempo` runs one of the two benchmarks:

```
cargo run --release -p euphoria-bench -- bench/data --only tempo
```

An optional reference run scores Essentia on the same tracks. It needs [uv](https://docs.astral.sh/uv/):

```
uv run bench/essentia_reference.py bench/data
```

The benchmark is run by hand, not in CI.

## Energy benchmark

`euphoria-bench energy` scores the [energy rating](../concepts/energy.md) against reference ratings. It reads a labels file with one track per line: the path of an audio file, relative to the labels file, then a tab and a number where higher means more energy.

```
tracks/first.flac	3
tracks/second.mp3	7.5
```

The benchmark does not care where the numbers come from: ratings from listeners, ratings you gave by hand, or energy values that other software wrote. It never reads anything into file or folder names. A file that lists paths only, with no numbers, gives the distribution of the ratings and nothing else.

```
cargo run --release -p euphoria-bench -- energy path/to/labels.tsv
```

Each track is rated the way `euphoria analyze` rates it, with its detected tempo. The report has the Spearman correlation between rating and reference, the share of track pairs with different references that the rating puts in the right order, averages by reference, the histogram of the ratings, the same figures for every feature and for the tempo, and the CPU time per track. It holds aggregates only, never a track name or a path, so a private library can serve as reference data. Set `RAYON_NUM_THREADS` to limit the number of tracks analyzed at once.

Next to the shipped rating, the report scores a fitted model, an alternative with weights fitted to listener ratings that Euphoria does not ship. It is defined in `crates/euphoria-bench/src/alternative.rs`, and [the results](https://github.com/lvndry/euphoria/blob/main/bench/results/energy.md) explain why it is not the rating.

### Reference data

The public reference is DEAM, 1802 songs with arousal ratings from listeners. Its terms of use are Creative Commons BY-NC, free for non profit work, so it is downloaded by whoever runs the benchmark and never stored in the repository, and no weights that Euphoria ships are fitted on it. It is not a dance music dataset, which [the results](https://github.com/lvndry/euphoria/blob/main/bench/results/energy.md) discuss. Download it (about 1.3 GB, into `bench/data/energy`) after the GiantSteps datasets:

```
./bench/download_datasets.sh
./bench/download_energy_dataset.sh
```

The script writes these labels files into `bench/data/energy`:

| File | Tracks |
| --- | --- |
| `deam/labels-tuning.tsv` | The 1216 songs that models are fitted on. |
| `deam/labels-held-out.tsv` | The 586 songs that models are judged on. They take no part in fitting. |
| `deam/labels-electronic-tuning.tsv`, `deam/labels-electronic-held-out.tsv` | The songs tagged electronic, in the same split. |
| `giantsteps.tsv` | The GiantSteps Key previews, without references, to see how ratings of dance music are distributed. |

```
cargo run --release -p euphoria-bench -- energy bench/data/energy/deam/labels-held-out.tsv
```

### Fitting the model

`--dump` writes one line per track with its reference, its rating, its features and its tempo, and no track name. `bench/fit_energy_model.py` reads these files and prints anchors and weights for a model of the same form as `ENERGY_MODEL` in `crates/euphoria-core/src/energy/model.rs`. This is how the fitted model of the benchmark was made. It needs [uv](https://docs.astral.sh/uv/):

```
cargo run --release -p euphoria-bench -- energy bench/data/energy/deam/labels-tuning.tsv --dump tuning.tsv
cargo run --release -p euphoria-bench -- energy bench/data/energy/deam/labels-held-out.tsv --dump held-out.tsv
cargo run --release -p euphoria-bench -- energy bench/data/energy/giantsteps.tsv --dump giantsteps.tsv
uv run bench/fit_energy_model.py --tuning tuning.tsv --held-out held-out.tsv --spread giantsteps.tsv
```

A feature has to be measured by `euphoria-core` before it can be fitted: add it to `EnergyFeatures` and its `named` list, run the dumps again, and keep it only if the script selects it.

The fitter excludes features with missing values in tuning or spread (including undetected tempos), and features whose low and high anchors are equal. Reports omit tracks missing a reference or a selected feature and show how many were omitted. Held-out data does not decide which features are used. A fit without useful features, positive weights or a usable output range stops with a diagnostic instead of emitting an invalid model.

Run the fitting regression tests with:

```
uv run --with numpy --with scipy python -m unittest discover -s bench -p 'test_*.py'
```

Changing the shipped rating changes every user's ratings, so it needs evidence on the music Euphoria is for: reference ratings of dance music in a labels file, with a model that is clearly better on tracks held out from fitting. A change to `ENERGY_MODEL` raises `ENERGY_MODEL_VERSION`, which makes cached ratings be computed again, and records the before and after in `bench/results/energy.md`.
