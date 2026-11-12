"""Exercise comparison validation with tiny WAVs, without loading Demucs."""

import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import numpy
import soundfile

import compare_with_demucs as comparison


class ComparisonTests(unittest.TestCase):
    def test_invalid_stem_and_instrumental_formats_cannot_produce_scores(self):
        audio = numpy.arange(64, dtype="float32").reshape(32, 2) / 100
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            mix = folder / "mix.wav"
            soundfile.write(mix, audio, 44100, subtype="FLOAT")
            reference = numpy.stack([audio.T] * 4)
            argv = ["compare", "--weights", "unused", "--mix", str(mix), "--stems", str(folder)]
            for source in comparison.SOURCES:
                soundfile.write(folder / f"{source}.wav", audio, 44100, subtype="FLOAT")
            with patch.object(sys, "argv", argv), patch.object(comparison, "reference_separation", return_value=reference):
                output = io.StringIO()
                with contextlib.redirect_stdout(output):
                    comparison.main()
                self.assertEqual(json.loads(output.getvalue())["vocals"]["correlation_with_reference"], 1.0)
                for samples, rate in [
                    (audio, 22050),
                    (audio[:, :1], 44100),
                    (audio[:-1], 44100),
                    (numpy.concatenate([audio, audio[:1]]), 44100),
                ]:
                    with self.subTest(shape=samples.shape, rate=rate):
                        for path in [folder / "vocals.wav", folder / "instrumental.wav"]:
                            soundfile.write(path, samples, rate, subtype="FLOAT")
                            arguments = argv if path.name == "vocals.wav" else argv + ["--instrumental", str(path)]
                            with patch.object(sys, "argv", arguments), self.assertRaises(SystemExit):
                                comparison.main()
                            soundfile.write(folder / "vocals.wav", audio, 44100, subtype="FLOAT")


if __name__ == "__main__":
    unittest.main()
