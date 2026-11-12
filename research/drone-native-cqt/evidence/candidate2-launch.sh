#!/usr/bin/env bash
set -uo pipefail
cd /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/drone-source-2ac6364
taskset -c 2,3 env OMP_NUM_THREADS=1 MKL_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 NUMEXPR_NUM_THREADS=1 /home/lysk-admin/drone/.venv/bin/python -m tools.run_cqt_expansion \
 --arithmetic-ablation \
 --math-library /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/svml-probe-93aeb04/libcqt_numpy_svml.so \
 --stage-root /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/candidate2-2ac6364-2f87e2c-v1 \
 --budget-root /home/lysk-admin/drone-experiments/native-cqt-20261111-v1 \
 --study /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/candidate2-2ac6364-2f87e2c-v1/study-manifest.json \
 --source-root /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/drone-source-2ac6364 \
 --python /home/lysk-admin/drone/.venv/bin/python \
 --native /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/euphoria-source-2f87e2c/research/drone-native-cqt/target/release/drone-native-cqt \
 --ort-library /home/lysk-admin/drone/.venv/lib/python3.12/site-packages/onnxruntime/capi/libonnxruntime.so.1.31.1 \
 --soxr-wrapper /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/libcqt_soxr_split.so \
 --full-model /home/lysk-admin/drone-experiments/k10-native-fixtures-20261111-v1/source/weights/drone-v1.onnx \
 --feature-source /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/drone-source-2ac6364/drone/features.py \
 --goldens /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/goldens-b1c90d3/manifest.json \
 --deadline 2026-11-12T06:00:00+00:00 &
candidate_pid=$!
printf '%s\n' "$candidate_pid" > /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/candidate2-2ac6364-2f87e2c-v1/controller.pid
wait "$candidate_pid"
candidate_exit=$?
printf '{"exit_code": %s}\n' "$candidate_exit" > /home/lysk-admin/drone-experiments/native-cqt-20261111-v1/candidate2-2ac6364-2f87e2c-v1/exit.json
exit "$candidate_exit"
