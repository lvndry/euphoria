"""Unit tests for measure_cost parsing helpers, without running euphoria."""

import contextlib
import io
import sys
import unittest
from unittest.mock import patch

import measure_cost as cost


class PeakResidentBytesTests(unittest.TestCase):
    def test_parses_macos_sample(self):
        stderr = "        1.23 real         4.56 user         0.78 sys\n          1536000000  maximum resident set size\n"
        self.assertEqual(cost.peak_resident_bytes(stderr, "Darwin"), 1536000000)

    def test_parses_linux_sample(self):
        stderr = '\tCommand being timed: "euphoria"\n\tMaximum resident set size (kbytes): 1500000\n'
        self.assertEqual(cost.peak_resident_bytes(stderr, "Linux"), 1536000000)

    def test_missing_line_raises_for_both_systems(self):
        stderr = "        1.23 real         4.56 user         0.78 sys\n"
        for system in ["Darwin", "Linux"]:
            with self.subTest(system=system):
                with self.assertRaises(SystemExit):
                    cost.peak_resident_bytes(stderr, system)


class ProcessorSecondsTests(unittest.TestCase):
    def test_adds_user_and_system_time_on_both_systems(self):
        darwin = "        1.23 real         4.56 user         0.78 sys\n"
        self.assertAlmostEqual(cost.processor_seconds(darwin, "Darwin"), 5.34)
        linux = "\tUser time (seconds): 4.56\n\tSystem time (seconds): 0.78\n"
        self.assertAlmostEqual(cost.processor_seconds(linux, "Linux"), 5.34)
        with self.assertRaises(SystemExit):
            cost.processor_seconds("nothing", "Linux")


class DurationTests(unittest.TestCase):
    def test_invalid_durations_are_refused_before_running_anything(self):
        for duration in ["0", "-5", "nan", "inf"]:
            with self.subTest(duration=duration):
                argv = [
                    "measure_cost.py",
                    "--binary", "/nonexistent/euphoria",
                    "--audio", "/nonexistent/track.wav",
                    "--weights", "/nonexistent/weights",
                    "--duration-seconds", duration,
                ]
                with patch.object(sys, "argv", argv), patch.object(cost.subprocess, "run") as run:
                    with contextlib.redirect_stderr(io.StringIO()):
                        with self.assertRaises(SystemExit):
                            cost.main()
                    run.assert_not_called()


class TimeCommandTests(unittest.TestCase):
    def test_unsupported_system_raises(self):
        with self.assertRaises(SystemExit):
            cost.time_command("Windows")

    def test_darwin_returns_long_flag(self):
        self.assertEqual(cost.time_command("Darwin"), ["/usr/bin/time", "-l"])


if __name__ == "__main__":
    unittest.main()
