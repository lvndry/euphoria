"""Saved-artifact regression only; no ML runtime or audio is needed."""

from __future__ import annotations

import hashlib
import json
import unittest
from pathlib import Path

from summarize_mac376 import certificate, comparison, summarize, unpack, verify

ROOT = Path(__file__).resolve().parent


class Mac376Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.files = unpack(ROOT)

    def test_original_phases_exact_union_and_failed_gates_preserved(self) -> None:
        result = summarize(ROOT)
        self.assertEqual(result["disjoint_phases"], [305, 71])
        self.assertEqual([r["index"] for r in result["cases"]], list(range(376)))
        self.assertEqual(
            [x["passed"] for x in result["families"].values()],
            [375, 375, 348, 350, 350, 376],
        )
        self.assertTrue(
            all(x["key_disagreements"] == 0 for x in result["families"].values())
        )
        self.assertFalse(result["strict_passed"])
        self.assertFalse(result["timing_authorized"])
        self.assertFalse(result["promotion_authorized"])

    def test_original_combined_source_cannot_be_rewritten(self) -> None:
        files = dict(self.files)
        source = json.loads(files["v4/combined-diagnostic.json"])
        source["cases"].pop()
        files["v4/combined-diagnostic.json"] = json.dumps(source)
        with self.assertRaisesRegex(ValueError, "source identity"):
            verify(files)

    def test_independent_certificate_is_recomputed_not_merely_trusted(self) -> None:
        files = dict(self.files)
        root = json.loads(files["root/observed-margin-certificates.json"])
        root["families"]["full_vs_mac"]["cases"][0][
            "minimum_pairwise_certificate_slack"
        ] += 1
        files["root/observed-margin-certificates.json"] = json.dumps(root)
        receipt = json.loads(files["independent/observed-margin-verification.json"])
        receipt["root_certificate_sha256"] = hashlib.sha256(
            files["root/observed-margin-certificates.json"].encode()
        ).hexdigest()
        files["independent/observed-margin-verification.json"] = json.dumps(receipt)
        with self.assertRaisesRegex(ValueError, "certificate arithmetic"):
            verify(files)

    def test_raw_native_vector_corruption_is_rejected(self) -> None:
        files = dict(self.files)
        name = "v3/qualification/cases/0000.native.json"
        native = json.loads(files[name])
        native["cases"][0]["iterations"][0]["predictions"][0]["logits"][0] += 0.1
        files[name] = json.dumps(native)
        with self.assertRaisesRegex(ValueError, "native report hash"):
            verify(files)

    def test_argmax_agreement_does_not_imply_strict_gate_or_certificate(self) -> None:
        reference = [1.0, 0.99] + [-2.0] * 22
        actual = [1.1, 1.09] + [-2.0] * 22
        gate = comparison(actual, reference)
        self.assertTrue(gate["same_key"])
        self.assertFalse(gate["passed"])
        self.assertFalse(
            certificate({"id": "synthetic", "gates": {"pair": gate}}, "pair")[
                "certified_for_observed_errors"
            ]
        )
        with self.assertRaisesRegex(ValueError, "finite"):
            comparison([float("nan")] * 24, reference)


if __name__ == "__main__":
    unittest.main()
