"""Verify immutable raw evidence and recover audited native KiB units explicitly."""

from __future__ import annotations

import gzip
import hashlib
import json
import math
import statistics
from pathlib import Path
from typing import Any


def load_bundle(root: Path, name: str) -> dict[str, str]:
    index = json.loads((root / "evidence/index.json").read_bytes())[name]
    packed = (root / "evidence" / index["path"]).read_bytes()
    if hashlib.sha256(packed).hexdigest() != index["sha256"]:
        raise ValueError("compressed evidence integrity")
    decoded = gzip.decompress(packed)
    if hashlib.sha256(decoded).hexdigest() != index["decoded_sha256"]:
        raise ValueError("decoded evidence integrity")
    bundle = json.loads(decoded)
    result = {}
    for path, record in bundle["files"].items():
        raw = record["raw_utf8"]
        if hashlib.sha256(raw.encode()).hexdigest() != record["sha256"]:
            raise ValueError(f"raw-file integrity: {path}")
        result[path] = raw
    return result


def native_peak_bytes(record: dict[str, Any]) -> int:
    """Frozen c5f8c4e mislabeled KiB as bytes; preserve raw and convert once."""
    return int(record["peak_rss_bytes"]) * 1024


def percentile95(values: list[float]) -> float:
    ordered = sorted(values)
    position = 0.95 * (len(ordered) - 1)
    low = math.floor(position)
    return ordered[low] + (ordered[min(low + 1, len(ordered) - 1)] - ordered[low]) * (
        position - low
    )


def summarize(root: Path) -> dict[str, Any]:
    raw = load_bundle(root, "linux")
    qualification = json.loads(raw["qualification/report.json"])
    timing = json.loads(raw["timing/report.json"])
    if (
        not qualification["complete"]
        or not qualification["passed"]
        or len(qualification["cases"]) != 376
    ):
        raise ValueError("composed qualification")
    if not timing["complete"] or not timing["passed"] or len(timing["cases"]) != 20:
        raise ValueError("timing completion")
    if timing["identity"] != qualification["identity"]:
        raise ValueError("qualification/timing identity")
    rows = []
    for case in timing["cases"]:
        row: dict[str, Any] = {"id": case["id"], "index": case["index"], "modes": {}}
        for mode in ("python", "full", "chunked"):
            path = f"timing/cases/{case['index']:04d}-{mode}.json"
            data = json.loads(raw[path])
            if (
                hashlib.sha256(raw[path].encode()).hexdigest()
                != case["results"][mode]["report_sha256"]
                or not data["passed"]
            ):
                raise ValueError("timing record identity/status")
            track = data if mode == "python" else data["cases"][0]
            if track["id"] != case["id"] or len(track["seconds"]) != 20:
                raise ValueError("fixed track/repetitions")
            peak = (
                int(data["peak_rss_bytes"])
                if mode == "python"
                else native_peak_bytes(data)
            )
            if peak < track["loaded_rss_bytes"]:
                raise ValueError("peak/loaded RSS unit contradiction")
            row["modes"][mode] = {
                "seconds": track["seconds"],
                "median_seconds": statistics.median(track["seconds"]),
                "loaded_rss_bytes": track["loaded_rss_bytes"],
                "peak_rss_bytes": peak,
                "raw_peak_field": data["peak_rss_bytes"],
                "raw_peak_unit": "bytes" if mode == "python" else "KiB",
                "report_sha256": hashlib.sha256(raw[path].encode()).hexdigest(),
            }
        rows.append(row)
    aggregate = {}
    for mode in ("python", "full", "chunked"):
        values = [row["modes"][mode] for row in rows]
        aggregate[mode] = {
            "median_seconds": statistics.median(v["median_seconds"] for v in values),
            "p95_seconds": percentile95([v["median_seconds"] for v in values]),
            "median_peak_rss_bytes": statistics.median(
                v["peak_rss_bytes"] for v in values
            ),
            "maximum_peak_rss_bytes": max(v["peak_rss_bytes"] for v in values),
        }
    paired = {}
    for numerator, denominator in (
        ("full", "python"),
        ("chunked", "python"),
        ("chunked", "full"),
    ):
        paired[f"{numerator}_over_{denominator}"] = {
            field: statistics.median(
                row["modes"][numerator][field] / row["modes"][denominator][field]
                for row in rows
            )
            for field in ("median_seconds", "peak_rss_bytes")
        }
    mac = load_bundle(root, "mac")
    portability = json.loads(mac["execution/comparisons.json"])
    return {
        "format": "cqt-composed-summary-v1",
        "native_peak_unit_erratum": "Raw native peak_rss_bytes is KiB; summary multiplies by1024. Raw Python/loaded RSS/supervisor bytes unchanged.",
        "qualification": {
            "tracks": 376,
            "passed": True,
            "child_cpu_seconds": qualification["child_cpu_seconds"],
            "wall_seconds": qualification["wall_seconds"],
        },
        "timing": {
            "tracks": 20,
            "domain": "FMAK only",
            "child_cpu_seconds": timing["child_cpu_seconds"],
            "wall_seconds": timing["wall_seconds"],
        },
        "aggregate": aggregate,
        "paired": paired,
        "rows": rows,
        "mac": portability,
    }


if __name__ == "__main__":
    directory = Path(__file__).resolve().parent
    (directory / "evidence/summary.json").write_text(
        json.dumps(summarize(directory), indent=2, allow_nan=False) + "\n"
    )
