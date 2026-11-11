# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Regression tests: uv run --with numpy --with scipy python -m unittest discover -s bench."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("fit_energy_model.py")


class FittingTests(unittest.TestCase):
    def run_fit(self, *, tempo=None, constant=False, descending=False, force=None,
                held_out_tempo=None, spread_constant=False):
        with tempfile.TemporaryDirectory() as directory:
            paths = {}
            for label in ("tuning", "held-out", "spread"):
                path = Path(directory) / f"{label}.tsv"
                rows = ["reference\tonset_rate\ttempo\tloudness_range"]
                for index in range(20):
                    reference = 20 - index if descending else index + 1
                    onset = 5 if label == "spread" and spread_constant else index + 1
                    beat = 100 + index * 3
                    if tempo is not None and index == 3:
                        beat = tempo
                    if held_out_tempo is not None and label == "held-out" and index == 3:
                        beat = held_out_tempo
                    rows.append(f"{reference}\t{onset}\t{beat}\t{4 if constant else index + 1}")
                path.write_text("\n".join(rows) + "\n", encoding="utf-8")
                paths[label] = str(path)
            command = [sys.executable, str(SCRIPT), "--tuning", paths["tuning"],
                       "--held-out", paths["held-out"], "--spread", paths["spread"]]
            if force:
                command += ["--force", force]
            return subprocess.run(command, capture_output=True, text=True, check=False)

    def model(self, result):
        self.assertEqual(result.returncode, 0, result.stderr)
        start = result.stdout.index('{')
        return json.JSONDecoder().raw_decode(result.stdout[start:])[0]

    def assert_diagnostic(self, result, message):
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(message, result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        self.assertNotIn('"anchors"', result.stdout)

    def test_successful_fit_emits_a_finite_model(self):
        model = self.model(self.run_fit())
        self.assertLess(model["combined_low"], model["combined_high"])
        self.assertAlmostEqual(sum(a["weight"] for a in model["anchors"].values()), 1.0)

    def test_missing_tempo_excludes_it_without_losing_the_other_features(self):
        result = self.run_fit(tempo="")
        model = self.model(result)
        self.assertIn("skip tempo: missing values", result.stdout)
        self.assertNotIn("tempo", model["anchors"])

    def test_constant_feature_is_excluded(self):
        result = self.run_fit(constant=True)
        model = self.model(result)
        self.assertIn("skip loudness_range: equal low and high anchors", result.stdout)
        self.assertNotIn("loudness_range", model["anchors"])

    def test_no_improving_feature_gives_a_diagnostic(self):
        # Constant loudness range leaves only rising features, all of which
        # run against this reference and cannot improve the fit.
        result = self.run_fit(descending=True, constant=True)
        self.assert_diagnostic(result, "no usable features selected")

    def test_forced_zero_weight_fit_gives_a_diagnostic(self):
        self.assert_diagnostic(self.run_fit(descending=True, force="onset_rate"),
                               "no positive feature weights")

    def test_forcing_a_constant_feature_gives_a_diagnostic(self):
        self.assert_diagnostic(self.run_fit(constant=True, force="loudness_range"),
                               "forced features are unavailable: loudness_range")

    def test_infinite_input_is_rejected(self):
        self.assert_diagnostic(self.run_fit(tempo="inf"), "contains an infinite value")

    def test_held_out_missing_tempo_does_not_change_the_fitted_model(self):
        complete = self.model(self.run_fit(force="tempo"))
        missing = self.run_fit(force="tempo", held_out_tempo="")
        self.assertEqual(self.model(missing), complete)
        self.assertIn("held out: omitted 1 tracks", missing.stdout)
        self.assertIn("19 tracks", missing.stdout)

    def test_constant_spread_is_rejected(self):
        self.assert_diagnostic(self.run_fit(force="onset_rate", spread_constant=True),
                               "spread data must produce distinct, finite model anchors")


if __name__ == "__main__":
    unittest.main()
