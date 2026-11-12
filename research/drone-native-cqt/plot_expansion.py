# /// script
# requires-python = ">=3.12"
# dependencies = ["matplotlib==3.11.2"]
# ///
"""Plot every frozen public case and both arithmetic outcomes from verified JSON."""

from __future__ import annotations

from pathlib import Path

import matplotlib.pyplot as plt
from evidence_io import load_json


def plot(root: Path) -> None:
    original = load_json(root / "evidence/original-376-summary.json.gz")
    ablation = load_json(root / "evidence/twoarm-376-summary.json.gz")
    series = [
        ("Original scalar math", "#bd672f", original, "original"),
        ("FMA + platform log1p", "#3566a1", ablation, "fma"),
        ("FMA + upstream SVML", "#19846d", ablation, "fma-svml"),
    ]
    figure, axes = plt.subplots(
        1,
        2,
        figsize=(11.2, 4.8),
        layout="constrained",
        gridspec_kw={"width_ratios": [1.9, 1]},
    )
    for index, (label, color, evidence, arm) in enumerate(series):
        durations = [case["frames"] * 4096 / 22050 / 60 for case in evidence["cases"]]
        ratios = [
            case["arms"][arm]["comparison"]["max_normalized_error"]
            for case in evidence["cases"]
        ]
        axes[0].scatter(
            durations,
            [max(ratio, 1e-5) for ratio in ratios],
            s=14,
            alpha=0.65,
            color=color,
            label=label,
            marker=["s", "^", "o"][index],
        )
        passed = 376 - evidence["aggregate"][arm]["strict_failures"]
        axes[1].bar(index, passed, color=color, width=0.6)
        axes[1].bar(index, 376 - passed, bottom=passed, color="#a94545", width=0.6)
        axes[1].text(
            index,
            382,
            f"{passed}/376\n{376 - passed} strict failure{'s' if 376 - passed != 1 else ''}",
            ha="center",
            va="bottom",
            fontsize=9,
        )
    axes[0].axhline(1, color="#555555", linestyle="--", linewidth=1)
    axes[0].set_xscale("log")
    axes[0].set_yscale("log")
    axes[0].set_ylim(5e-6, 2.0)
    axes[0].set_xlabel("Track duration (minutes; logarithmic)")
    axes[0].set_ylabel(
        "Maximum normalized logit error\n|Δ| / (10⁻⁵ + 10⁻⁵ |reference|)"
    )
    axes[0].set_title("Unchanged gate: every logit ≤ 1")
    axes[0].legend(loc="center right", frameon=False, fontsize=8)
    axes[0].text(
        0.02, 0.02, "Zero errors shown at 10⁻⁵", transform=axes[0].transAxes, fontsize=8
    )
    axes[1].set_xticks(range(3), ["Original", "FMA +\nplatform log1p", "FMA +\nSVML"])
    axes[1].set_ylim(0, 425)
    axes[1].set_ylabel("Tracks passing the strict logit gate")
    axes[1].set_title("All three arms: 0 key changes")
    for axis in axes:
        axis.spines[["top", "right"]].set_visible(False)
        axis.grid(axis="y", color="#e0e0e0", linewidth=0.5)
        axis.set_axisbelow(True)
    figure.suptitle(
        "Native CQT: the exact upstream arithmetic closes the 376-track gate",
        fontsize=13,
    )
    figure.savefig(root / "cqt-376-ablation.png", dpi=180)
    figure.savefig(root / "cqt-376-ablation.svg")
    plt.close(figure)


if __name__ == "__main__":
    plot(Path(__file__).resolve().parent)
