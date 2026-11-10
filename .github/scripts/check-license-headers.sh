#!/usr/bin/env bash
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Fails when a tracked source file lacks the SPDX license header.
set -euo pipefail

expected_identifier="SPDX-License-Identifier: GPL-3.0-or-later"
missing_files=()

while IFS= read -r source_file; do
  if ! head -n 5 "$source_file" | grep -q "$expected_identifier"; then
    missing_files+=("$source_file")
  fi
done < <(git ls-files '*.rs' '*.cpp' '*.h' '*.sh' '*.py')

if [ "${#missing_files[@]}" -gt 0 ]; then
  echo "These files are missing the license header:"
  printf '  %s\n' "${missing_files[@]}"
  echo
  echo "Add these lines at the top, using the file's comment syntax:"
  echo "  Copyright 2026 The Euphoria contributors"
  echo "  $expected_identifier"
  exit 1
fi

echo "All source files carry the license header."
