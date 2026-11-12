# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Exercise the real workflow preflight against trusted and proposed scripts."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

WORKFLOW = Path(__file__).resolve().parents[2] / ".github/workflows/jazz.yml"
COLLECTOR = ".github/jazz/workflows/code-review/collect_context.py"


def preflight():
    step = WORKFLOW.read_text().split("      - name: Map changed contracts and their callers\n", 1)[1]
    block = step.split("        run: |\n", 1)[1].split("\n      - name:", 1)[0]
    return "\n".join(line[10:] for line in block.splitlines() if line.strip())


class ReviewPreflightTests(unittest.TestCase):
    def git(self, root, *arguments):
        return subprocess.check_output(["git", "-C", str(root), *arguments], stderr=subprocess.PIPE).decode().strip()

    def commit(self, root):
        self.git(root, "add", ".")
        self.git(root, "commit", "--allow-empty", "-qm", "fixture")
        return self.git(root, "rev-parse", "HEAD")

    def check_case(self, base_has_collector):
        with tempfile.TemporaryDirectory(prefix="review preflight ") as temporary:
            root = Path(temporary) / "repo"
            root.mkdir()
            self.git(root, "init", "-q")
            self.git(root, "config", "user.email", "fixture@example.invalid")
            self.git(root, "config", "user.name", "Review fixture")
            self.git(root, "config", "commit.gpgsign", "false")
            collector = root / COLLECTOR
            collector.parent.mkdir(parents=True)
            if base_has_collector:
                collector.write_text('import json\nprint(json.dumps({"trusted_base": True}))\n')
            base = self.commit(root)
            marker = Path(temporary) / "proposed-script-executed"
            collector.write_text(f'from pathlib import Path\nPath({str(marker)!r}).touch()\nprint("{{}}")\n')
            head = self.commit(root)
            output = Path(temporary) / "context.json"
            script = preflight().replace("/tmp/jazz-contract-context.json", str(output))
            # Quote the substituted output path exactly as a workflow-provided path.
            script = script.replace(str(output), '"' + str(output) + '"')
            result = subprocess.run(
                ["bash", "-eu", "-c", script], cwd=root,
                env={**os.environ, "PR_BASE_SHA": base, "PR_HEAD_SHA": head,
                     "GITHUB_WORKSPACE": str(root), "RUNNER_TEMP": temporary},
                capture_output=True, text=True, timeout=10,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(marker.exists(), "the proposed collector was executed")
            context = json.loads(output.read_text())
            if base_has_collector:
                self.assertEqual(context, {"trusted_base": True})
            else:
                self.assertIn("manual caller searches", context["error"])

    def test_proposed_script_is_not_executed(self):
        self.check_case(True)

    def test_missing_base_collector_falls_back_without_executing_head(self):
        self.check_case(False)


if __name__ == "__main__":
    unittest.main()
