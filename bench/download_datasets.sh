#!/usr/bin/env bash
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Downloads the GiantSteps key and tempo datasets (annotations and audio previews)
# into bench/data, which is not tracked by git.
set -euo pipefail

bench_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
data_directory="${bench_directory}/data"
audio_base_url="https://www.cp.jku.at/datasets/giantsteps/backup"

fetch_dataset() {
  local dataset_name="$1"
  local repository_url="$2"
  local dataset_directory="${data_directory}/${dataset_name}"

  if [ ! -d "${dataset_directory}/.git" ]; then
    git clone --quiet --depth 1 "${repository_url}" "${dataset_directory}"
  fi
  mkdir -p "${dataset_directory}/audio"

  find "${dataset_directory}/md5" -name '*.md5' -print0 |
    xargs -0 -n 1 -P 8 bash -c '
      track_name="$(basename "$3" .md5)"
      target_file="$1/audio/${track_name}.mp3"
      if [ ! -s "${target_file}" ]; then
        curl --silent --fail --location --retry 3 \
          --output "${target_file}" "$2/${track_name}.mp3" ||
          { rm -f "${target_file}"; echo "failed: ${track_name}" >&2; }
      fi
    ' _ "${dataset_directory}" "${audio_base_url}"

  local downloaded_count
  downloaded_count="$(find "${dataset_directory}/audio" -name '*.mp3' | wc -l | tr -d ' ')"
  echo "${dataset_name}: ${downloaded_count} audio files"
}

fetch_dataset key https://github.com/GiantSteps/giantsteps-key-dataset.git
fetch_dataset tempo https://github.com/GiantSteps/giantsteps-tempo-dataset.git
