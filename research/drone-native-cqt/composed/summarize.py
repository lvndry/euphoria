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
    if len(bundle["files"]) != index["files"]:
        raise ValueError("raw evidence file count")
    result = {}
    for path, record in bundle["files"].items():
        raw = record["raw_utf8"]
        if hashlib.sha256(raw.encode()).hexdigest() != record["sha256"]:
            raise ValueError(f"raw-file integrity: {path}")
        result[path] = raw
    return result


def native_peak_bytes(record: dict[str, Any]) -> int:
    """Frozen c5f8c4e mislabeled KiB as bytes; preserve raw and convert once."""
    if (
        record.get("format") != "k10-composed-pipeline-v1"
        or record.get("identity", {}).get("pipeline_source_sha256")
        != "77bc7059ce96f81cac38751590f9fca7fba7a4728460d5d6215dcd841e1af5ec"
    ):
        raise ValueError("KiB recovery applies only to the frozen legacy report source")
    return int(record["peak_rss_bytes"]) * 1024


def percentile95(values: list[float]) -> float:
    ordered = sorted(values)
    position = 0.95 * (len(ordered) - 1)
    low = math.floor(position)
    return ordered[low] + (ordered[min(low + 1, len(ordered) - 1)] - ordered[low]) * (
        position - low
    )


def verify_logits(actual: list[float], reference: list[float]) -> None:
    if len(actual) != 24 or len(reference) != 24:
        raise ValueError("24-class logit shape")
    if not all(math.isfinite(v) for v in actual + reference):
        raise ValueError("nonfinite recorded logits")
    if any(
        abs(a - b) > 1e-5 + 1e-5 * abs(b)
        for a, b in zip(actual, reference, strict=True)
    ):
        raise ValueError("recorded logit tolerance")
    if max(range(24), key=actual.__getitem__) != max(
        range(24), key=reference.__getitem__
    ):
        raise ValueError("recorded key disagreement")


def verify_native(
    record: dict[str, Any],
    expected: dict[str, Any],
    backends: set[str],
    repetitions: int,
) -> None:
    if not record["passed"] or len(record["cases"]) != 1:
        raise ValueError("native record status/count")
    case = record["cases"][0]
    if (
        case["id"] != expected["id"]
        or case["pcm_sha256"] != expected["pcm"]["sha256"]
        or case["frames"] != expected["frames"]
    ):
        raise ValueError("native case identity")
    if len(case["iterations"]) != repetitions:
        raise ValueError("native iteration count")
    for iteration in case["iterations"]:
        if (
            not iteration["passed"]
            or not iteration["core_hash_matches"]
            or iteration["core_sha256"] != expected["core"]["sha256"]
        ):
            raise ValueError("recorded native core identity")
        if {p["backend"] for p in iteration["predictions"]} != backends or len(
            iteration["predictions"]
        ) != len(backends):
            raise ValueError("native backend coverage")
        for prediction in iteration["predictions"]:
            if (
                not prediction["comparison"]["allclose"]
                or not prediction["comparison"]["argmax_agrees"]
            ):
                raise ValueError("recorded comparison status")
            verify_logits(prediction["logits"], expected["reference_logits"])


def summarize(root: Path) -> dict[str, Any]:
    raw = load_bundle(root, "linux")
    qualification_inputs = load_bundle(root, "qualification_inputs")
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
    if len({case["id"] for case in qualification["cases"]}) != 376 or {
        case["index"] for case in qualification["cases"]
    } != set(range(376)):
        raise ValueError("duplicate qualification identity")
    for case in qualification["cases"]:
        path = f"qualification/cases/{case['index']:04d}-both.json"
        inputs = qualification_inputs[
            f"qualification/cases/{case['index']:04d}-input.json"
        ]
        record = json.loads(raw[path])
        if (
            not case["passed"]
            or not case["results"]["both"]["passed"]
            or json.loads(inputs)["cases"][0]["id"] != case["id"]
        ):
            raise ValueError("qualification case status/identity")
        if (
            hashlib.sha256(raw[path].encode()).hexdigest()
            != case["results"]["both"]["report_sha256"]
            or hashlib.sha256(inputs.encode()).hexdigest()
            != record["identity"]["manifest_sha256"]
        ):
            raise ValueError("qualification report/input binding")
        verify_native(record, json.loads(inputs)["cases"][0], {"full", "chunked"}, 1)
    if len({case["id"] for case in timing["cases"]}) != 20:
        raise ValueError("duplicate timing identity")
    rows = []
    for case in timing["cases"]:
        inputs = raw[f"timing/cases/{case['index']:04d}-input.json"]
        expected = json.loads(inputs)["cases"][0]
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
            if mode == "python":
                if len(data["logits"]) != 21:
                    raise ValueError("Python warmup/repetition coverage")
                for logits in data["logits"]:
                    verify_logits(logits, expected["reference_logits"])
            else:
                if (
                    hashlib.sha256(inputs.encode()).hexdigest()
                    != data["identity"]["manifest_sha256"]
                ):
                    raise ValueError("timing input binding")
                verify_native(data, expected, {mode}, 21)
                if "frames" in row and row["frames"] != track["frames"]:
                    raise ValueError("timing frame count")
                row["frames"] = track["frames"]
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
    if (
        not portability["complete"]
        or not portability["passed"]
        or portability["tracks"] != 9
        or len(portability["cases"]) != 9
        or len({case["id"] for case in portability["cases"]}) != 9
    ):
        raise ValueError("Mac case coverage/status")
    comparisons = {
        "native_vs_mac",
        "mac_vs_linux",
        "native_vs_linux",
        "canonical_core_mac_vs_linux",
    }
    for case in portability["cases"]:
        if set(case["gates"]) != comparisons:
            raise ValueError("Mac comparison coverage")
        for gate in case["gates"].values():
            if not gate["passed"] or not gate["finite"] or not gate["same_key"]:
                raise ValueError("Mac comparison status")
            verify_logits(gate["logits"], gate["reference"])
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
