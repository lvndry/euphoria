#!/usr/bin/env bash
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Downloads the disco and hip hop excerpts of GTZAN with their beat and
# downbeat annotations into bench/data/beats, which is not tracked by git,
# for `euphoria-bench beats`.
#
# No openly available dataset of electronic dance music has both audio and
# beat annotations, so the beat grid is measured on the steady 4/4 drum
# music of GTZAN instead: 100 disco and 100 hip hop excerpts of 30 seconds.
# GTZAN comes without a licence and is used for research; it is fetched
# here and never stored in the repository. The beat and downbeat
# annotations, from the GTZAN-Rhythm work of Marchand and Peeters, come from
# the beat_this_annotations collection of the Institute of Computational
# Perception, under the MIT licence.
set -euo pipefail

bench_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
dataset_directory="${bench_directory}/data/beats"
audio_url="https://huggingface.co/datasets/marsyas/gtzan/resolve/main/data/genres.tar.gz"
audio_checksum="b28cc067ff6199bd826f5d1a6931458586d64acae7f44b2e882b7a97af057531"
annotations_commit="fd9fcc0896cb78730bae735102c8753ef3a3badd"
annotations_url="https://github.com/CPJKU/beat_this_annotations/archive/${annotations_commit}.zip"

mkdir -p "${dataset_directory}"
download_directory="$(mktemp -d)"
trap 'rm -rf "${download_directory}"' EXIT

if [ ! -e "${dataset_directory}/.audio.done" ]; then
  curl --fail --location --retry 3 --output "${download_directory}/genres.tar.gz" "${audio_url}"
  echo "${audio_checksum}  ${download_directory}/genres.tar.gz" | shasum -a 256 --check --quiet
  tar -xzf "${download_directory}/genres.tar.gz" -C "${download_directory}" genres/disco genres/hiphop
  mkdir -p "${dataset_directory}/audio"
  mv "${download_directory}"/genres/disco/*.wav "${download_directory}"/genres/hiphop/*.wav \
    "${dataset_directory}/audio/"
  touch "${dataset_directory}/.audio.done"
fi

if [ ! -e "${dataset_directory}/.annotations.done" ]; then
  curl --fail --location --retry 3 --output "${download_directory}/annotations.zip" "${annotations_url}"
  unzip -q -o "${download_directory}/annotations.zip" -d "${download_directory}"
  mkdir -p "${dataset_directory}/annotations"
  annotations_source="${download_directory}/beat_this_annotations-${annotations_commit}/gtzan/annotations/beats"
  cp "${annotations_source}"/gtzan_disco_*.beats "${annotations_source}"/gtzan_hiphop_*.beats \
    "${dataset_directory}/annotations/"
  touch "${dataset_directory}/.annotations.done"
fi
