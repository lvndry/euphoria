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
