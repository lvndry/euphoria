# Standalone native CQT research harness

Read [REPORT.md](REPORT.md) for the frozen method/results and
[EXPANSION.md](EXPANSION.md) for the proposed next stage. Production workspace,
MSRV, default dependencies and the detector are untouched.

The companion Drone source exports reference files with:

```sh
python -m tools.export_cqt_goldens \
  --data-manifest /home/lysk-admin/drone-experiments/k10-multidomain-20261111-v1/data/manifest.json \
  --selection /home/lysk-admin/drone-experiments/k10-cpu-20261111-v1/selection.json \
  --feature-source drone/features.py \
  --full-model /home/lysk-admin/drone-experiments/k10-native-fixtures-20261111-v1/source/weights/drone-v1.onnx \
  --full-model-sha256 35fc1bfe5d67f7aed2c46221c15519d0f3d5822351932b5bd9899d47d48cb613 \
  --protocol experiments/studies/native-cqt/PROTOCOL.md \
  --output /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/goldens-b1c90d3
```

Use the server's installed Drone Python with numeric thread variables one and
`taskset -c 2,3`. Output must be a new directory. The exported files stay on the
server and never enter Git. `tools.record_cqt_environment` records actual build
and binary identities; its corrected source is Drone `b9c22f2`.

Build the exact verified source already extracted under the dedicated root:

```sh
bash research/drone-native-cqt/build_soxr.sh /home/lysk-admin/drone-experiments/native-cqt-20261111-v1
cargo build --release --locked --jobs 2 --manifest-path research/drone-native-cqt/Cargo.toml
```

The first nine-case measurements use pinned Rust1.99.1, source `d8424bf`, binary
SHA `9ee59e01709bf6f9def5fbeaade529944e5835731910ca166ac5d22554616f4b`.
The optional owner snapshot is external and must never be committed or published.
Building with `CQT_OWNER_SNAPSHOT=/absolute/frozen/snapshot` enables that candidate;
without it, the default unit tests and exported-kernel candidate remain usable.
The snapshot contains `cqt.rs` and `halving.rs`; exact source hashes are recorded
in the first comparison reports.

Actual first passing command, under server task root:

```sh
ulimit -t 600
taskset -c 2,3 env OMP_NUM_THREADS=1 MKL_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 NUMEXPR_NUM_THREADS=1 \
  timeout 610 \
  /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/euphoria-source-d8424bf/research/drone-native-cqt/target/release/drone-native-cqt \
  --candidate exported \
  --soxr-wrapper /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/libcqt_soxr_split.so \
  --manifest /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/goldens-b1c90d3/manifest.json \
  --full-model /home/lysk-admin/drone-experiments/k10-native-fixtures-20261111-v1/source/weights/drone-v1.onnx \
  --ort-library /home/lysk-admin/drone/.venv/lib/python3.12/site-packages/onnxruntime/capi/libonnxruntime.so.1.31.1 \
  --output /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/exported-d8424bf.json
```

Do not overwrite the existing evidence. Reproduction requires a new output name.
Use `--candidate owner` without `--soxr-wrapper` for the externally snapshotted
experimental frontend. The failed WIP command exits nonzero **after** preserving
all per-case diagnostics. The exported candidate's zero exit reflects only the
nine frozen identical-PCM cases.

```sh
cargo fmt --manifest-path research/drone-native-cqt/Cargo.toml --check
cargo test --manifest-path research/drone-native-cqt/Cargo.toml
cargo clippy --manifest-path research/drone-native-cqt/Cargo.toml --all-targets -- -D warnings
uv run --script research/drone-native-cqt/plot_results.py
```

`libsoxr` and the Python-SoXR-derived wrapper are LGPL-2.1-or-later; preserve
[the upstream Python wrapper notice](LICENSE-python-soxr). The task-local source
archive also contains libsoxr and its component notices. No dynamic product bundle
or legal distribution conclusion is established here.
