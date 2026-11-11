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

The report has the Spearman correlation between rating and reference, the share of track pairs with different references that the rating puts in the right order, averages by reference, the histogram of the ratings, the same figures for every feature, and the CPU time per track. It holds aggregates only, never a track name or a path, so a private library can serve as reference data. Set `RAYON_NUM_THREADS` to limit the number of tracks analyzed at once.

### Reference data

The public reference is DEAM, 1802 songs with arousal ratings from listeners, free for non profit work under Creative Commons BY-NC. It is not a dance music dataset, which [the results](https://github.com/lvndry/euphoria/blob/main/bench/results/energy.md) discuss. Download it (about 1.3 GB, into `bench/data/energy`) after the GiantSteps datasets:

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

`--dump` writes one line per track with its reference, its rating and its features, and no track name. `bench/fit_energy_model.py` reads these files and prints the anchors and weights that go into `ENERGY_MODEL` in `crates/euphoria-core/src/energy/model.rs`. It needs [uv](https://docs.astral.sh/uv/):

```
cargo run --release -p euphoria-bench -- energy bench/data/energy/deam/labels-tuning.tsv --dump tuning.tsv
cargo run --release -p euphoria-bench -- energy bench/data/energy/deam/labels-held-out.tsv --dump held-out.tsv
cargo run --release -p euphoria-bench -- energy bench/data/energy/giantsteps.tsv --dump giantsteps.tsv
uv run bench/fit_energy_model.py --tuning tuning.tsv --held-out held-out.tsv --spread giantsteps.tsv
```

A feature has to be measured by `euphoria-core` before it can be fitted: add it to `EnergyFeatures` and its `named` list, run the dumps again, and keep it only if the script selects it. After a change to the model, raise `ENERGY_MODEL_VERSION`, run the benchmark on the held out songs, and record the before and after in `bench/results/energy.md`.
