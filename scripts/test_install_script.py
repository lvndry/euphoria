"""Runs scripts/install.sh against a fake release made of local files."""

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "install.sh"
VERSION = "2026.11.5"
NEXT_VERSION = "2026.12.0"
BUNDLE_IDENTIFIER = "io.github.lvndry.euphoria"

FAKE_UNAME = """#!/bin/sh
case "$1" in
  -s) echo "$FAKE_SYSTEM" ;;
  -m) echo "$FAKE_MACHINE" ;;
  *) echo "$FAKE_SYSTEM" ;;
esac
"""

FAKE_APPIMAGE = """#!/bin/sh
if [ "$1" = --appimage-extract ]; then
  mkdir -p squashfs-root/usr/share/icons/hicolor/256x256/apps
  echo png > squashfs-root/usr/share/icons/hicolor/256x256/apps/euphoria.png
  exit 0
fi
echo "Euphoria desktop"
"""


def write_executable(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    path.chmod(0o755)


class FakeRelease:
    def __init__(self, root):
        self.api = root / "api"
        self.files = root / "files"
        (self.api / "tags").mkdir(parents=True)
        self.files.mkdir()

    def directory(self, version):
        return self.files / f"v{version}"

    def add(self, version, target, latest=True, app_files=None):
        directory = self.directory(version)
        directory.mkdir()
        staging = directory.parent / f"staging-{version}"
        name = f"euphoria-{version}-{target}"
        folder = staging / name
        write_executable(folder / "bin" / "euphoria", f"#!/bin/sh\necho 'euphoria {version}'\n")
        (folder / "lib").mkdir()
        (folder / "lib" / f"libfake.so.{version}").write_text("library")
        (folder / "licenses").mkdir()
        (folder / "licenses" / "notice.txt").write_text("notice")
        (folder / "LICENSE").write_text("GPL")
        with tarfile.open(directory / f"{name}.tar.gz", "w:gz") as archive:
            archive.add(folder, arcname=name)
        shutil.rmtree(staging)
        for file_name, writer in (app_files or {}).items():
            writer(directory / file_name)
        assets = sorted(path.name for path in directory.iterdir())
        sums = "".join(
            f"{hashlib.sha256((directory / asset).read_bytes()).hexdigest()}  {asset}\n" for asset in assets
        )
        (directory / "SHA256SUMS").write_text(sums)
        release = {
            "tag_name": f"v{version}",
            "name": f"Euphoria {version}",
            "assets": [
                {"name": asset, "browser_download_url": f"https://example.invalid/download/v{version}/{asset}"}
                for asset in [*assets, "SHA256SUMS"]
            ],
        }
        body = json.dumps(release, indent=2)
        (self.api / "tags" / f"v{version}").write_text(body)
        if latest:
            (self.api / "latest").write_text(body)


def linux_app_files(version):
    def appimage(path):
        write_executable(path, FAKE_APPIMAGE)

    return {f"Euphoria-{version}-x86_64.AppImage": appimage}


def macos_app_files(version):
    def disk_image(path):
        source = path.parent / "dmg-source"
        contents = source / "Euphoria.app" / "Contents"
        (contents / "MacOS").mkdir(parents=True)
        write_executable(contents / "MacOS" / "euphoria-desktop", "#!/bin/sh\n")
        (contents / "Info.plist").write_text(
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" '
            '"http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n'
            '<plist version="1.0"><dict>'
            f"<key>CFBundleIdentifier</key><string>{BUNDLE_IDENTIFIER}</string>"
            f"<key>CFBundleShortVersionString</key><string>{version}</string>"
            "</dict></plist>\n"
        )
        subprocess.run(
            ["hdiutil", "create", "-quiet", "-volname", "Euphoria", "-srcfolder", str(source), "-format", "UDZO", str(path)],
            check=True,
        )
        shutil.rmtree(source)

    return {f"Euphoria-{version}.dmg": disk_image}


class InstallScriptTest(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="euphoria-install-test-"))
        self.addCleanup(shutil.rmtree, self.root, ignore_errors=True)
        self.release = FakeRelease(self.root / "release")
        self.prefix = self.root / "prefix"
        self.applications = self.root / "Applications"
        self.home = self.root / "home"
        self.home.mkdir()
        self.temporary = self.root / "tmp"
        self.temporary.mkdir()
        self.shims = self.root / "shims"
        write_executable(self.shims / "uname", FAKE_UNAME)

    def run_script(self, *arguments, system="Linux", machine="x86_64", version=VERSION, extra_path=None, **environment):
        path = os.pathsep.join(
            [str(self.shims), *([str(extra_path)] if extra_path else []), "/usr/bin", "/bin", "/usr/sbin", "/sbin"]
        )
        variables = {
            "PATH": path,
            "HOME": str(self.home),
            "SHELL": "/bin/zsh",
            "TMPDIR": str(self.temporary),
            "FAKE_SYSTEM": system,
            "FAKE_MACHINE": machine,
            "EUPHORIA_INSTALL_DIR": str(self.prefix),
            "EUPHORIA_APP_DIR": str(self.applications),
            "EUPHORIA_RELEASES_API": self.release.api.as_uri(),
            "EUPHORIA_DOWNLOAD_BASE": self.release.directory(version).as_uri(),
            **environment,
        }
        result = subprocess.run(
            ["sh", str(SCRIPT), *arguments], env=variables, capture_output=True, text=True, timeout=120
        )
        self.assertEqual(list(self.temporary.iterdir()), [], "the temporary folder was left behind")
        return result

    def assert_succeeded(self, result):
        self.assertEqual(result.returncode, 0, f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}")

    def manifest(self):
        return (self.prefix / "share" / "euphoria" / "install-manifest.txt").read_text().splitlines()

    def test_linux_install_then_uninstall_removes_only_installed_files(self):
        self.release.add(VERSION, "x86_64-unknown-linux-gnu", app_files=linux_app_files(VERSION))
        unrelated = [
            self.prefix / "bin" / "other-tool",
            self.prefix / "lib" / "libother.so",
            self.prefix / "share" / "applications" / "other.desktop",
        ]
        for path in unrelated:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("not Euphoria's")

        result = self.run_script()
        self.assert_succeeded(result)
        installed = [
            self.prefix / "bin" / "euphoria",
            self.prefix / "lib" / f"libfake.so.{VERSION}",
            self.prefix / "share" / "doc" / "euphoria" / "LICENSE",
            self.prefix / "share" / "doc" / "euphoria" / "notice.txt",
            self.prefix / "bin" / "euphoria-desktop",
            self.prefix / "share" / "applications" / "euphoria.desktop",
            self.prefix / "share" / "icons" / "hicolor" / "256x256" / "apps" / "euphoria.png",
        ]
        for path in installed:
            self.assertTrue(path.exists(), path)
        self.assertTrue(os.access(self.prefix / "bin" / "euphoria-desktop", os.X_OK))
        entry = (self.prefix / "share" / "applications" / "euphoria.desktop").read_text()
        self.assertIn(f'Exec="{self.prefix}/bin/euphoria-desktop"', entry)
        self.assertIn(f"Installed euphoria {VERSION}", result.stdout)
        self.assertEqual(sorted(line.split(" ", 1)[1] for line in self.manifest()), sorted(map(str, installed)))

        result = self.run_script("--uninstall")
        self.assert_succeeded(result)
        for path in installed:
            self.assertFalse(path.exists(), path)
        for path in unrelated:
            self.assertEqual(path.read_text(), "not Euphoria's")
        self.assertFalse((self.prefix / "share" / "euphoria").exists())

    def test_prints_the_path_line_only_when_bin_is_not_on_path(self):
        self.release.add(VERSION, "x86_64-unknown-linux-gnu")
        result = self.run_script("--cli-only")
        self.assert_succeeded(result)
        self.assertIn("is not on your PATH", result.stdout)
        self.assertIn(f"echo 'export PATH=\"{self.prefix}/bin:$PATH\"' >> ~/.zshrc", result.stdout)

        result = self.run_script("--cli-only", extra_path=self.prefix / "bin", SHELL="/bin/bash")
        self.assert_succeeded(result)
        self.assertNotIn("is not on your PATH", result.stdout)

    def test_checksum_mismatch_aborts_before_installing(self):
        self.release.add(VERSION, "x86_64-unknown-linux-gnu", app_files=linux_app_files(VERSION))
        tarball = self.release.directory(VERSION) / f"euphoria-{VERSION}-x86_64-unknown-linux-gnu.tar.gz"
        with tarball.open("ab") as file:
            file.write(b"tampered")
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum mismatch", result.stderr)
        self.assertFalse(self.prefix.exists())

    def test_rerun_upgrades_in_place_and_drops_stale_files(self):
        self.release.add(VERSION, "x86_64-unknown-linux-gnu", app_files=linux_app_files(VERSION))
        self.release.add(NEXT_VERSION, "x86_64-unknown-linux-gnu", app_files=linux_app_files(NEXT_VERSION))

        result = self.run_script(EUPHORIA_VERSION=VERSION)
        self.assert_succeeded(result)
        self.assertIn(f"Installed euphoria {VERSION}", result.stdout)

        result = self.run_script(version=NEXT_VERSION)
        self.assert_succeeded(result)
        self.assertIn(f"Installed euphoria {NEXT_VERSION}", result.stdout)
        self.assertFalse((self.prefix / "lib" / f"libfake.so.{VERSION}").exists())
        self.assertTrue((self.prefix / "lib" / f"libfake.so.{NEXT_VERSION}").exists())
        self.assertNotIn(VERSION, "\n".join(self.manifest()))

    def test_cli_only_rerun_keeps_the_app_in_the_manifest(self):
        self.release.add(VERSION, "x86_64-unknown-linux-gnu", app_files=linux_app_files(VERSION))
        self.assert_succeeded(self.run_script())
        self.assert_succeeded(self.run_script("--cli-only"))
        self.assertIn(f"app {self.prefix}/bin/euphoria-desktop", self.manifest())
        self.assertTrue((self.prefix / "bin" / "euphoria-desktop").exists())

    def test_refuses_to_overwrite_files_it_did_not_install(self):
        self.release.add(VERSION, "x86_64-unknown-linux-gnu")
        existing = self.prefix / "bin" / "euphoria"
        existing.parent.mkdir(parents=True)
        existing.write_text("someone else's")
        result = self.run_script("--cli-only")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("were not installed by this script", result.stderr)
        self.assertEqual(existing.read_text(), "someone else's")
        self.assertFalse((self.prefix / "lib").exists())

    def test_unsupported_platforms_get_a_clear_message(self):
        self.release.add(VERSION, "x86_64-unknown-linux-gnu")
        result = self.run_script(machine="aarch64")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no release build for Linux on aarch64", result.stderr)
        self.assertIn("https://github.com/lvndry/euphoria/releases", result.stderr)
        self.assertFalse(self.prefix.exists())

        result = self.run_script(system="FreeBSD", machine="amd64")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("build from source", result.stderr)

    def test_intel_mac_is_told_there_is_no_build(self):
        self.release.add(VERSION, "aarch64-apple-darwin")
        result = self.run_script(system="Darwin", machine="x86_64")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no build for Intel Macs", result.stderr)
        self.assertFalse(self.prefix.exists())

    def test_unknown_option_and_help(self):
        result = self.run_script("--bogus")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown option --bogus", result.stderr)
        result = self.run_script("--help")
        self.assert_succeeded(result)
        self.assertIn("--uninstall", result.stdout)

    @unittest.skipUnless(sys.platform == "darwin", "needs hdiutil and ditto")
    def test_macos_installs_the_app_from_the_disk_image(self):
        self.release.add(VERSION, "aarch64-apple-darwin", app_files=macos_app_files(VERSION))
        result = self.run_script(system="Darwin", machine="arm64")
        self.assert_succeeded(result)
        app = self.applications / "Euphoria.app"
        self.assertTrue((app / "Contents" / "Info.plist").exists())
        self.assertTrue((self.prefix / "bin" / "euphoria").exists())
        self.assertIn(f"app {app}", self.manifest())
        mounted = subprocess.run(["hdiutil", "info"], capture_output=True, text=True, check=True).stdout
        self.assertNotIn(str(self.root), mounted)

        self.assert_succeeded(self.run_script(system="Darwin", machine="arm64"))
        self.assertTrue(app.exists())

        self.assert_succeeded(self.run_script("--uninstall", system="Darwin", machine="arm64"))
        self.assertFalse(app.exists())
        self.assertTrue(self.applications.exists())


if __name__ == "__main__":
    unittest.main()
