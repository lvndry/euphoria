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
    def run_install(self, mode, extra_env=None, arguments=()):
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
  if [[ "$*" == apt-get\ install* ]]; then
    echo install >> "$TEST_STATE/installed"
  fi
fi
""",
                "sleep": "#!/usr/bin/env bash\necho \"$1\" >> \"$TEST_STATE/pauses\"\n",
                "install-libkeyfinder.sh": "#!/usr/bin/env bash\necho keyfinder >> \"$TEST_STATE/installed\"\n",
                "curl": "#!/usr/bin/env bash\necho onnxruntime >> \"$TEST_STATE/installed\"\n",
                "sha256sum": "#!/usr/bin/env bash\nexit 0\n",
                "tar": "#!/usr/bin/env bash\nexit 0\n",
            }
            for name, source in stubs.items():
                path = root / name
                path.write_text(source)
                path.chmod(0o755)
            result = subprocess.run(
                ["bash", str(root / SCRIPT.name), *arguments],
                env={**os.environ, "PATH": f"{root}:{os.environ['PATH']}",
                     "TEST_STATE": str(root), "TEST_MODE": mode, **(extra_env or {})},
                capture_output=True, text=True, check=False,
            )
            count = int((root / "count").read_text()) if (root / "count").exists() else 0
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

    def test_successful_retry_continues_through_every_install_step(self):
        result, count, installed, pauses = self.run_install("transient")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(count, 2)
        self.assertEqual(installed.splitlines(),
                     ["install", "keyfinder", "onnxruntime"])
        self.assertEqual(pauses, ["15"])

    def test_cached_packages_skip_apt_but_not_the_other_installs(self):
        result, count, installed, _ = self.run_install(
            "failure", {"EUPHORIA_APT_PACKAGES_INSTALLED": "1"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(count, 0)
        self.assertEqual(installed.splitlines(), ["keyfinder", "onnxruntime"])

    def test_package_list_is_printed_without_installing_anything(self):
        result, count, installed, _ = self.run_install(
            "failure", arguments=["--list-packages"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(count, 0)
        self.assertEqual(installed, "")
        self.assertIn("libfftw3-dev", result.stdout.split())
