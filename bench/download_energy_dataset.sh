#!/usr/bin/env bash
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Downloads DEAM, the MediaEval Database for Emotional Analysis in Music, into
# bench/data/energy, which is not tracked by git, and builds the labels files
# that `euphoria-bench energy` reads.
#
# DEAM is 1802 Creative Commons licensed songs with arousal ratings given by
# listeners. Its terms of use are Creative Commons BY-NC: free for non profit
# work, with a citation of the paper by Aljanaki, Yang and Soleymani in PLOS
# ONE that the manual of the dataset names.
#
# Run download_datasets.sh first: the GiantSteps Key previews it fetches are
# listed here as the dance music whose ratings should use the whole scale.
set -euo pipefail

bench_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
dataset_directory="${bench_directory}/data/energy/deam"
base_url="https://cvml.unige.ch/databases/DEAM"

mkdir -p "${dataset_directory}"

fetch_archive() {
  local archive_name="$1"
  local marker="${dataset_directory}/.${archive_name}.done"
  if [ -e "${marker}" ]; then
    return
  fi
  curl --fail --location --retry 3 --continue-at - \
    --output "${dataset_directory}/${archive_name}" "${base_url}/${archive_name}"
  unzip -q -o "${dataset_directory}/${archive_name}" -d "${dataset_directory}"
  rm "${dataset_directory}/${archive_name}"
  touch "${marker}"
}

fetch_archive DEAM_Annotations.zip
fetch_archive metadata.zip
fetch_archive DEAM_audio.zip

python3 "${bench_directory}/build_energy_labels.py" "${dataset_directory}"
