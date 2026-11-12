# /// script
# requires-python = ">=3.12"
# dependencies = ["matplotlib==3.11.2"]
# ///
"""Regenerate inline Mac376 figures from verified saved vectors only."""

from __future__ import annotations

from pathlib import Path

import matplotlib.pyplot as plt
from matplotlib.figure import Figure
from summarize_mac376 import FAMILIES, summarize

LABELS = [
    "Native full / Mac reference",
    "Native chunk / Mac reference",
    "Mac frontend / Linux",
    "Native full / Linux",
    "Native chunk / Linux",
    "Same Linux core: Mac / Linux ORT",
]
COLORS = ["#315e8e", "#5d86ac", "#bc6d35", "#975b32", "#d39864", "#23856d"]


def save(fig: Figure, root: Path, name: str) -> None:
    fig.savefig(root / (name + ".png"), dpi=180)
    fig.savefig(root / (name + ".svg"), metadata={"Date": None})
    plt.close(fig)


def plot(root: Path) -> None:
    data = summarize(root)
    plt.rcParams["svg.hashsalt"] = "mac376-disjoint-v4"
    fig, axes = plt.subplots(
        1,
        2,
        figsize=(12, 5.1),
        layout="constrained",
        gridspec_kw={"width_ratios": [1, 1.12]},
    )
    for i, (name, color) in enumerate(zip(FAMILIES, COLORS, strict=True)):
        failed = 376 - data["families"][name]["passed"]
        axes[0].barh(i, failed, color=color)
        axes[0].text(
            failed + 0.45,
            i,
            f"{failed} fail; {376 - failed}/376 pass",
            va="center",
            fontsize=9,
        )
        ratios = sorted(
            max(
                abs(a - r) / (1e-5 + 1e-5 * abs(r))
                for a, r in zip(
                    row["gates"][name]["logits"],
                    row["gates"][name]["reference"],
                    strict=True,
                )
            )
            for row in data["cases"]
        )
        axes[1].plot(range(1, 377), ratios, color=color, label=LABELS[i], linewidth=1.4)
    axes[0].set_yticks(range(6), LABELS)
    axes[0].invert_yaxis()
    axes[0].set_xlim(0, 40)
    axes[0].set_xlabel("Tracks failing the unchanged strict logit gate")
    axes[0].set_title("Strict portability gate failed")
    axes[1].axhline(1, color="#a33a35", linestyle="--", linewidth=1)
    axes[1].text(12, 1.04, "Fixed gate ≤ 1", color="#a33a35", fontsize=9)
    axes[1].set_xlabel("Case rank, sorted separately for each comparison")
    axes[1].set_ylabel("Maximum error / allowed error per track")
    axes[1].set_title("Failures retained; tolerances unchanged")
    axes[1].legend(frameon=False, fontsize=7.5, loc="upper left")
    for ax in axes:
        ax.grid(axis="x", alpha=0.15)
        ax.spines[["top", "right"]].set_visible(False)
    fig.suptitle(
        "376 public tracks, composed from disjoint 305 + 71 phases\nAll six comparisons have zero argmax disagreements; no Mac timing or promotion",
        fontsize=12,
    )
    save(fig, root, "mac376-diagnostic")
    fig, ax = plt.subplots(figsize=(9.2, 4.2), layout="constrained")
    for i, (name, color) in enumerate(zip(FAMILIES, COLORS, strict=True)):
        item = data["families"][name]
        ax.scatter(item["maximum_absolute"], i, color=color, marker="x", s=50)
        ax.scatter(item["minimum_observed_slack"], i, color=color, marker="o", s=40)
        ax.plot(
            [item["maximum_absolute"], item["minimum_observed_slack"]],
            [i, i],
            color=color,
            alpha=0.4,
        )
    ax.set_xscale("log")
    ax.set_yticks(range(6), LABELS)
    ax.invert_yaxis()
    ax.set_xlabel("Logit units (log scale)")
    ax.grid(axis="x", alpha=0.2)
    ax.spines[["top", "right"]].set_visible(False)
    ax.set_title(
        "Observed pairs only: × largest absolute error; ● smallest certificate slack\nEvery pairwise slack positive; not an unseen-input bound or a new acceptance gate",
        fontsize=11,
    )
    save(fig, root, "mac376-observed-margins")


if __name__ == "__main__":
    plot(Path(__file__).resolve().parent)
