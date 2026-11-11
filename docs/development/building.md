---
title: Building from source
description: Build, test and benchmark Euphoria from a source checkout.
---

# Building from source

## Requirements

- Rust 1.89 or newer
- CMake and pkg-config
- libkeyfinder
- FFTW, with its threads library, which the commands below install

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

`--tracks development` and `--tracks held-out` each score half of a dataset: every other track in name order, starting with the first and with the second. Choose a setting on the development half and check it on the held out half, so that it is not fitted to the tracks it is judged on:

```
cargo run --release -p euphoria-bench -- bench/data --only key --tracks held-out
```

The key benchmark compares every other key detector with the first, track by track, next to the percentages.

An optional reference run scores Essentia on the same tracks. It needs [uv](https://docs.astral.sh/uv/):

```
uv run bench/essentia_reference.py bench/data
```

The benchmark is run by hand, not in CI.

### Scoring rekordbox on the same previews

rekordbox's own key and tempo analysis can be scored with the same rules. `bench/rekordbox_accuracy_kit.py` puts the previews in a new folder, cloned on APFS so they take no extra space, with a checklist for analyzing them in rekordbox and exporting its collection as XML:

```
uv run bench/rekordbox_accuracy_kit.py --data bench/data --output ~/Music/euphoria-accuracy-kit
```

The `rekordbox` mode of the benchmark then reads that export and prints the same tables as the detectors: exact, fifth, relative, parallel, other and MIREX score for key, Accuracy 1 and Accuracy 2 for tempo, and how many tracks had no value.

```
cargo run --release -p euphoria-bench -- rekordbox ~/Music/euphoria-accuracy-kit/rekordbox-collection.xml bench/data
```

Tracks are matched by file name, so the export may list them anywhere. A name listed more than once (43 previews are in both datasets) is taken from the copy in a folder named `key` or `tempo`, as the kit lays them out. `Tonality` is read in Camelot, Open Key or musical notation; an empty `Tonality` or an `AverageBpm` of zero counts as no value, as does a preview missing from the export. `--verify` instead of `--output` checks that rekordbox changed none of the previews in a kit.

## rekordbox test kit

`bench/rekordbox_test_kit.py` builds a folder for checking by hand which tags and rekordbox XML values rekordbox reads, the questions [use Euphoria with your DJ program](../guides/dj-programs.md#rekordbox) leaves open. It cuts thirteen 30 second clips from the benchmark previews, tags each in one variant (key field name per format, key notation, tempo with and without decimals, comment), writes a rekordbox XML file covering three of them, and a checklist with tables to fill in. One WAV clip is tagged by `euphoria tags write` itself, with the rekordbox preset, to check that rekordbox shows the label Euphoria writes into the RIFF INFO comment; the script uses the release build, or the binary given with `--euphoria`. It needs [uv](https://docs.astral.sh/uv/) and ffmpeg:

```
./bench/download_datasets.sh
cargo build --release -p euphoria-cli
uv run bench/rekordbox_test_kit.py --clips bench/data/key/audio --output /tmp/euphoria-kit
```

The script writes only into the new output folder. It never opens a rekordbox library or its settings; the checklist asks whoever runs it to back up the library first and says how to remove the test tracks afterwards.

## Audio quality benchmark

The thresholds of the [audio quality](../concepts/audio-quality.md) verdict come from encoding lossless music at several bit rates and measuring the result. Download the lossless sources (about 170 MB of WAV files under Creative Commons BY-SA, into `bench/data/quality`) and run the benchmark. It needs [uv](https://docs.astral.sh/uv/) and ffmpeg, and uses Apple's AAC encoder through `afconvert` on macOS:

```
./bench/download_quality_sources.sh
uv run bench/quality_benchmark.py bench/data/quality
```

It prints the table in [bench/results/quality.md](https://github.com/lvndry/euphoria/blob/main/bench/results/quality.md). `euphoria-bench quality LIST_FILE` measures any list of files on its own: each line is a label, a tab and a path, and the output is one tab separated line per file with the label and never the path, so a private library can be measured too.

## Beat grid benchmark

The [beat grid](../concepts/accuracy.md#beat-grid) is measured on the disco and hip hop excerpts of GTZAN with the beat and downbeat annotations of beat_this. The annotations are under the MIT licence; the GTZAN audio has no stated licence, so the script downloads it for local measurement only, and it must never be committed or redistributed. Download them (about 1.2 GB for the GTZAN archive, of which about 260 MB of WAV files is kept in `bench/data/beats`) and run the benchmark:

```
./bench/download_beat_dataset.sh
cargo run --release -p euphoria-bench -- beats --tracks development
cargo run --release -p euphoria-bench -- beats --tracks held-out
```

It prints the tables in [bench/results/beats.md](https://github.com/lvndry/euphoria/blob/main/bench/results/beats.md): the beat F-measure and how often the grid and the downbeat are right, with the tempo as reported and with its octave taken from the annotation, and how often each confidence level is right. Settings are chosen on the development half and checked on the held out half.

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

The fitter excludes features with missing values in tuning or spread (including undetected tempos), and features whose low and high anchors are equal. Reports omit tracks missing a reference or a selected feature and show how many were omitted. Held-out data does not decide which features are used, and in cross validation each fold's anchors come from its training songs and the spread file only, so the songs a fold scores do not shape their own scaling. A fit without useful features, positive weights or a usable output range stops with a diagnostic instead of emitting an invalid model.

Run the fitting regression tests with:

```
uv run --with numpy --with scipy python -m unittest discover -s bench -p 'test_*.py'
```

Changing the shipped rating changes every user's ratings, so it needs evidence on the music Euphoria is for: reference ratings of dance music in a labels file, with a model that is clearly better on tracks held out from fitting. A change to `ENERGY_MODEL` raises `ENERGY_MODEL_VERSION`, which makes cached ratings be computed again, and records the before and after in `bench/results/energy.md`.
