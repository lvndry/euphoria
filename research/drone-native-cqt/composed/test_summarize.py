"""Regression for the measured RSS unit erratum, without runtime/model downloads."""

from __future__ import annotations

import unittest
from pathlib import Path

from summarize import native_peak_bytes, summarize


class SummaryTests(unittest.TestCase):
    def test_native_kib_is_converted_exactly_once(self) -> None:
        self.assertEqual(native_peak_bytes({"peak_rss_bytes": 105408}), 107937792)

    def test_frozen_evidence_integrity_and_counts(self) -> None:
        result = summarize(Path(__file__).resolve().parent)
        self.assertEqual(result["qualification"]["tracks"], 376)
        self.assertEqual(len(result["rows"]), 20)
        self.assertTrue(result["mac"]["passed"])
        self.assertEqual(
            result["aggregate"]["chunked"]["maximum_peak_rss_bytes"], 477.5 * 1024**2
        )


if __name__ == "__main__":
    unittest.main()
