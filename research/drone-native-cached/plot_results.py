# /// script
# requires-python = ">=3.12"
# dependencies = ["matplotlib==3.11.2"]
# ///
"""Reproduce the native cached-feature figure from immutable JSON reports.

Run: uv run --script research/drone-native-cached/plot_results.py
This plotting dependency is separate from Rust and production dependencies.
"""

from __future__ import annotations

import hashlib
import json
import statistics
from pathlib import Path
from typing import Any

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parent
EXPECTED = {
    "native-full-58c290b.json": "68fb6a9f6311f817c6bd8b3e0431477b9f42c1d99d4a5cf430d70faad3a39f2f",
    "native-chunked-58c290b.json": "5a49330cbd0d8777541e9ce54fcdec0ef62aa7a3e3204d2b2df095f0e74ee0be",
}
FULL = "#344e5b"
CHUNKED = "#c5623c"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_reports() -> tuple[dict[str, Any], dict[str, Any]]:
    reports = []
    for name, expected in EXPECTED.items():
        path = ROOT / "evidence" / name
        if sha256(path) != expected:
            raise ValueError(f"Changed measured evidence: {name}")
        reports.append(json.loads(path.read_text()))
    full, chunked = reports
    if full["identity"] != chunked["identity"] or not all(r["passed"] for r in reports):
        raise ValueError("Reports are not the same passing experiment")
    if any(r["tracks"] != 20 or r["repetitions"] != 20 for r in reports):
        raise ValueError("Unexpected track/repetition counts")
    if any({row["process_threads"] for row in r["records"]} != {2} for r in reports):
        raise ValueError("Figure's observed thread note no longer matches evidence")
    return full, chunked


def make_figure() -> None:
    full, chunked = load_reports()
    baseline = {row["audio_sha256"]: row for row in full["records"]}
    selected = sorted(chunked["records"], key=lambda row: row["frames"])
    full_ms = [baseline[row["audio_sha256"]]["median_seconds"] * 1000 for row in selected]
    chunked_ms = [row["median_seconds"] * 1000 for row in selected]
    ratios = [c / f for c, f in zip(chunked_ms, full_ms, strict=True)]
    paired_change = (statistics.median(ratios) - 1) * 100
    rss_mib = [r["peak_process_rss_kib"] / 1024 for r in (full, chunked)]
    matplotlib.rcParams.update({
        "font.family": "DejaVu Sans", "font.size": 10,
        "axes.spines.top": False, "axes.spines.right": False,
        "axes.edgecolor": "#a2adb1", "axes.labelcolor": "#243843",
        "text.color": "#243843", "xtick.color": "#52646e", "ytick.color": "#52646e",
        "svg.fonttype": "none", "svg.hashsalt": "k10-native-58c290b",
    })
    figure, (memory, latency) = plt.subplots(
        1, 2, figsize=(11.8, 5.5), gridspec_kw={"width_ratios": [0.9, 1.8]}
    )
    figure.subplots_adjust(left=0.075, right=0.98, top=0.76, bottom=0.24, wspace=0.35)
    figure.suptitle("Native K10: 8.76× lower peak memory, +1.84% paired latency", x=0.075,
                   y=0.965, ha="left", fontsize=17, fontweight="bold")
    figure.text(0.075, 0.875, "Frozen FP32 weights · exact overlap/trim · ORT 1.31.1 · AMD 9955WX",
                fontsize=11, color="#52646e")

    memory.set_title("A  Fresh-process peak RSS", loc="left", pad=18, fontweight="bold")
    bars = memory.bar(["Full", "Chunked"], rss_mib, color=[FULL, CHUNKED], width=0.58)
    memory.set_ylabel("MiB")
    memory.set_ylim(0, 960)
    memory.set_yticks([0, 200, 400, 600, 800])
    memory.grid(axis="y", color="#e5e9eb", zorder=0)
    memory.set_axisbelow(True)
    for bar, value in zip(bars, rss_mib, strict=True):
        memory.text(bar.get_x() + bar.get_width() / 2, value + 24, f"{value:.2f}",
                    ha="center", fontsize=11, fontweight="bold")

    latency.set_title("B  Per-track median inference latency", loc="left", pad=18, fontweight="bold")
    positions = list(range(1, 21))
    latency.plot(positions, full_ms, "o-", color=FULL, label="Full", markersize=4.5, linewidth=1.3)
    latency.plot(positions, chunked_ms, "s--", color=CHUNKED, label="Chunked", markersize=4,
                 linewidth=1.3)
    latency.set_yscale("log")
    latency.set_ylim(4, 400)
    latency.set_yticks([5, 10, 20, 50, 100, 200, 400], ["5", "10", "20", "50", "100", "200", "400"])
    latency.set_ylabel("Milliseconds (log scale)")
    latency.set_xlabel("20 public tracks, ordered by frame count")
    latency.set_xticks([1, 5, 10, 15, 20])
    latency.grid(axis="y", color="#e5e9eb", which="major")
    latency.legend(frameon=False, loc="upper left", ncol=2)
    latency.text(0.02, 0.73, f"Median paired change +{paired_change:.2f}%\n"
                 f"Individual ratios {min(ratios):.3f}–{max(ratios):.3f}; none >+10%",
                 transform=latency.transAxes, fontsize=9, va="top", color="#52646e")

    figure.text(0.075, 0.105,
                "1 logical CPU; TWO observed OS threads · 20 tracks × 20 timed repetitions + 1 warmup each",
                fontsize=10)
    figure.text(0.075, 0.060,
                "Cached features only; no decode/CQT · 376-track validation: 0 key changes, max logit error 1.91e−6",
                fontsize=10, color="#52646e")
    destination = ROOT / "figures"
    destination.mkdir(exist_ok=True)
    svg = destination / "native-cached-results-58c290b.svg"
    png = destination / "native-cached-results-58c290b.png"
    figure.savefig(svg, metadata={"Date": None, "Creator": "plot_results.py / matplotlib 3.11.2"})
    # Matplotlib puts trailing spaces in path commands; normalize generated text
    # without changing path semantics, keeping the artifact whitespace-clean.
    svg.write_text("\n".join(line.rstrip() for line in svg.read_text().splitlines()) + "\n")
    figure.savefig(png, dpi=180, metadata={"Software": "plot_results.py / matplotlib 3.11.2"})
    plt.close(figure)
    provenance = {
        "measured_source_commit": "58c290b88959c9901a5c59fd27037b247364190d",
        "binary_sha256": full["identity"]["executable_sha256"],
        "plot_script_sha256": sha256(Path(__file__)), "matplotlib": matplotlib.__version__,
        "raw_report_sha256": EXPECTED,
        "figure_sha256": {path.name: sha256(path) for path in (svg, png)},
    }
    (destination / "native-cached-results-58c290b.json").write_text(
        json.dumps(provenance, indent=2) + "\n"
    )
    print(png)
    print(svg)


if __name__ == "__main__":
    make_figure()
