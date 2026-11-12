"""Unit tests for measure_cost parsing helpers, without running euphoria."""

import unittest

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


class TimeCommandTests(unittest.TestCase):
    def test_unsupported_system_raises(self):
        with self.assertRaises(SystemExit):
            cost.time_command("Windows")

    def test_darwin_returns_long_flag(self):
        self.assertEqual(cost.time_command("Darwin"), ["/usr/bin/time", "-l"])


if __name__ == "__main__":
    unittest.main()
