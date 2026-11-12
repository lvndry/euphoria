# /// script
# requires-python = ">=3.12"
# dependencies = ["matplotlib==3.11.2"]
# ///
"""Reproduce the inline identical-PCM feasibility figure from immutable JSON."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from evidence_io import load_json


def validate_case_order(reference: list[dict[str, Any]], cases: list[dict[str, Any]]) -> None:
    identities = []
    for rows in (reference, cases):
        ids = [row.get("id") for row in rows]
        if any(not isinstance(identity, str) or not identity for identity in ids):
            raise ValueError("Missing or invalid evidence case identity")
        if len(set(ids)) != len(ids):
            raise ValueError("Duplicate evidence case identity")
        identities.append(ids)
    if identities[0] != identities[1]:
        raise ValueError("Report case identities/order differ from the reference")


def plot(root: Path) -> None:
    reference = load_json(root / "evidence/reference-summary.json")["cases"]
    reports = [
        ("Experimental WIP", "#b75d28", load_json(root / "evidence/owner-d8424bf.json")),
        (
            "Exported CSR + exact-source soxr",
            "#245c8c",
            load_json(root / "evidence/exported-d8424bf.json"),
        ),
    ]
    for _, _, report in reports:
        validate_case_order(reference, report["cases"])
    # Validate evidence before importing the optional rendering dependency or
    # creating a figure, so invalid inputs cannot produce a misleading chart.
    import matplotlib.pyplot as plt

    figure, axes = plt.subplots(
        1, 2, figsize=(11.4, 5), layout="constrained", sharey=True
    )
    names = [row["id"].replace("fmak-", "FMAK ") for row in reference]
    for index, (label, color, report) in enumerate(reports):
        ratios = [
            max(
                abs(a - b) / (1e-5 + 1e-5 * abs(b))
                for a, b in zip(
                    row["logits"], expected["reference_logits"], strict=True
                )
            )
            for row, expected in zip(report["cases"], reference, strict=True)
        ]
        rates = [row["features"]["fp16_mismatch_rate"] * 100 for row in report["cases"]]
        positions = [i + (index - 0.5) * 0.19 for i in range(len(reference))]
        axes[0].scatter(
            ratios,
            positions,
            label=label,
            color=color,
            s=35,
            marker="o" if index else "s",
            zorder=3,
        )
        axes[1].scatter(
            [max(rate, 1e-5) for rate in rates],
            positions,
            label=label,
            color=color,
            s=35,
            marker="o" if index else "s",
            zorder=3,
        )
    axes[0].axvline(1, color="#555555", linestyle="--", linewidth=1)
    axes[0].set_xlabel(
        "Maximum normalized logit error\nmax |Δ| / (10⁻⁵ + 10⁻⁵ |reference|)"
    )
    axes[0].set_title("Unchanged strict gate: pass ≤ 1")
    axes[0].set_yticks(range(len(names)), names)
    axes[0].invert_yaxis()
    axes[1].set_xscale("log")
    axes[1].set_xlabel(
        "FP16 feature mismatch rate (%)\nzeros placed at 10⁻⁵% for display"
    )
    axes[1].set_title("Feature differences are diagnostics")
    for axis in axes:
        axis.grid(axis="x", color="#dddddd", linewidth=0.6)
        axis.spines[["top", "right"]].set_visible(False)
    figure.suptitle(
        "Native CQT on identical reference PCM: 9 fixed cases, 0 key changes",
        fontsize=13,
    )
    handles, labels = axes[0].get_legend_handles_labels()
    figure.legend(handles, labels, loc="outside lower center", ncols=2, frameon=False)
    figure.savefig(root / "cqt-feasibility.png", dpi=180)
    figure.savefig(root / "cqt-feasibility.svg")
    plt.close(figure)


if __name__ == "__main__":
    plot(Path(__file__).resolve().parent)
