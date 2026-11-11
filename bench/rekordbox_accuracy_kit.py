# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# /// script
# requires-python = ">=3.12"
# ///
"""Builds a kit for measuring rekordbox's own key and tempo accuracy.

    uv run bench/rekordbox_accuracy_kit.py --data bench/data --output ~/Music/euphoria-accuracy-kit
    uv run bench/rekordbox_accuracy_kit.py --data bench/data --verify ~/Music/euphoria-accuracy-kit

The kit holds the benchmark previews in two folders, `key` (GiantSteps Key)
and `tempo` (GiantSteps Tempo), and CHECKLIST.md: how to let rekordbox
analyze them and export its collection as XML, which
`cargo run --release -p euphoria-bench -- rekordbox EXPORT.xml` then scores
with the same rules as Euphoria's detectors.

On APFS the previews are clones, which take no extra space; elsewhere they
are copies. The script reads the previews and writes only into the output
folder, which must not exist yet, and removes it again if building the kit
fails. It never touches a rekordbox library or its settings. --verify
checks that no preview in a kit differs from the one it was made from, to
show that rekordbox changed no file.
"""

import argparse
import filecmp
import shutil
import subprocess
import sys
from pathlib import Path

DATASETS = ("key", "tempo")
EXPORT_NAME = "rekordbox-collection.xml"

CHECKLIST = """# Measure rekordbox's key and tempo accuracy

This kit lets rekordbox analyze the {total} previews that Euphoria's accuracy
is measured on, so the two can be compared on the same files. Your part is
about ten minutes of clicking, then rekordbox works on its own for a while:
an estimate is 15 to 45 minutes for the analysis, depending on the Mac and
on how many tracks rekordbox analyzes at once. That figure is a guess, not a
measurement; write down how long it took.

- `key/` holds the {key_count} previews of GiantSteps Key.
- `tempo/` holds the {tempo_count} previews of GiantSteps Tempo.

43 previews are in both datasets, so the same file name appears in both
folders. That is expected: import both folders whole.

Fill in as you go and send this file back with the export.

| | |
| --- | --- |
| rekordbox version (Help > About rekordbox) | |
| Date | |
| Mac model and macOS version | |
| Track Analysis Mode (step 2) | |
| BPM range (step 2) | |
| Key display format (step 2) | |
| Analysis started, finished | |

## 1. Back up (1 minute)

**File > Library > Backup Library**, and keep the backup until step 6 is
done. It lets you put the library back exactly as it was.

## 2. Settings (2 minutes)

The aim is to measure rekordbox as it ships, so change only what is listed.

1. **Preferences > Analysis > Track Analysis**
   - **Track Analysis Setting**: tick **BPM / Grid** and **KEY**. Note what
     else is ticked and leave it.
   - **Track Analysis Mode** (Normal or Dynamic): leave it, and write it in
     the table above.
   - **BPM range**: leave it, and write it in the table above. rekordbox
     reports tempos only inside this range, so it changes the tempo result.
   - **Auto Analysis**: note whether it is on. Either way works.
2. **Preferences > Analysis > Key Analysis**: make sure **Write the value to
   the ID3 tag** is **off**, so that no file is changed. If it was on, note
   it and switch it back in step 6.
3. **Preferences > View**: write the key display format (Classic or
   Alphanumeric) in the table above. It only changes how keys are shown;
   the scorer reads both.

## 3. Import (2 minutes)

1. In the tree on the left, right click **Playlists > Create New Playlist**,
   name it `Euphoria key`.
2. Open the kit's `key` folder in Finder, select every file (Command A) and
   drag them onto the `Euphoria key` playlist. rekordbox adds them to the
   collection as well.
3. Do the same with a playlist `Euphoria tempo` and the `tempo` folder.
4. Check the counts: `Euphoria key` holds {key_count} tracks and `Euphoria
   tempo` {tempo_count}.

## 4. Analyze (a few minutes of yours, then waiting)

If Auto Analysis is on, analysis has already started. Otherwise open
`Euphoria key`, select all tracks (Command A), right click, **Analyze
Track**, and do the same in `Euphoria tempo`.

Wait until every track in both playlists shows a BPM and a key. Sort by the
BPM column, then by the Key column, to see quickly whether any is empty. A
track left empty counts as a miss, so if a few are left, select them and
analyze them again. Write down the start and end times.

## 5. Export (1 minute)

**File > Export Collection in xml format**, and save it in the kit folder as
`{export_name}`. The file lists your whole collection, including the paths
of your own music, so send only the scores if you would rather keep it
private. To score it:

```
cargo run --release -p euphoria-bench -- rekordbox {kit}/{export_name} bench/data
```

To check that rekordbox changed none of the previews:

```
uv run bench/rekordbox_accuracy_kit.py --data bench/data --verify {kit}
```

## 6. Clean up (2 minutes)

1. Open `Euphoria key`, select all, right click, **Remove from Collection**.
   Do the same in `Euphoria tempo`. Then delete the two playlists.
2. Put back anything you changed in step 2: **Track Analysis Setting**,
   **Write the value to the ID3 tag**.
3. Delete the kit folder once the export is scored.
4. Removing tracks leaves rekordbox's analysis files for them in its own
   data folder. If you want the library exactly as it was, **File >
   Library > Restore Library** from the backup of step 1.
"""


def previews(data, dataset):
    sources = sorted((data / dataset / "audio").glob("*.mp3"))
    if not sources:
        raise SystemExit(f"no previews in {data / dataset / 'audio'}; run bench/download_datasets.sh first")
    return sources


def clone_or_copy(sources, folder):
    """Clones the files into the folder with `cp -c`, or copies them where
    the file system cannot clone. Returns whether they were cloned."""
    result = subprocess.run(["cp", "-c", *map(str, sources), str(folder)], capture_output=True, check=False)
    if result.returncode == 0:
        return True
    for stale in folder.iterdir():
        stale.unlink()
    for source in sources:
        shutil.copy2(source, folder / source.name)
    return False


def build(data, output):
    counts = {}
    cloned = True
    for dataset in DATASETS:
        sources = previews(data, dataset)
        folder = output / dataset
        folder.mkdir()
        cloned = clone_or_copy(sources, folder) and cloned
        made = len(list(folder.glob("*.mp3")))
        if made != len(sources):
            raise SystemExit(f"{folder} holds {made} previews, expected {len(sources)}")
        counts[dataset] = made
    checklist = CHECKLIST.format(
        total=sum(counts.values()),
        key_count=counts["key"],
        tempo_count=counts["tempo"],
        export_name=EXPORT_NAME,
        kit=output,
    )
    (output / "CHECKLIST.md").write_text(checklist, encoding="utf-8")
    return counts, cloned


def verify(data, kit):
    differing = []
    checked = 0
    for dataset in DATASETS:
        for source in previews(data, dataset):
            target = kit / dataset / source.name
            checked += 1
            if not target.is_file() or not filecmp.cmp(source, target, shallow=False):
                differing.append(f"{dataset}/{source.name}")
    if differing:
        print(f"{len(differing)} of {checked} previews are missing or differ from their source:")
        for name in differing:
            print(f"  {name}")
        return 1
    print(f"All {checked} previews match their source byte for byte.")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--data", required=True, type=Path, help="the benchmark data folder, bench/data")
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--output", type=Path, help="new folder for the kit")
    action.add_argument("--verify", type=Path, help="kit folder to compare with the previews")
    arguments = parser.parse_args()

    if arguments.verify:
        return verify(arguments.data, arguments.verify)
    if arguments.output.exists():
        parser.error(f"{arguments.output} exists; choose a new folder")
    output = arguments.output.resolve()
    output.mkdir(parents=True)
    try:
        counts, cloned = build(arguments.data, output)
    except BaseException:
        shutil.rmtree(output, ignore_errors=True)
        raise
    method = "cloned" if cloned else "copied"
    print(f"Kit written to {output}: {counts['key']} key and {counts['tempo']} tempo previews, {method}.")
    print(f"Follow {output / 'CHECKLIST.md'}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
