# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Regression tests: uv run --with numpy --with scipy python -m unittest discover -s bench."""

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import numpy


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


def load_fitter():
    specification = importlib.util.spec_from_file_location("fit_energy_model", SCRIPT)
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


class FunctionTests(unittest.TestCase):
    def setUp(self):
        self.fitter = load_fitter()
        rows = numpy.arange(40, dtype=float)
        self.tuning = {"reference": rows + 1, "onset_rate": rows * 0.2, "spectral_flux": rows * 0.001}
        self.spread = {"reference": rows, "onset_rate": rows * 0.1, "spectral_flux": rows * 0.0005}
        self.inputs = ["onset_rate", "spectral_flux"]

    def test_fold_anchors_ignore_the_rows_that_the_fold_scores(self):
        training = numpy.arange(40) % 5 != 0
        before = self.fitter.fold_anchors(self.tuning, self.spread, self.inputs, training)
        changed = {name: values.copy() for name, values in self.tuning.items()}
        changed["onset_rate"][~training] = 1.0e6
        changed["spectral_flux"][~training] = -1.0e6
        after = self.fitter.fold_anchors(changed, self.spread, self.inputs, training)
        self.assertEqual(before, after)
        expected = self.fitter.choose_anchors(
            [self.fitter.subset(self.tuning, training), self.spread], self.inputs
        )
        self.assertEqual(before, expected)

    def test_whole_tuning_anchors_do_see_every_row(self):
        changed = {name: values.copy() for name, values in self.tuning.items()}
        changed["onset_rate"][numpy.arange(40) % 5 == 0] = 1.0e6
        whole = self.fitter.choose_anchors([self.tuning, self.spread], self.inputs)
        whole_changed = self.fitter.choose_anchors([changed, self.spread], self.inputs)
        self.assertNotEqual(whole["onset_rate"], whole_changed["onset_rate"])

    def test_a_feature_constant_in_a_fold_gives_a_diagnostic(self):
        tuning = dict(self.tuning)
        tuning["onset_rate"] = numpy.where(numpy.arange(40) < 39, 2.0, 3.0)
        spread = dict(self.spread)
        spread["onset_rate"] = numpy.full(40, 2.0)
        training = numpy.arange(40) < 39
        with self.assertRaisesRegex(ValueError, "in a cross validation fold, onset_rate"):
            self.fitter.fold_anchors(tuning, spread, ["onset_rate"], training)

    def test_cross_validation_runs_with_fold_anchors(self):
        folds = numpy.arange(40) % self.fitter.FOLD_COUNT
        value = self.fitter.cross_validated_correlation(self.tuning, self.spread, self.inputs, folds)
        self.assertGreater(value, 0.9)

    def test_scores_refuse_equal_anchors_and_missing_values(self):
        with self.assertRaisesRegex(ValueError, "unusable anchors"):
            self.fitter.scores(self.tuning, ["onset_rate"], {"onset_rate": (2.0, 2.0)})
        with self.assertRaisesRegex(ValueError, "no anchors"):
            self.fitter.scores(self.tuning, ["onset_rate"], {})
        missing = dict(self.tuning)
        missing["onset_rate"] = numpy.where(numpy.arange(40) == 3, numpy.nan, 1.0)
        with self.assertRaisesRegex(ValueError, "missing or infinite values"):
            self.fitter.scores(missing, ["onset_rate"], {"onset_rate": (0.0, 2.0)})


if __name__ == "__main__":
    unittest.main()
