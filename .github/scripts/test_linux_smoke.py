# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Exercise the actual source-only packaging step without a Rust/LTO build."""

import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/ci.yml").read_text()


def job(name):
    match = re.search(rf"^  {re.escape(name)}:\n(.*?)(?=^  [a-z][a-z-]*:|\Z)", WORKFLOW, re.M | re.S)
    if match is None:
        raise AssertionError(f"missing workflow job {name}")
    return match.group(1)


def smoke_commands():
    match = re.search(
        r"- name: Package the debug binaries the release way\n.*?        run: \|\n((?:          [^\n]*\n)+)",
        job("linux-packages"), re.S,
    )
    if match is None:
        raise AssertionError("missing source-only packaging commands")
    return "\n".join(line[10:] for line in match.group(1).splitlines())


class LinuxSmokeTests(unittest.TestCase):
    def test_artifact_producers_match_the_clean_package_baseline(self):
        baseline = re.search(r"^    runs-on: (\S+)", job("linux-packages"), re.M).group(1)
        desktop = re.search(r"^    runs-on: (\S+)", job("desktop-linux"), re.M).group(1)
        cli = re.search(r"name: Linux, runner: ([^,}]+)", job("test")).group(1)
        self.assertEqual(cli, baseline, "CLI artifact cannot require a newer glibc than its runtime")
        self.assertEqual(desktop, baseline, "desktop artifact cannot require a newer glibc than its runtime")

    def run_packaging(self, existing_release):
        with tempfile.TemporaryDirectory(prefix="euphoria-smoke-") as temporary:
            root = Path(temporary)
            (root / "target/debug").mkdir(parents=True)
            for name in ("euphoria", "euphoria-desktop"):
                downloaded = root / "target/debug" / name
                downloaded.write_text("#!/bin/sh\nexit 0\n")
                # upload/download-artifact restores regular files as 0644.
                downloaded.chmod(0o644)
            if existing_release:
                (root / "target/release/deps").mkdir(parents=True)
                (root / "target/release/deps/cached").write_text("retained dependency")
            for script, binary in (("scripts/release/package-cli.sh", "euphoria"), ("scripts/bundle-linux.sh", "euphoria-desktop")):
                path = root / script
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(f'#!/bin/sh\nset -eu\ntarget/release/{binary} --version\necho {binary} >> packaged\n')
                path.chmod(0o755)
            result = subprocess.run(["sh", "-eu", "-c", smoke_commands()], cwd=root,
                                    env=os.environ, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root / "packaged").read_text().splitlines(), ["euphoria", "euphoria-desktop"])
            if existing_release:
                self.assertEqual((root / "target/release/deps/cached").read_text(), "retained dependency")

    def test_downloaded_binaries_regain_execute_permissions(self):
        self.run_packaging(False)

    def test_restored_release_dependencies_do_not_break_smoke_packaging(self):
        self.run_packaging(True)


if __name__ == "__main__":
    unittest.main()
