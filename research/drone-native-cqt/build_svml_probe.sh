#!/usr/bin/env bash
# Bounded arithmetic-only upstream-kernel prototype; no model/NumPy linkage.
set -euo pipefail
output=${1:?new dedicated output directory required}
mkdir "$output"
source_dir=$(cd "$(dirname "$0")" && pwd)
printf '%s  %s\n' '91f68c250ebf9360c078f8ae3f1e6a96506d5995bb717362fb7b65147f3f76a2' "$source_dir/vendor/svml_z0_log1p_s_la.s" | sha256sum -c -
taskset -c 2,3 cc -O2 -fno-fast-math -shared -fPIC -Wl,-z,noexecstack "$source_dir/src/svml_wrapper.c" "$source_dir/vendor/svml_z0_log1p_s_la.s" -o "$output/libcqt_numpy_svml.so"
taskset -c 2,3 cc -O2 -fno-fast-math "$source_dir/src/svml_probe.c" -L "$output" -Wl,-rpath,"$output" -lcqt_numpy_svml -lm -o "$output/svml_probe"
