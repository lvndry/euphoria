# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Lossless, deterministic storage and verified loading of large study JSON."""

from __future__ import annotations

import gzip
import hashlib
import io
import json
import zlib
from pathlib import Path
from typing import Any

LARGE_SUMMARIES = ("original-376-summary.json", "twoarm-376-summary.json")


def digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def canonical_gzip(raw: bytes) -> bytes:
    stream = io.BytesIO()
    with gzip.GzipFile(
        filename="", mode="wb", compresslevel=9, fileobj=stream, mtime=0
    ) as output:
        output.write(raw)
    return stream.getvalue()


def load_json(path: Path) -> dict[str, Any]:
    encoded = path.read_bytes()
    if path.suffix == ".gz":
        index = json.loads((path.parent / "index.json").read_bytes())
        identity = index["compressed_json"][path.name]
        if digest(encoded) != identity["compressed_sha256"]:
            raise ValueError(f"Changed compressed evidence: {path.name}")
        raw = gzip.decompress(encoded)
        if (
            digest(raw) != identity["decoded_sha256"]
            or len(raw) != identity["decoded_bytes"]
        ):
            raise ValueError(f"Changed decoded evidence: {path.name}")
    else:
        raw = encoded
    return json.loads(raw)


def package_summaries(directory: Path) -> None:
    index_path = directory / "index.json"
    index = json.loads(index_path.read_bytes())
    compressed = index.setdefault("compressed_json", {})
    for name in LARGE_SUMMARIES:
        original = directory / name
        target = original.with_suffix(".json.gz")
        if original.exists():
            raw = original.read_bytes()
            expected = index["files"].get(
                name, compressed.get(target.name, {}).get("decoded_sha256")
            )
            if digest(raw) != expected:
                raise ValueError(f"Original decoded identity changed: {name}")
            encoded = canonical_gzip(raw)
            if target.exists():
                if target.read_bytes() != encoded:
                    raise ValueError(
                        f"Unexpected existing compressed bytes: {target.name}"
                    )
            else:
                with target.open("xb") as output:
                    output.write(encoded)
            if gzip.decompress(encoded) != raw:
                raise ValueError("Lossless compression verification failed")
            compressed[target.name] = {
                "decoded_name": name,
                "decoded_sha256": digest(raw),
                "decoded_bytes": len(raw),
                "compressed_sha256": digest(encoded),
                "compressed_bytes": len(encoded),
                "gzip_mtime": 0,
                "gzip_filename": "",
                "gzip_level": 9,
                "zlib_version": zlib.ZLIB_RUNTIME_VERSION,
            }
        elif target.exists():
            # Repeated packaging verifies both stored and decoded identities.
            load_json(target)
        else:
            raise FileNotFoundError(original)
        index["files"].pop(name, None)
        index["files"][target.name] = compressed[target.name]["compressed_sha256"]
    pending = index_path.with_suffix(".pending")
    pending.write_text(json.dumps(index, indent=2) + "\n")
    pending.replace(index_path)
    for name in LARGE_SUMMARIES:
        (directory / name).unlink(missing_ok=True)


if __name__ == "__main__":
    package_summaries(Path(__file__).resolve().parent / "evidence")
