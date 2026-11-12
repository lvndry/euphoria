# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Exercise the Cargo runner's real subprocess and cleanup behavior."""

import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

RUNNER = Path(__file__).resolve().parents[1] / "run-macos.py"


class MacosRunnerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="euphoria runner ")
        self.addCleanup(self.temporary.cleanup)
        self.folder = Path(self.temporary.name)
        iconutil = self.folder / "iconutil"
        iconutil.write_text(
            f"#!{sys.executable}\nfrom pathlib import Path\nimport sys\n"
            "Path(sys.argv[sys.argv.index('--output') + 1]).write_bytes(b'icns')\n"
        )
        iconutil.chmod(0o755)
        self.environment = {**os.environ, "PATH": f"{self.folder}{os.pathsep}{os.environ['PATH']}",
                            "TMPDIR": str(self.folder)}

    def executable(self, name, body):
        binary = self.folder / name
        binary.write_text(f"#!{sys.executable}\n{body}\n")
        binary.chmod(0o755)
        return binary

    def run_binary(self, binary, *arguments):
        return subprocess.run(
            [sys.executable, str(RUNNER), str(binary), *arguments],
            env=self.environment, capture_output=True, text=True, timeout=10,
        )

    def test_cli_arguments_and_exit_status_pass_through(self):
        binary = self.executable("euphoria", "import json,sys\nprint(json.dumps(sys.argv[1:]))\nsys.exit(37)")
        result = self.run_binary(binary, "--version", "path with spaces", "")
        self.assertEqual(result.returncode, 37, result.stderr)
        self.assertEqual(json.loads(result.stdout), ["--version", "path with spaces", ""])

    def test_desktop_identity_arguments_and_cleanup(self):
        binary = self.executable("euphoria-desktop", """import json,plistlib,sys
from pathlib import Path
executable = Path(sys.argv[0])
contents = executable.parent.parent
print(json.dumps({'path': str(executable), 'arguments': sys.argv[1:],
 'info': plistlib.loads((contents / 'Info.plist').read_bytes()),
 'icon': (contents / 'Resources/AppIcon.icns').read_bytes().decode()}))
sys.exit(19)""")
        result = self.run_binary(binary, "track with spaces", "--flag")
        self.assertEqual(result.returncode, 19, result.stderr)
        observed = json.loads(result.stdout)
        self.assertEqual(observed["arguments"], ["track with spaces", "--flag"])
        self.assertEqual(observed["info"]["CFBundleName"], "Euphoria")
        self.assertEqual(observed["info"]["CFBundleExecutable"], "euphoria-desktop")
        self.assertEqual(observed["icon"], "icns")
        self.assertFalse(Path(observed["path"]).exists())
        self.assertTrue(binary.exists())

    def stop_runner(self, runner):
        if runner.poll() is None:
            runner.terminate()
            try:
                runner.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
        try:
            os.killpg(runner.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        runner.communicate(timeout=10)

    @unittest.skipUnless(os.name == "posix", "POSIX signals")
    def test_termination_during_icon_conversion_cleans_up(self):
        for number in (signal.SIGTERM, signal.SIGHUP, signal.SIGQUIT):
            with self.subTest(signal=number):
                ready = self.folder / "icon-ready"
                ready.unlink(missing_ok=True)
                self.executable("iconutil", f"""import os,resource,sys,time
from pathlib import Path
resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
Path({str(ready)!r}).write_text(str(os.getpid()) + '\\n' + sys.argv[sys.argv.index('--output') + 1])
while True: time.sleep(1)""")
                binary = self.executable("euphoria-desktop", "raise SystemExit('must not start')")
                runner = subprocess.Popen(
                    [sys.executable, str(RUNNER), str(binary)],
                    env=self.environment, start_new_session=True, stderr=subprocess.PIPE,
                )
                try:
                    deadline = time.monotonic() + 10
                    while not ready.exists() and time.monotonic() < deadline:
                        time.sleep(0.02)
                    self.assertTrue(ready.exists(), "icon conversion never started")
                    pid, icon = ready.read_text().splitlines()
                    temporary = Path(icon).parents[3]
                    runner.send_signal(number)
                    status = runner.wait(timeout=10)
                    self.assertFalse(temporary.exists(), "startup termination left the app bundle behind")
                    self.assertEqual(status, 128 + number)
                    with self.assertRaises(ProcessLookupError):
                        os.kill(int(pid), 0)
                finally:
                    self.stop_runner(runner)

    @unittest.skipUnless(os.name == "posix", "POSIX signals")
    def test_termination_reaches_the_desktop_and_removes_the_bundle(self):
        for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP, signal.SIGQUIT):
            with self.subTest(signal=number):
                ready = self.folder / "ready"
                ready.unlink(missing_ok=True)
                binary = self.executable("euphoria-desktop", f"""import os,resource,time
from pathlib import Path
resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
Path({str(ready)!r}).write_text(str(os.getpid()) + '\\n' + __file__)
while True: time.sleep(1)""")
                runner = subprocess.Popen(
                    [sys.executable, str(RUNNER), str(binary)],
                    env=self.environment, start_new_session=True, stderr=subprocess.PIPE,
                )
                try:
                    deadline = time.monotonic() + 10
                    while not ready.exists() and time.monotonic() < deadline:
                        time.sleep(0.02)
                    self.assertTrue(ready.exists(), "the desktop never started")
                    pid, executable = ready.read_text().splitlines()
                    runner.send_signal(number)
                    self.assertEqual(runner.wait(timeout=10), 128 + number)
                    self.assertFalse(Path(executable).exists())
                    with self.assertRaises(ProcessLookupError):
                        os.kill(int(pid), 0)
                finally:
                    self.stop_runner(runner)
