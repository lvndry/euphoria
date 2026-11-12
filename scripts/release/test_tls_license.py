# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Run the real packagers on scratch binaries; native signing/artwork are stubbed."""

import os
import pathlib
import shutil
import subprocess
import tarfile
import tempfile
import unittest

REPOSITORY = pathlib.Path(__file__).resolve().parents[2]
LIBRARIES = ("webpki-root-certs-CDLA-Permissive-2.0", "demucs-NOTICE")


class PackagedStemLicenseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="euphoria-packaged-licenses-")
        self.addCleanup(self.temporary.cleanup)
        self.root = pathlib.Path(self.temporary.name)
        self.repo = self.root / "repo with spaces"
        for relative in ("scripts/bundle-macos.sh", "scripts/release/package-cli.sh"):
            target = self.repo / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPOSITORY / relative, target)
        for package in ("euphoria-cli", "euphoria-desktop"):
            manifest = self.repo / "crates" / package / "Cargo.toml"
            manifest.parent.mkdir(parents=True)
            manifest.write_text('version = "0.1.0"\n')
        for name in ("LICENSE", "README.md"):
            (self.repo / name).write_text(name)
        for name in ("euphoria", "euphoria-desktop"):
            binary = self.repo / "target/release" / name
            binary.parent.mkdir(parents=True, exist_ok=True)
            binary.write_text("#!/bin/sh\nexit 0\n")
            binary.chmod(0o755)
        for library in LIBRARIES:
            target = self.repo / "licenses" / f"{library}.txt"
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPOSITORY / target.relative_to(self.repo), target)
        (self.repo / "scripts/macos_libraries.py").write_text(
            "import sys\nif sys.argv[1] == 'minimum-macos': print('11.0')\n"
        )
        tools = self.root / "tools"
        tools.mkdir()
        for name, body in {
            "uname": "echo Darwin",
            "rustc": "echo 'host: aarch64-apple-darwin'",
            "codesign": "exit 0",
            "sips": "exit 0",
            "iconutil": "exit 0",
            "plutil": "exit 0",
        }.items():
            tool = tools / name
            tool.write_text(f"#!/bin/sh\n{body}\n")
            tool.chmod(0o755)
        self.env = {**os.environ, "PATH": f"{tools}:{os.environ['PATH']}", "TMPDIR": str(self.root)}

    def package(self, kind):
        if kind == "cli":
            args = ["scripts/release/package-cli.sh", "--skip-build", str(self.root / "dist")]
        else:
            args = ["scripts/bundle-macos.sh", "--skip-build", "--no-dmg"]
        return subprocess.run(["sh", *args], cwd=self.repo, env=self.env, capture_output=True, text=True)

    def expected(self, library):
        return (REPOSITORY / "licenses" / f"{library}.txt").read_bytes()

    def test_cli_archive_contains_complete_tls_data_notice(self):
        result = self.package("cli")
        self.assertEqual(result.returncode, 0, result.stderr)
        archive_path, = (self.root / "dist").glob("*.tar.gz")
        with tarfile.open(archive_path) as archive:
            for library in LIBRARIES:
                name = f"euphoria-0.1.0-aarch64-apple-darwin/licenses/{library}.txt"
                self.assertIn(name, archive.getnames())
                self.assertEqual(archive.extractfile(name).read(), self.expected(library))

    def test_desktop_bundle_contains_complete_tls_data_notice(self):
        result = self.package("desktop")
        self.assertEqual(result.returncode, 0, result.stderr)
        resources = self.repo / "target/release/bundle/Euphoria.app/Contents/Resources"
        for library in LIBRARIES:
            notice = resources / "licenses" / f"{library}.txt"
            self.assertTrue(notice.is_file(), str(notice))
            self.assertEqual(notice.read_bytes(), self.expected(library))

    def test_missing_upstream_notice_stops_both_packagers(self):
        notice = self.repo / "licenses/webpki-root-certs-CDLA-Permissive-2.0.txt"
        notice.unlink()
        for kind in ("cli", "desktop"):
            with self.subTest(kind=kind):
                result = self.package(kind)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("webpki-root-certs-CDLA-Permissive-2.0.txt", result.stderr)


if __name__ == "__main__":
    unittest.main()
