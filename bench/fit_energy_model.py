# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# /// script
# requires-python = ">=3.12"
# dependencies = ["numpy", "scipy"]
# ///
"""Fits the anchors and weights of the energy model.

Reads the files that `euphoria-bench energy LABELS --dump FILE` writes: one
line per track with a reference, the rating and every feature, and no track
names.

    uv run bench/fit_energy_model.py \\
        --tuning tuning.dump.tsv --held-out held-out.dump.tsv \\
        --spread giantsteps.dump.tsv

The model is the one in `crates/euphoria-core/src/energy/model.rs`: every
feature becomes a score from 0 to 1 between a low and a high anchor, the
scores are averaged with weights, and the average is spread over the ratings.

- Anchors are the 3rd and 97th percentile of each feature in the tuning file
  and in the spread file, taking the wider of the two, so that neither kind
  of music runs into an anchor.
- Weights are a least squares fit of the reference on the scores of the
  tuning file, with every weight held at zero or above. That keeps the
  direction of every feature: louder and busier never rate lower.
- Features are added one at a time, best first, for as long as five fold
  cross validation on the tuning file improves. The held out file takes no
  part in any choice and is only scored at the end.
- The range of the average that is spread over the ratings comes from the
  2nd and 98th percentile of the spread file.

The numbers printed at the end go into `ENERGY_MODEL`, rounded.
"""

import argparse
import json

import numpy
from scipy.optimize import nnls
from scipy.stats import spearmanr

ANCHOR_PERCENTILES = (3, 97)
SPREAD_PERCENTILES = (2, 98)
FOLD_COUNT = 5
# A feature joins the model only when it improves the cross validated
# correlation by at least this much.
MINIMUM_GAIN = 0.003


def read_dump(path):
    with open(path, encoding="utf-8") as handle:
        header = handle.readline().rstrip("\n").split("\t")
        rows = [line.rstrip("\n").split("\t") for line in handle if line.strip()]
    columns = {}
    for index, name in enumerate(header):
        columns[name] = numpy.array(
            [float(row[index]) if row[index] else numpy.nan for row in rows]
        )
    return columns


def choose_anchors(dumps, inputs):
    anchors = {}
    for name in inputs:
        lows, highs = [], []
        for dump in dumps:
            values = dump[name][~numpy.isnan(dump[name])]
            low, high = numpy.percentile(values, ANCHOR_PERCENTILES)
            lows.append(low)
            highs.append(high)
        anchors[name] = (min(lows), max(highs))
    return anchors


def scores(dump, inputs, anchors):
    columns = []
    for name in inputs:
        low, high = anchors[name]
        columns.append(numpy.clip((dump[name] - low) / (high - low), 0.0, 1.0))
    return numpy.column_stack(columns)


def fit_weights(score_matrix, reference):
    """Least squares with weights at zero or above and a free offset."""
    ones = numpy.ones((len(reference), 1))
    design = numpy.hstack([score_matrix, ones, -ones])
    solution, _ = nnls(design, reference)
    return solution[: score_matrix.shape[1]]


def combined(score_matrix, weights):
    total = weights.sum()
    if total <= 0:
        return numpy.zeros(score_matrix.shape[0])
    return score_matrix @ weights / total


def correlation(estimate, reference):
    if numpy.all(estimate == estimate[0]):
        return 0.0
    return float(spearmanr(estimate, reference).statistic)


def ordered_pair_share(estimate, reference):
    reference_order = numpy.sign(reference[:, None] - reference[None, :])
    estimate_order = numpy.sign(estimate[:, None] - estimate[None, :])
    comparable = numpy.triu(reference_order != 0, 1)
    agreement = (estimate_order == reference_order) + 0.5 * (estimate_order == 0)
    return float(agreement[comparable].sum() / comparable.sum())


def cross_validated_correlation(score_matrix, reference, folds):
    estimate = numpy.zeros(len(reference))
    for fold in range(FOLD_COUNT):
        test = folds == fold
        weights = fit_weights(score_matrix[~test], reference[~test])
        estimate[test] = combined(score_matrix[test], weights)
    return correlation(estimate, reference)


def select_inputs(tuning, candidates, anchors, folds):
    reference = tuning["reference"]
    chosen = []
    best = 0.0
    while True:
        gains = []
        for name in candidates:
            if name in chosen:
                continue
            trial = chosen + [name]
            value = cross_validated_correlation(scores(tuning, trial, anchors), reference, folds)
            gains.append((value, name))
        if not gains:
            break
        value, name = max(gains)
        if value - best < MINIMUM_GAIN:
            break
        chosen.append(name)
        best = value
        print(f"  add {name:24s} cross validated correlation {value:.3f}")
    return chosen, best


def report(label, dump, inputs, anchors, weights):
    usable = ~numpy.isnan(dump["reference"])
    estimate = combined(scores(dump, inputs, anchors), weights)[usable]
    reference = dump["reference"][usable]
    print(
        f"{label}: correlation {correlation(estimate, reference):.3f}, "
        f"pairs in the right order {ordered_pair_share(estimate, reference) * 100:.1f}%, "
        f"{usable.sum()} tracks"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--tuning", required=True)
    parser.add_argument("--held-out", required=True)
    parser.add_argument("--spread", required=True)
    parser.add_argument("--check", action="append", default=[], help="label=dump, scored only")
    parser.add_argument("--features", help="comma separated candidates, default every feature")
    parser.add_argument("--force", help="comma separated features to use without selection")
    arguments = parser.parse_args()

    tuning = read_dump(arguments.tuning)
    held_out = read_dump(arguments.held_out)
    spread = read_dump(arguments.spread)
    anchor_dumps = [tuning, spread]
    skipped = {"reference", "rating", "score"}
    candidates = (
        arguments.features.split(",")
        if arguments.features
        else [name for name in tuning if name not in skipped]
    )
    anchors = choose_anchors(anchor_dumps, candidates)

    random = numpy.random.default_rng(20261111)
    folds = random.permutation(len(tuning["reference"])) % FOLD_COUNT

    print("each feature alone on the tuning file:")
    for name in candidates:
        alone = scores(tuning, [name], anchors)[:, 0]
        print(f"  {name:24s} {correlation(alone, tuning['reference']):+.3f}")

    if arguments.force:
        inputs = arguments.force.split(",")
        value = cross_validated_correlation(scores(tuning, inputs, anchors), tuning["reference"], folds)
        print(f"forced features, cross validated correlation {value:.3f}")
    else:
        print("selection:")
        inputs, _ = select_inputs(tuning, candidates, anchors, folds)

    weights = fit_weights(scores(tuning, inputs, anchors), tuning["reference"])
    inputs = [name for name, weight in zip(inputs, weights) if weight > 0]
    weights = weights[weights > 0]
    weights = weights / weights.sum()

    spread_average = combined(scores(spread, inputs, anchors), weights)
    combined_low, combined_high = numpy.percentile(spread_average, SPREAD_PERCENTILES)
    model = {
        "anchors": {
            name: {"low": float(anchors[name][0]), "high": float(anchors[name][1]), "weight": float(weight)}
            for name, weight in zip(inputs, weights)
        },
        "combined_low": float(combined_low),
        "combined_high": float(combined_high),
    }
    print(json.dumps(model, indent=2))

    report("tuning", tuning, inputs, anchors, weights)
    report("held out", held_out, inputs, anchors, weights)
    for check in arguments.check:
        label, path = check.split("=", 1)
        report(label, read_dump(path), inputs, anchors, weights)


if __name__ == "__main__":
    main()
