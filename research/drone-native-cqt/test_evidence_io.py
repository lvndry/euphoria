# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Evidence packaging must preserve the exact previously recorded JSON bytes."""

from __future__ import annotations

import gzip
import json
import tempfile
import unittest
from pathlib import Path

from evidence_io import (
    LARGE_SUMMARIES,
    canonical_gzip,
    digest,
    load_json,
    package_summaries,
)


class EvidenceStorageTests(unittest.TestCase):
    def test_plain_json_must_match_its_indexed_identity(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / "reference-summary.json"
            raw = b'{"cases": [{"id": "original"}]}\n'
            path.write_bytes(raw)
            (root / "index.json").write_text(json.dumps({"files": {path.name: digest(raw)}}))
            self.assertEqual(load_json(path), json.loads(raw))
            path.write_bytes(b'{"cases": [{"id": "changed"}]}\n')
            with self.assertRaises(ValueError):
                load_json(path)

    def test_unindexed_plain_json_is_not_accepted_as_verified_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / "unindexed.json"
            path.write_text("{}")
            (root / "index.json").write_text(json.dumps({"files": {}}))
            with self.assertRaises(ValueError):
                load_json(path)

    def test_roundtrip_is_deterministic_and_keeps_decoded_hashes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            original = b'{"spacing": [1,  2], "text": "unchanged"}\n'
            identities = {}
            for name in LARGE_SUMMARIES:
                (root / name).write_bytes(original)
                identities[name] = digest(original)
            (root / "index.json").write_text(json.dumps({"files": identities}))
            package_summaries(root)
            before = {}
            index = json.loads((root / "index.json").read_bytes())
            for name in LARGE_SUMMARIES:
                path = root / f"{name}.gz"
                before[name] = path.read_bytes()
                self.assertEqual(gzip.decompress(before[name]), original)
                self.assertEqual(
                    index["compressed_json"][path.name]["decoded_sha256"],
                    identities[name],
                )
                self.assertEqual(before[name][3], 0)  # No optional filename/header.
                self.assertEqual(before[name][4:8], b"\0\0\0\0")
                self.assertEqual(load_json(path), json.loads(original))
                self.assertFalse((root / name).exists())
            package_summaries(root)
            for name in LARGE_SUMMARIES:
                self.assertEqual((root / f"{name}.gz").read_bytes(), before[name])

    def test_tampering_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            raw = b'{"valid": true}\n'
            encoded = canonical_gzip(raw)
            path = root / "sample.json.gz"
            path.write_bytes(encoded)
            record = {
                "compressed_sha256": digest(encoded),
                "decoded_sha256": "0" * 64,
                "decoded_bytes": len(raw),
            }
            (root / "index.json").write_text(
                json.dumps({"compressed_json": {path.name: record}})
            )
            with self.assertRaises(ValueError):
                load_json(path)
            record["decoded_sha256"] = digest(raw)
            path.write_bytes(encoded + b"changed")
            (root / "index.json").write_text(
                json.dumps({"compressed_json": {path.name: record}})
            )
            with self.assertRaises(ValueError):
                load_json(path)


if __name__ == "__main__":
    unittest.main()
