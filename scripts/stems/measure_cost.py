"""Measure the cost of one `euphoria stems` run.

Runs the given euphoria binary once on an audio file with a fresh stem cache
and a freshly copied weights file, wrapped in /usr/bin/time. Prints a JSON
document with the wall time of the run, the seconds it took per minute of
audio, and the peak resident memory of the child process.
"""

import argparse
import json
import os
import platform
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

MODEL_FILENAME = "htdemucs-955717e8.safetensors"


def time_command(system: str) -> list[str]:
    """Return the /usr/bin/time invocation flags for the given system."""
    if system == "Darwin":
        return ["/usr/bin/time", "-l"]
    if system == "Linux":
        return ["/usr/bin/time", "-v"]
    raise SystemExit("measure_cost.py needs /usr/bin/time on macOS or Linux")


def peak_resident_bytes(stderr: str, system: str) -> int:
    """Extract the peak resident set size in bytes from /usr/bin/time output."""
    marker = (
        "maximum resident set size"
        if system == "Darwin"
        else "Maximum resident set size (kbytes):"
    )
    for line in stderr.splitlines():
        if marker in line:
            value_text = (
                line.strip().split()[0]
                if system == "Darwin"
                else line.strip().split(":", 1)[1].strip()
            )
            return int(value_text) * (1 if system == "Darwin" else 1024)
    raise SystemExit("no peak memory in the output of /usr/bin/time")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Measure the wall time and peak memory of one euphoria stems run."
    )
    parser.add_argument("--binary", type=Path, required=True, help="path to the euphoria executable")
    parser.add_argument("--audio", type=Path, required=True, help="audio file to separate")
    parser.add_argument("--weights", type=Path, required=True, help="already downloaded weights file")
    parser.add_argument("--duration-seconds", type=float, required=True, help="length of the audio in seconds")
    parser.add_argument("--threads", type=int, default=None, help="set EUPHORIA_STEMS_THREADS for the child")
    arguments = parser.parse_args()

    system = platform.system()
    with tempfile.TemporaryDirectory() as scratch:
        workspace = Path(scratch)
        models_directory = workspace / "data" / "models"
        models_directory.mkdir(parents=True)
        shutil.copyfile(arguments.weights, models_directory / MODEL_FILENAME)
        output_directory = workspace / "output"
        output_directory.mkdir()

        child_environment = dict(os.environ)
        child_environment["EUPHORIA_DATA_DIR"] = str(workspace / "data")
        if arguments.threads is not None:
            child_environment["EUPHORIA_STEMS_THREADS"] = str(arguments.threads)

        command = (
            time_command(system)
            + [
                str(arguments.binary),
                "stems",
                str(arguments.audio),
                "--output",
                str(output_directory),
                "--force",
            ]
        )
        started = time.monotonic()
        result = subprocess.run(command, capture_output=True, text=True, env=child_environment)
        wall_seconds = time.monotonic() - started
        if result.returncode != 0:
            raise SystemExit(result.stderr)

        peak_bytes = peak_resident_bytes(result.stderr, system)

    summary = {
        "audio_seconds": arguments.duration_seconds,
        "wall_seconds": round(wall_seconds, 1),
        "seconds_per_minute_of_audio": round(wall_seconds / (arguments.duration_seconds / 60), 1),
        "peak_resident_megabytes": round(peak_bytes / 1_000_000),
        "threads": arguments.threads,
    }
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
