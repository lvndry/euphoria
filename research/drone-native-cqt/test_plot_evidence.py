# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Hashed reports must still agree on case identities before plotting."""

import json
import tempfile
import unittest
from pathlib import Path

from evidence_io import digest
from plot_results import plot, validate_case_order


class PlotEvidenceTests(unittest.TestCase):
    def test_matching_case_ids_preserve_the_original_order(self) -> None:
        reference = [{"id": "silence"}, {"id": "tone"}]
        validate_case_order(reference, reference)

    def test_plot_rejects_invalid_case_alignment_even_with_valid_hashes(self) -> None:
        for ids in [
            ["tone", "silence"],
            ["silence", "substitute"],
            ["silence", "silence"],
            ["silence"],
            ["silence", None],
        ]:
            with self.subTest(ids=ids), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                evidence = root / "evidence"
                evidence.mkdir()
                reference = [{"id": "silence"}, {"id": "tone"}]
                files = {}
                for name, cases in [
                    ("reference-summary.json", reference),
                    ("owner-d8424bf.json", [{"id": identity} for identity in ids]),
                    ("exported-d8424bf.json", reference),
                ]:
                    raw = json.dumps({"cases": cases}).encode()
                    (evidence / name).write_bytes(raw)
                    files[name] = digest(raw)
                (evidence / "index.json").write_text(json.dumps({"files": files}))
                with self.assertRaisesRegex(ValueError, "case"):
                    plot(root)
                self.assertFalse((root / "cqt-feasibility.png").exists())

    def test_duplicate_reference_ids_are_rejected(self) -> None:
        cases = [{"id": "silence"}, {"id": "silence"}]
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            validate_case_order(cases, cases)


if __name__ == "__main__":
    unittest.main()
