# /// script
# requires-python = ">=3.12"
# dependencies = ["matplotlib==3.11.2"]
# ///
"""Regenerate inline figures exclusively from integrity-checked measured evidence."""

from __future__ import annotations

from pathlib import Path

import matplotlib.pyplot as plt
from summarize import summarize


def plot(root: Path) -> None:
    data = summarize(root)
    ordered = sorted(
        data["rows"], key=lambda row: row["modes"]["python"]["median_seconds"]
    )
    colors = {"python": "#bc6d35", "full": "#3e6e9d", "chunked": "#23856d"}
    names = {
        "python": "Python full",
        "full": "Native full",
        "chunked": "Native chunked",
    }
    fig, axes = plt.subplots(1, 2, figsize=(11.4, 4.5), layout="constrained")
    for mode, color in colors.items():
        axes[0].plot(
            range(1, 21),
            [row["modes"][mode]["median_seconds"] * 1000 for row in ordered],
            marker=".",
            color=color,
            label=names[mode],
        )
        axes[1].plot(
            range(1, 21),
            [row["modes"][mode]["peak_rss_bytes"] / 1024**2 for row in ordered],
            marker=".",
            color=color,
            label=names[mode],
        )
    axes[0].set_ylabel("CQT + inference median (ms)")
    axes[1].set_ylabel("Fresh worker peak RSS (MiB)")
    for axis in axes:
        axis.set_xlabel("20 FMAK tracks, ordered by Python latency")
        axis.grid(alpha=0.2)
        axis.spines[["top", "right"]].set_visible(False)
    axes[0].legend(frameon=False)
    fig.suptitle(
        "Identical PCM → features → logits: one Linux logical CPU\n376 composition checks passed; 20 fresh workers per mode, 20 timed reps each",
        fontsize=12,
    )
    fig.savefig(root / "linux-composed.png", dpi=180)
    fig.savefig(root / "linux-composed.svg")
    plt.close(fig)
    mac = data["mac"]["cases"]
    names = {
        "native_vs_mac": "Native / Mac reference",
        "mac_vs_linux": "Mac / Linux reference",
        "native_vs_linux": "Native / Linux reference",
        "canonical_core_mac_vs_linux": "Mac ORT / Linux ORT, same core",
    }
    fig, axes = plt.subplots(1, 2, figsize=(11.4, 4.7), layout="constrained")
    for name, label in names.items():
        ratios = [
            max(
                abs(a - b) / (1e-5 + 1e-5 * abs(b))
                for a, b in zip(
                    row["gates"][name]["logits"],
                    row["gates"][name]["reference"],
                    strict=True,
                )
            )
            for row in mac
        ]
        axes[0].plot(range(9), ratios, marker=".", label=label)
    axes[0].axhline(1, color="#a34439", linestyle="--", linewidth=1, label="Fixed gate")
    axes[0].set_ylabel("Largest normalized logit error (gate ≤ 1)")
    axes[0].set_ylim(0, 1.05)
    axes[0].legend(frameon=False, fontsize=8)
    axes[1].bar(
        range(9),
        [
            100
            * row["features"]["mac"]["mismatches"]
            / row["features"]["mac"]["values"]
            for row in mac
        ],
        color="#23856d",
    )
    axes[1].set_ylabel("Native / Mac reference core mismatches (%)")
    for axis in axes:
        axis.set_xticks(
            range(9),
            [row["id"].replace("fmak-", "") for row in mac],
            rotation=55,
            ha="right",
        )
        axis.grid(axis="y", alpha=0.2)
        axis.spines[["top", "right"]].set_visible(False)
    fig.suptitle(
        "Apple M5 / arm64, identical PCM: all four nine-case logit/key gates passed\nFeature bytes differ; no Mac376 or production claim",
        fontsize=12,
    )
    fig.savefig(root / "mac-nine.png", dpi=180)
    fig.savefig(root / "mac-nine.svg")
    plt.close(fig)


if __name__ == "__main__":
    plot(Path(__file__).resolve().parent)
