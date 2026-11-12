# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Trace changed package selectors across real Git snapshots."""

import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[2] / ".github/jazz/workflows/code-review/collect_context.py"
SPEC = importlib.util.spec_from_file_location("review_context", SOURCE)
CONTEXT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CONTEXT)


class ReviewContextTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="review context ")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.git("init", "-q")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("config", "user.name", "Review fixture")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "core.quotePath", "true")
        self.write("crates/desktop/Cargo.toml", '[package]\nname="euphoria"\nversion="0.1.0"\n[[bin]]\nname="euphoria-desktop"\n')
        self.write("crates/cli/Cargo.toml", '[package]\nname="euphoria-cli"\nversion="0.1.0"\n[[bin]]\nname="euphoria"\n')
        self.write("screenshots.py", 'command = ["cargo", "build", "-p",\n "euphoria"]\n')
        self.write(".github/ci.yml", 'run: cargo build --package=euphoria\n')
        self.write("guide.md", 'cargo run -p euphoria-cli\ncargo test -p euphoria-core\n')
        self.base = self.commit()

    def git(self, *arguments):
        return subprocess.check_output(["git", "-C", str(self.root), *arguments], stderr=subprocess.PIPE).decode().strip()

    def write(self, name, value):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value)

    def commit(self):
        self.git("add", ".")
        self.git("commit", "--allow-empty", "-qm", "fixture")
        return self.git("rev-parse", "HEAD")

    def rename_package(self):
        path = self.root / "crates/desktop/Cargo.toml"
        path.write_text(path.read_text().replace('name="euphoria"', 'name="euphoria-desktop"', 1))
        return self.commit()

    def test_unchanged_multiline_arrays_and_hidden_ci_callers_are_found(self):
        result = CONTEXT.collect(self.root, self.base, self.rename_package())
        self.assertEqual({item["path"] for item in result["affected_callers"]}, {"screenshots.py", ".github/ci.yml"})
        self.assertTrue(all(not item["changed"] for item in result["affected_callers"]))
        self.assertEqual(result["renamed_contracts"][0]["old"], "euphoria")

    def test_a_moved_and_renamed_package_keeps_its_old_contract(self):
        self.rename_package()
        old = self.root / "crates/desktop/Cargo.toml"
        new = self.root / "crates/new-desktop/Cargo.toml"
        new.parent.mkdir(parents=True)
        old.rename(new)
        result = CONTEXT.collect(self.root, self.base, self.commit())
        self.assertEqual(result["renamed_contracts"], [{
            "kind": "package", "path": "crates/new-desktop/Cargo.toml",
            "old": "euphoria", "new": "euphoria-desktop",
        }])
        self.assertEqual({item["path"] for item in result["affected_callers"]},
                         {"screenshots.py", ".github/ci.yml"})

    def test_package_and_executable_names_remain_distinct(self):
        result = CONTEXT.collect(self.root, self.base, self.rename_package())
        packages = {package["name"]: package["explicit_binaries"] for package in result["head_packages"]}
        self.assertEqual(packages, {"euphoria-cli": ["euphoria"], "euphoria-desktop": ["euphoria-desktop"]})

    def test_corrected_callers_are_not_reported_and_input_code_is_never_run(self):
        self.rename_package()
        self.write("screenshots.py", 'raise RuntimeError("must never execute")\ncommand=["cargo","build","-p","euphoria-desktop"]\n')
        self.write(".github/ci.yml", 'run: cargo build -p euphoria-desktop\n')
        result = CONTEXT.collect(self.root, self.base, self.commit())
        self.assertEqual(result["affected_callers"], [])

    def test_rename_uses_the_diff_merge_base_when_target_branch_diverged(self):
        self.write("crates/desktop/Cargo.toml", '[package]\nname="new-target-name"\nversion="0.1.0"\n')
        target_tip = self.commit()
        self.git("checkout", "-q", "--detach", self.base)
        head = self.rename_package()
        result = CONTEXT.collect(self.root, target_tip, head)
        self.assertEqual(result["renamed_contracts"], [{
            "kind": "package", "path": "crates/desktop/Cargo.toml",
            "old": "euphoria", "new": "euphoria-desktop",
        }])
        self.assertEqual({item["path"] for item in result["affected_callers"]},
                         {"screenshots.py", ".github/ci.yml"})

    def test_compact_short_selector_is_found_without_long_option_or_name_prefix_matches(self):
        self.write("compact.sh", "cargo build -peuphoria\n")
        self.write("not-selectors.txt", "\n".join([
            "cargo build --packageeuphoria", "cargo build --peuphoria",
            "cargo build -peuphoria-desktop", "cargo build -peuphoria_cli",
            "prefix-peuphoria", "cargo build --package euphoria-cli",
        ]))
        self.commit()
        result = CONTEXT.collect(self.root, self.base, self.rename_package())
        paths = {item["path"] for item in result["affected_callers"]}
        self.assertIn("compact.sh", paths)
        self.assertNotIn("not-selectors.txt", paths)

    def test_unicode_changed_resource_sites_come_from_head_not_dirty_worktree(self):
        self.write("é.py", "raise RuntimeError('must not execute')\nimport subprocess\nchild = subprocess.Popen(['fixture'])\nchild.wait()\n")
        head = self.commit()
        self.write("é.py", "raise RuntimeError('dirty worktree must not execute')\nsubprocess.run(['dirty'])\n")
        result = CONTEXT.collect(self.root, self.base, head)
        self.assertIn("é.py", result["changed_files"])
        self.assertEqual(result["resource_sites"], [
            {"path": "é.py", "line": 3, "call": "subprocess.Popen",
             "source": "child = subprocess.Popen(['fixture'])"},
            {"path": "é.py", "line": 4, "call": "child.wait", "source": "child.wait()"},
        ])

    def test_changed_python_resource_inventory_covers_acquisition_and_teardown(self):
        self.write("resources.py", "\n".join([
            "raise RuntimeError('must not execute')", "import subprocess, tempfile, shutil",
            "with tempfile.TemporaryDirectory() as folder:",
            "    child = subprocess.Popen(['fixture'])", "    try:",
            "        subprocess.run(['fixture'])", "    finally:",
            "        child.terminate()", "        child.wait()", "        shutil.rmtree(folder)",
        ]) + "\n")
        self.write("unchanged.py", "subprocess.Popen(['unchanged'])\n")
        base = self.commit()
        self.write("resources.py", (self.root / "resources.py").read_text() + "# changed\n")
        result = CONTEXT.collect(self.root, base, self.commit())
        self.assertEqual([(site["line"], site["call"]) for site in result["resource_sites"]], [
            (3, "tempfile.TemporaryDirectory"), (4, "subprocess.Popen"),
            (6, "subprocess.run"), (8, "child.terminate"), (9, "child.wait"),
            (10, "shutil.rmtree"),
        ])
        self.assertTrue(all(site["path"] == "resources.py" for site in result["resource_sites"]))


if __name__ == "__main__":
    unittest.main()
