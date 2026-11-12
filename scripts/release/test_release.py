#!/usr/bin/env python3
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests the release scripts:

python3 -m unittest discover -s scripts/release -p 'test_*.py'
"""

import datetime
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import next_version
import plan_release
import release_notes
import set_version

REPOSITORY = pathlib.Path(__file__).resolve().parents[2]


def day(text):
    return datetime.date.fromisoformat(text)


class NextVersionTests(unittest.TestCase):
    def test_the_first_release_of_a_month_is_zero(self):
        self.assertEqual(next_version.next_version([], day("2026-11-11")), "2026.11.0")

    def test_releases_in_the_same_month_count_up(self):
        tags = ["v2026.11.0", "v2026.11.1"]
        self.assertEqual(
            next_version.next_version(tags, day("2026-11-30")), "2026.11.2"
        )
        many = [f"v2026.11.{number}" for number in range(11)]
        self.assertEqual(
            next_version.next_version(many, day("2026-11-30")), "2026.11.11"
        )

    def test_a_new_month_and_a_new_year_start_again_at_zero(self):
        self.assertEqual(
            next_version.next_version(["v2026.11.4"], day("2026-12-01")), "2026.12.0"
        )
        self.assertEqual(
            next_version.next_version(["v2026.12.2"], day("2027-01-01")), "2027.1.0"
        )

    def test_tags_that_are_not_calendar_versions_are_ignored(self):
        tags = [
            "v0.1.0",
            "vfoo",
            "2026.11.5",
            "v2026.011.0",
            "v2026.11.01",
            "v2026.13.0",
            "v2026.0.0",
            "v26.11.0",
            "v2026.11",
            "v2026.11.3-rc1",
        ]
        self.assertEqual(
            next_version.next_version(tags, day("2026-11-11")), "2026.11.0"
        )
        for tag in tags:
            self.assertIsNone(next_version.parse_tag(tag), tag)

    def test_a_tag_from_a_later_month_is_refused(self):
        with self.assertRaisesRegex(SystemExit, "later month"):
            next_version.next_version(["v2027.1.0"], day("2026-12-31"))
        with self.assertRaisesRegex(SystemExit, "later month"):
            next_version.next_version(["v2026.12.0"], day("2026-11-30"))


# The files a release carries, written out rather than taken from
# plan_release, so that a change there has to be made here too.
RELEASE_FILES_OF_2026_11_0 = {
    "Euphoria-2026.11.0.dmg",
    "Euphoria.dmg",
    "euphoria-2026.11.0-aarch64-apple-darwin.tar.gz",
    "SHA256SUMS",
}
LINUX_FILES_OF_2026_11_0 = {
    "Euphoria-2026.11.0-x86_64.AppImage",
    "Euphoria-x86_64.AppImage",
    "euphoria-2026.11.0-x86_64-unknown-linux-gnu.tar.gz",
}


def finished(tag):
    """A release as the workflow leaves it when it finishes."""
    version = tag[1:]
    return {
        "assets": sorted(plan_release.expected_assets(version)),
        "title": f"Euphoria {version}",
        "notes": f"Euphoria {version}.\n\n{release_notes.COMPLETE_MARKER}\n",
        "is_latest": True,
    }


class PlanTests(unittest.TestCase):
    def test_a_failed_planner_stops_the_workflow_step(self):
        workflow = (REPOSITORY / ".github/workflows/release.yml").read_text()
        step = workflow.split("      - name: Choose a new version or the one to finish\n", 1)[1]
        script = re.search(r"(?m)^        run: \|\n((?:          .*\n)+)", step)[1]
        script = "\n".join(line[10:] for line in script.splitlines())
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            python = root / "python3"
            python.write_text("#!/bin/sh\nexit 42\n")
            python.chmod(0o755)
            result = subprocess.run(
                ["bash", "-e", "-c", script],
                env={**os.environ, "PATH": f"{root}:{os.environ['PATH']}",
                     "GITHUB_OUTPUT": str(root / "output")},
                capture_output=True, text=True, check=False,
            )
        self.assertEqual(result.returncode, 42, result.stderr)

    def test_a_release_carries_these_files(self):
        self.assertEqual(
            plan_release.expected_assets("2026.11.0"),
            RELEASE_FILES_OF_2026_11_0 | LINUX_FILES_OF_2026_11_0,
        )
        self.assertEqual(
            plan_release.expected_assets("2026.11.0", linux=False),
            RELEASE_FILES_OF_2026_11_0,
        )

    def test_a_release_tagged_before_linux_packaging_needs_no_linux_files(self):
        def macos_only(tag):
            return {**finished(tag), "assets": sorted(RELEASE_FILES_OF_2026_11_0)}

        self.assertEqual(
            plan_release.plan(
                ["v2026.11.0"], macos_only, day("2026-11-20"), lambda tag: False
            ),
            ("new", "2026.11.1", "v2026.11.0"),
        )
        self.assertEqual(
            plan_release.plan(
                ["v2026.11.0"], macos_only, day("2026-11-20"), lambda tag: True
            ),
            ("resume", "2026.11.0", ""),
        )

    def test_the_tag_of_the_first_release_has_no_linux_packaging(self):
        if subprocess.run(
            ["git", "-C", str(REPOSITORY), "rev-parse", "--verify", "--quiet",
             "v2026.11.0^{commit}"],
            capture_output=True, check=False,
        ).returncode != 0:  # fmt: skip
            self.skipTest("the tag v2026.11.0 is not in this clone")
        original = pathlib.Path.cwd()
        os.chdir(REPOSITORY)
        try:
            self.assertFalse(plan_release.packages_linux("v2026.11.0"))
            self.assertTrue(plan_release.packages_linux("HEAD"))
        finally:
            os.chdir(original)

    def test_the_workflow_builds_the_files_a_release_carries(self):
        workflow = (REPOSITORY / ".github" / "workflows" / "release.yml").read_text()
        package = (REPOSITORY / "scripts" / "release" / "package-cli.sh").read_text()
        # The disk image, its copy without the version, and the checksums
        # are gathered into dist/ by the workflow.
        self.assertIn(
            'cp "target/release/bundle/Euphoria-$VERSION.dmg" dist/\n', workflow
        )
        self.assertIn(
            'cp "target/release/bundle/Euphoria-$VERSION.dmg" dist/Euphoria.dmg',
            workflow,
        )
        self.assertIn("shasum -a 256 -- * > SHA256SUMS", workflow)
        self.assertIn("./scripts/release/package-cli.sh dist", workflow)
        # The tarball is named by package-cli.sh, built on an Apple Silicon
        # runner, whose Rust host is aarch64-apple-darwin.
        self.assertIn('name="euphoria-$version-$target"', package)
        self.assertIn('"$output/$name.tar.gz"', package)
        self.assertIn("runs-on: macos-15", workflow)
        built = {
            "Euphoria-2026.11.0.dmg",
            "Euphoria.dmg",
            "SHA256SUMS",
            "euphoria-2026.11.0-aarch64-apple-darwin.tar.gz",
        }
        self.assertEqual(built, RELEASE_FILES_OF_2026_11_0)
        # The Linux files are built on an x86_64 runner, whose Rust host is
        # x86_64-unknown-linux-gnu, and bundle-linux.sh names the AppImage by
        # the machine.
        bundle = (REPOSITORY / "scripts" / "bundle-linux.sh").read_text()
        self.assertIn("runs-on: ubuntu-22.04", workflow)
        self.assertIn('appimage="$output/Euphoria-$version-$architecture.AppImage"', bundle)
        self.assertIn(
            'cp "target/release/bundle/Euphoria-$VERSION-x86_64.AppImage" dist/\n',
            workflow,
        )
        self.assertIn(
            'cp "target/release/bundle/Euphoria-$VERSION-x86_64.AppImage" dist/Euphoria-x86_64.AppImage',
            workflow,
        )
        self.assertIn("sha256sum -- * > SHA256SUMS", workflow)
        self.assertIn("cat dist-linux/SHA256SUMS >> dist/SHA256SUMS", workflow)
        self.assertIn('mv "$file" dist/', workflow)
        # Everything in dist/ is published.
        self.assertIn('gh release create "v$VERSION" dist/*', workflow)
        self.assertIn('gh release upload "v$VERSION" dist/* --clobber', workflow)

    def test_without_releases_the_plan_is_a_new_version(self):
        self.assertEqual(
            plan_release.plan([], lambda tag: None, day("2026-11-11")),
            ("new", "2026.11.0", ""),
        )

    def test_a_finished_newest_release_leads_to_the_next_version(self):
        tags = ["v2026.11.0", "v2026.11.1"]
        self.assertEqual(
            plan_release.plan(tags, finished, day("2026-11-20")),
            ("new", "2026.11.2", "v2026.11.1"),
        )

    def test_a_tag_without_a_release_is_resumed(self):
        tags = ["v2026.11.0", "v2026.11.1"]

        def state(tag):
            return None if tag == "v2026.11.1" else finished(tag)

        # Resuming does not depend on the date, so a run in the next month
        # still finishes the stranded version.
        self.assertEqual(
            plan_release.plan(tags, state, day("2026-12-02")),
            ("resume", "2026.11.1", "v2026.11.0"),
        )

    def test_a_release_left_unfinished_in_any_way_is_resumed(self):
        unfinished = {
            "a missing file": {"assets": ["Euphoria.dmg", "Euphoria-2026.11.0.dmg"]},
            "the title gh gives by default": {"title": "v2026.11.0"},
            "notes without the closing marker": {"notes": "Euphoria 2026.11.0.\n"},
            "empty notes": {"notes": ""},
            "not marked as the latest": {"is_latest": False},
        }
        for reason, change in unfinished.items():
            state = {**finished("v2026.11.0"), **change}
            self.assertEqual(
                plan_release.plan(
                    ["v2026.11.0"], lambda tag, state=state: state, day("2026-11-20")
                ),
                ("resume", "2026.11.0", ""),
                reason,
            )

    def test_the_notes_the_workflow_writes_count_as_finished(self):
        folder = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, folder)
        subprocess.run(["git", "-C", str(folder), "init", "--quiet"], check=True)
        subprocess.run(
            [
                "git",
                "-C",
                str(folder),
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--quiet",
                "--allow-empty",
                "--message",
                "feat: a thing (#1)",
            ],
            check=True,
        )
        original = pathlib.Path.cwd()
        os.chdir(folder)
        try:
            written = release_notes.notes(
                "2026.11.0", None, "HEAD", "owner/repository", "15.0"
            )
        finally:
            os.chdir(original)
        state = {**finished("v2026.11.0"), "notes": written}
        self.assertTrue(plan_release.is_complete(state, "2026.11.0"))

    def test_only_the_newest_tag_is_looked_at(self):
        asked = []

        def state(tag):
            asked.append(tag)
            return finished(tag)

        plan_release.plan(
            ["v2026.10.3", "v2026.11.0", "v0.1.0"], state, day("2026-11-20")
        )
        self.assertEqual(asked, ["v2026.11.0"])


class SetVersionTests(unittest.TestCase):
    def setUp(self):
        self.folder = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.folder)
        files = ["Cargo.toml", "Cargo.lock"] + [
            str(path.relative_to(REPOSITORY))
            for folder in set_version.PACKAGE_FOLDERS
            for path in REPOSITORY.glob(f"{folder}/*/Cargo.toml")
        ]
        self.files = sorted(files)
        for name in self.files:
            target = self.folder / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPOSITORY / name, target)

    def changed_lines(self, name):
        before = (REPOSITORY / name).read_text().splitlines()
        after = (self.folder / name).read_text().splitlines()
        self.assertEqual(len(before), len(after), name)
        return [
            (old, new) for old, new in zip(before, after, strict=True) if old != new
        ]

    def test_only_the_versions_of_the_workspace_packages_change(self):
        names = set_version.set_version(self.folder, "2026.11.0")
        self.assertEqual(
            sorted(names),
            ["euphoria-bench", "euphoria-cli", "euphoria-core", "euphoria-desktop"],
        )
        for name in self.files:
            changes = self.changed_lines(name)
            if name == "Cargo.lock":
                self.assertEqual(len(changes), len(names))
                for old, new in changes:
                    self.assertTrue(old.startswith('version = "'), old)
                    self.assertEqual(new, 'version = "2026.11.0"')
            elif name == "Cargo.toml":
                self.assertEqual(len(changes), 1)
                self.assertTrue(
                    changes[0][1].startswith('euphoria-core = { version = "2026.11.0"')
                )
            else:
                self.assertEqual(len(changes), 1, name)
                self.assertEqual(changes[0][1], 'version = "2026.11.0"', name)

    def test_a_second_run_with_the_same_version_changes_nothing(self):
        set_version.set_version(self.folder, "2026.11.0")
        first = {name: (self.folder / name).read_text() for name in self.files}
        set_version.set_version(self.folder, "2026.11.0")
        for name in self.files:
            self.assertEqual((self.folder / name).read_text(), first[name], name)

    def test_a_version_that_is_not_three_numbers_is_refused(self):
        for version in ["2026.11", "2026.11.01", "v2026.11.0", "2026.11.0-rc1", ""]:
            with self.assertRaises(SystemExit, msg=version):
                set_version.set_version(self.folder, version)
        for name in self.files:
            self.assertEqual(self.changed_lines(name), [], name)


class ReleaseNotesTests(unittest.TestCase):
    def setUp(self):
        self.folder = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.folder)
        self.git("init", "--quiet", "--initial-branch=main")

    def git(self, *arguments):
        return subprocess.run(
            [
                "git",
                "-C",
                str(self.folder),
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                *arguments,
            ],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()

    def commit(self, message):
        self.git("commit", "--quiet", "--allow-empty", "--message", message)

    def notes(self, since=None):
        original = pathlib.Path.cwd()
        os.chdir(self.folder)
        try:
            return release_notes.notes("2026.11.1", since, "HEAD", "owner/repository")
        finally:
            os.chdir(original)

    def test_commits_are_grouped_and_linked(self):
        self.commit("feat(core): add a thing (#12)")
        self.commit("fix: mend a thing (#13)")
        self.commit("perf(core): speed a thing up")
        self.commit("docs: explain a thing (#14)")
        self.commit("chore: tidy (#15)")
        self.commit("not a conventional subject")
        notes = self.notes()
        self.assertIn(
            "## Features\n\n- **core:** Add a thing ([#12](https://github.com/owner/repository/pull/12))",
            notes,
        )
        self.assertIn("## Fixes\n\n- Mend a thing ([#13]", notes)
        self.assertIn("## Performance\n\n- **core:** Speed a thing up ([", notes)
        self.assertIn("## Documentation\n\n- Explain a thing ([#14]", notes)
        self.assertNotIn("Tidy", notes)
        self.assertNotIn("not a conventional", notes)

    def test_a_breaking_change_shows_its_description_under_its_subject(self):
        self.commit(
            "feat(cli)!: rename the export command (#20)\n\n"
            "The old name is gone.\n\n"
            "BREAKING CHANGE: `euphoria export` is now `euphoria export rekordbox`;\n"
            "scripts that call the old name must be updated.\n"
            "Refs: #19"
        )
        self.commit(
            "fix(core): change a default (#21)\n\nBREAKING-CHANGE: the cache moves to a new folder."
        )
        self.commit("refactor(core)!: drop an old function")
        notes = self.notes()
        breaking = notes.split("## Breaking changes\n\n")[1].split("\n## ")[0]
        self.assertIn(
            "- **cli:** Rename the export command ([#20](https://github.com/owner/repository/pull/20))\n"
            "  `euphoria export` is now `euphoria export rekordbox`; scripts that call the old name must be updated.",
            breaking,
        )
        self.assertNotIn("Refs", breaking)
        self.assertIn(
            "- **core:** Change a default ([#21](https://github.com/owner/repository/pull/21))\n"
            "  the cache moves to a new folder.",
            breaking,
        )
        self.assertIn("- **core:** Drop an old function ([", breaking)

    def test_the_notes_name_the_minimum_macos_and_end_with_the_marker(self):
        self.commit("feat: a thing (#1)")
        original = pathlib.Path.cwd()
        os.chdir(self.folder)
        try:
            with_minimum = release_notes.notes(
                "2026.11.1", None, "HEAD", "owner/repository", "15.0"
            )
            without = release_notes.notes("2026.11.1", None, "HEAD", "owner/repository")
        finally:
            os.chdir(original)
        self.assertIn(
            "It needs a Mac with Apple Silicon and macOS 15.0 or later.", with_minimum
        )
        self.assertIn("It needs a Mac with Apple Silicon.", without)
        self.assertTrue(with_minimum.rstrip().endswith(release_notes.COMPLETE_MARKER))

    def test_the_notes_name_the_minimum_glibc_only_when_given(self):
        self.commit("feat: a thing (#1)")
        original = pathlib.Path.cwd()
        os.chdir(self.folder)
        try:
            with_linux = release_notes.notes(
                "2026.11.1", None, "HEAD", "owner/repository", "15.0", "2.35"
            )
            without = release_notes.notes(
                "2026.11.1", None, "HEAD", "owner/repository", "15.0"
            )
        finally:
            os.chdir(original)
        self.assertIn("`Euphoria-2026.11.1-x86_64.AppImage`", with_linux)
        self.assertIn(
            "`euphoria-2026.11.1-x86_64-unknown-linux-gnu.tar.gz`", with_linux
        )
        self.assertIn("with glibc 2.35 or later", with_linux)
        self.assertNotIn("glibc", without)
        self.assertTrue(with_linux.rstrip().endswith(release_notes.COMPLETE_MARKER))

    def test_only_commits_since_the_previous_tag_are_counted(self):
        self.commit("feat: before (#1)")
        self.git("tag", "v2026.11.0")
        self.commit("feat: after (#2)")
        notes = self.notes(since="v2026.11.0")
        self.assertIn("After", notes)
        self.assertNotIn("Before", notes)
        self.assertIn("compare/v2026.11.0...v2026.11.1", notes)


if __name__ == "__main__":
    unittest.main()
