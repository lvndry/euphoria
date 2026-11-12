"""Regression for the measured RSS unit erratum, without runtime/model downloads."""

from __future__ import annotations

import unittest
from pathlib import Path

from summarize import native_peak_bytes, summarize, verify_logits


class SummaryTests(unittest.TestCase):
    def test_native_kib_is_converted_exactly_once(self) -> None:
        raw = {
            "peak_rss_bytes": 105408,
            "format": "k10-composed-pipeline-v1",
            "identity": {
                "pipeline_source_sha256": "77bc7059ce96f81cac38751590f9fca7fba7a4728460d5d6215dcd841e1af5ec"
            },
        }
        self.assertEqual(native_peak_bytes(raw), 107937792)
        raw["format"] = "k10-composed-pipeline-v2"
        with self.assertRaisesRegex(ValueError, "frozen legacy"):
            native_peak_bytes(raw)

    def test_frozen_evidence_integrity_and_counts(self) -> None:
        result = summarize(Path(__file__).resolve().parent)
        self.assertEqual(result["qualification"]["tracks"], 376)
        self.assertEqual(len(result["rows"]), 20)
        self.assertTrue(result["mac"]["passed"])
        self.assertEqual(
            result["aggregate"]["chunked"]["maximum_peak_rss_bytes"], 477.5 * 1024**2
        )

    def test_finite_tolerance_and_key_are_independent_requirements(self) -> None:
        reference = [1.000001, 1.0] + [0.0] * 22
        verify_logits(reference, reference)
        with self.assertRaisesRegex(ValueError, "nonfinite"):
            verify_logits([float("nan")] + reference[1:], reference)
        with self.assertRaisesRegex(ValueError, "tolerance"):
            verify_logits([2.0] + reference[1:], reference)
        with self.assertRaisesRegex(ValueError, "key disagreement"):
            verify_logits([1.0, 1.000001] + [0.0] * 22, reference)


if __name__ == "__main__":
    unittest.main()
