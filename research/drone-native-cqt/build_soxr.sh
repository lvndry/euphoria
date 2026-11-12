#!/usr/bin/env bash
# Research-only exact source build. No system installation or downloads.
set -euo pipefail
root=${1:?dedicated experiment root required}
source_dir="$root/dependencies/soxr-1.1.0/libsoxr"
printf '%s  %s\n' '9f228ae21c78fa9359ca98d8a5e8e91f30639e438e574133dace62c5b5309e44' "$root/soxr-1.1.0.tar.gz" | sha256sum -c -
python3 "$(dirname "$0")/verify_soxr_source.py" "$root"
taskset -c 2,3 cmake -S "$source_dir" -B "$root/soxr-build" -DBUILD_SHARED_LIBS=ON -DWITH_OPENMP=OFF -DBUILD_TESTS=OFF -DBUILD_EXAMPLES=OFF -DWITH_LSR_BINDINGS=OFF -DCMAKE_BUILD_TYPE=Release
taskset -c 2,3 cmake --build "$root/soxr-build" --parallel 2
taskset -c 2,3 cc -shared -fPIC -O2 -I "$source_dir/src" "$(dirname "$0")/src/soxr_split.c" -L "$root/soxr-build/src" -Wl,-rpath,"$root/soxr-build/src" -lsoxr -o "$root/libcqt_soxr_split.so"
