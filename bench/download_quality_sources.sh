#!/usr/bin/env bash
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Downloads lossless sources for the audio quality benchmark into
# bench/data/quality/sources, which is not tracked by git.
#
# The music is "Lithium Toolbox" by John Lithium (Argali Records, ARGREC14),
# released as 16 bit 44.1 kHz WAV files on the Internet Archive under the
# Creative Commons Attribution-ShareAlike 3.0 licence:
# https://archive.org/details/ARGREC14
set -euo pipefail

bench_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
target="${bench_directory}/data/quality/sources"
base_url="https://archive.org/download/ARGREC14"
tracks=(
  01LithiumToolboxOnespaceTechnician.wav
  02LithiumToolboxTwoendOfEasyComfort.wav
  03LithiumToolboxThreeaManNamedTyrell.wav
  06LithiumToolboxSixshipAndCrewLost.wav
  14LithiumToolboxFourteenshaman.wav
  19LithiumToolboxNineteenheavyEnergy.wav
)

mkdir -p "${target}"
for track in "${tracks[@]}"; do
  if [ ! -s "${target}/${track}" ]; then
    curl --fail --location --retry 3 --silent --show-error \
      --output "${target}/${track}.part" "${base_url}/${track}"
    mv "${target}/${track}.part" "${target}/${track}"
  fi
done
echo "$(ls "${target}" | wc -l | tr -d ' ') lossless sources in ${target}"
