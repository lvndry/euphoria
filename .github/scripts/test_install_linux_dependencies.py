# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Run with python3 -m unittest discover -s .github/scripts -p 'test_*.py'."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("install-linux-dependencies.sh")


class InstallTests(unittest.TestCase):
    def run_install(self, mode):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            shutil.copy(SCRIPT, root / SCRIPT.name)
            stubs = {
                "sudo": """#!/usr/bin/env bash
set -eu
if [[ "$*" == 'apt-get update' ]]; then
  count=$(cat "$TEST_STATE/count" 2>/dev/null || echo 0)
  count=$((count + 1))
  echo "$count" > "$TEST_STATE/count"
  case "$TEST_MODE" in
    failure) exit 42 ;;
    warning) echo 'W: Failed to fetch a mirror'; exit 0 ;;
    transient) if ((count == 1)); then exit 42; fi ;;
  esac
else
  echo install >> "$TEST_STATE/installed"
fi
""",
                "sleep": "#!/usr/bin/env bash\necho \"$1\" >> \"$TEST_STATE/pauses\"\n",
                "install-libkeyfinder.sh": "#!/usr/bin/env bash\necho keyfinder >> \"$TEST_STATE/installed\"\n",
            }
            for name, source in stubs.items():
                path = root / name
                path.write_text(source)
                path.chmod(0o755)
            result = subprocess.run(
                ["bash", str(root / SCRIPT.name)],
                env={**os.environ, "PATH": f"{root}:{os.environ['PATH']}",
                     "TEST_STATE": str(root), "TEST_MODE": mode},
                capture_output=True, text=True, check=False,
            )
            count = int((root / "count").read_text())
            installed = (root / "installed").read_text() if (root / "installed").exists() else ""
            pauses = (root / "pauses").read_text().splitlines() if (root / "pauses").exists() else []
            return result, count, installed, pauses

    def test_nonzero_update_without_diagnostic_is_retried_and_fails(self):
        result, count, installed, pauses = self.run_install("failure")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(count, 5)
        self.assertEqual(installed, "")
        self.assertEqual(pauses, ["15", "30", "45", "60"])

    def test_mirror_warning_with_zero_exit_is_still_retried(self):
        result, count, installed, _ = self.run_install("warning")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(count, 5)
        self.assertEqual(installed, "")

    def test_successful_retry_continues_through_both_install_steps(self):
        result, count, installed, pauses = self.run_install("transient")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(count, 2)
        self.assertEqual(installed.splitlines(), ["install", "keyfinder"])
        self.assertEqual(pauses, ["15"])
