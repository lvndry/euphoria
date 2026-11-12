---
title: Command line
description: Every command, option, output format and exit status of the euphoria command.
---

# Command line

## euphoria analyze

Shows the key, tempo, energy and sound quality of audio files.

```
euphoria analyze [OPTIONS] <PATHS>...
```

Each path is a file or a folder. Folders are searched recursively for [supported audio files](supported-formats.md). Hidden files and folders are skipped, including the `._` copies macOS leaves on external drives. A file named directly is always analyzed, so an unsupported file is reported as a failure instead of being ignored.

A symbolic link to an audio file is analyzed. A symbolic link to a folder is not entered, so a scan stays inside the folders you named; to include a linked folder, name it on the command line.

How the results are shown depends on where they go:

| Output | Shown as |
| --- | --- |
| A terminal, several files | A [table that fits the window](#the-table-on-a-terminal), then a summary. |
| A terminal, one file, or any output with `--detail` | A [detailed view](#the-detailed-view) of each track. |
| A pipe or a file | The [plain table](#the-plain-table), with the same columns every time, for scripts. |
| Any output with `--json` | [JSON](#json-output). |

A `-` means nothing was detected, for example in silence or a track shorter than one second.

### The table on a terminal

```
$ euphoria analyze ~/Music/DJ
Key                  BPM       Energy              Quality   Time  File
 8A  A minor  ●●●  124.0  ●●○       6  ━━━━━━────  lossless  6:48  Low Tide.flac
10B  D major  ●○○  149.8  ●●●       6  ━━━━━━────  weak      2:00  edits/Intro.m4a

Key and tempo: 1 of 2 tracks rated low confidence (●○○), worth checking by ear.
Sound quality: 1 track lossless, 1 weak on a big system.
hint: --needs-review lists only the tracks to check by ear.
hint: --quality weak lists the tracks worth replacing, and --detail says why.
```

The key is shown in the notation `--key-notation` picks, coloured by its number on the Camelot wheel as in the app, with the key in another notation beside it. The three dots after the key and after the tempo rate how far to trust them: `●●●` is high confidence, `●●○` medium and `●○○` low. On the benchmark a key rated high was exactly right about three times in four and one rated low less than half the time; [accuracy](../concepts/accuracy.md#confidence) has the figures. Tracks with `●○○` are worth checking by ear, and `--needs-review` lists only those. A key or tempo you set yourself in the desktop app shows `you` in place of the dots: see [Corrections](#corrections).

Energy is the [energy rating](../concepts/energy.md) from 1 to 10, with a bar of ten cells. It is Euphoria's own scale, so do not compare it with the energy numbers of other tools.

Quality is the [audio quality](../concepts/audio-quality.md) verdict for DJ use in a word: `lossless`, `good`, `acceptable`, `weak` (weak on a big system) or `re-encoded?` (likely made from a lower quality file). The detailed view gives the reason.

The folder that holds every file is left out of the File column, so a folder shows its file names and the subfolders below it. A name too long for the window is shortened in the middle, keeping its end and extension.

In a narrow window, the least needed columns are left out first: the energy bar, the key in the other notation, the time, the confidence dots, the quality, then the energy. With the dots left out, a key or tempo rated low is marked with a `?`. The key, the tempo and the file always stay.

After the table, standard error gets a summary in plain sentences: how many tracks have a key or tempo rated low, how many have each sound quality, how many files could not be analyzed, and `hint:` lines on where to look next.

### The detailed view

```
$ euphoria analyze ~/Music/DJ/Intro.m4a
Intro.m4a
/Users/you/Music/DJ

  Key            10B  D major
                 ●○○ low confidence, worth checking by ear
                 runner up 9A  E minor
  Tempo          149.8 BPM
                 ●●● high confidence
  Beat grid      first downbeat at 0.412 s, 149.82 BPM
                 ●●○ medium confidence
  Energy         6 of 10  ━━━━━━────
  Sound quality  Weak on a big system
                 96 kbit/s. Fine for practice and small rooms; on a club system the top end and
                 detail suffer.
                 aac, 96 kbit/s, 44.1 kHz, stereo, sound stops at 15.4 kHz
  Loudness       -12.4 LUFS, true peak +1.0 dBTP
  Tuning         at concert pitch, A = 440 Hz
  Length         2:00
```

One file analyzed on a terminal is always shown this way. `--detail` shows every track this way, whatever the output. The view names the confidence of the key and of the tempo, the runner up key, the half or double tempo when a DJ could count the track that way, the reason for the sound quality and the format behind it, the loudness, the beat grid with its confidence, the tuning, and the key changes when the key changes during the track.

### The plain table

Into a pipe or a file, the table has the same columns as before the terminal table existed, with no colour, so scripts that read it keep working:

```
$ euphoria analyze ~/Music/DJ | cat
Key             BPM  Energy  Conf  Quality   Time  File
8A   A minor  124.0       6  H/M   Lossless  6:48  /Users/you/Music/DJ/Low Tide.flac
10B  D major  149.8       6  L/H   Weak      2:00  /Users/you/Music/DJ/Intro.m4a
```

The Conf column rates the key and the tempo, in that order: `H` for high, `M` for medium and `L` for low, or `U` for a value you set yourself. The summary still goes to standard error, so piped output holds only the table.

The tuning of a track and its key over time are in the detailed view and the [JSON output](#json-output).

### Options

| Option | Effect |
| --- | --- |
| `--json` | Print a JSON array instead of a table. |
| `--detail` | Show every track in the [detailed view](#the-detailed-view). Cannot be combined with `--json`. |
| `--needs-review` | List only the tracks whose key or tempo is rated low confidence, and the files that could not be analyzed. |
| `--quality <QUALITY>` | List only the tracks with one of these sound qualities, separated by commas: `lossless`, `good`, `acceptable`, `weak` or `reencoded`. `weak,reencoded` finds the files worth replacing. Combined with `--needs-review`, a track must match both. |
| `--key-notation <NOTATION>` | How keys are shown in the terminal table and the detailed view: `camelot` (the default), `musical` or `open-key`. The plain table and JSON are unchanged. |
| `--color <WHEN>` | `auto` (the default), `always` or `never`. See [colour](#colour). |
| `--tempo-range <SLOWEST-FASTEST>` | Report tempos from SLOWEST up to, not including, FASTEST beats per minute, such as `70-140` for hip hop or `120-200` for drum and bass. The default is `90-180`. FASTEST can be at most twice SLOWEST. See [choosing the range](../concepts/accuracy.md#choosing-the-range). |
| `-j`, `--jobs <N>` | Analyze N files at once. The default is the number of cores, at most 8. |
| `--no-cache` | Analyze every file, and neither read nor update the [cache](#the-analysis-cache). |
| `--reanalyze` | Analyze every file again and replace its cached result. Cannot be combined with `--no-cache`. |
| `-h`, `--help` | Show help. |
| `-V`, `--version` | Show the version. |

A tempo is stored as it was estimated and placed in the range when it is shown, so running again with another `--tempo-range` takes every result from the cache. A tempo with no octave inside a range narrower than an octave, such as 105 BPM with `120-200`, is shown at the octave nearest to the range, here 210.

Analyzing one seven minute track peaks near 0.6 GB of memory. On full length tracks, 4 workers peaked near 2 GB in total and 8 workers between 4.8 and 6.2 GB, which is why the default stops at 8. Lower `--jobs` on a machine with 8 GB of memory or less.

### JSON output

One object per file, in path order, including files that failed:

```json
[
  {
    "path": "/Users/you/Music/DJ/Low Tide.flac",
    "duration_seconds": 408.4,
    "key": { "camelot": "8A", "name": "A minor", "open_key": "1m", "confidence": "high" },
    "runner_up_key": { "camelot": "8B", "name": "C major", "open_key": "1d" },
    "bpm": 170.0,
    "bpm_confidence": "medium",
    "bpm_alternative": 85.0,
    "energy": 6,
    "tuning_cents": -31.3,
    "key_timeline": [
      {
        "start_seconds": 0.0,
        "end_seconds": 250.0,
        "key": { "camelot": "8A", "name": "A minor", "open_key": "1m" }
      },
      {
        "start_seconds": 250.0,
        "end_seconds": 408.4,
        "key": { "camelot": "9A", "name": "E minor", "open_key": "2m" }
      }
    ],
    "beat_grid": { "first_downbeat_seconds": 0.372, "bpm": 170.004, "confidence": "high" },
    "cues": [
      { "seconds": 0.372, "name": "Start", "hot_cue": "A" },
      { "seconds": 45.548, "name": "Main", "hot_cue": "B" },
      { "seconds": 181.077, "name": "Breakdown", "hot_cue": "C" },
      { "seconds": 226.249, "name": "Drop", "hot_cue": "D" },
      { "seconds": 361.775, "name": "Outro", "hot_cue": "E" }
    ],
    "needs_review": false,
    "set_by_user": [],
    "quality": {
      "codec": "flac",
      "lossless": true,
      "bitrate_kbps": 912,
      "sample_rate": 44100,
      "bit_depth": 16,
      "channels": 2,
      "cutoff_hz": null,
      "clipped_share": 0.0,
      "integrated_loudness_lufs": -8.4,
      "true_peak_dbtp": -0.3,
      "verdict": "lossless",
      "reason": "Lossless FLAC with a full spectrum."
    },
    "error": null
  },
  {
    "path": "/Users/you/Music/DJ/notes.txt",
    "duration_seconds": null,
    "key": null,
    "runner_up_key": null,
    "bpm": null,
    "bpm_confidence": null,
    "bpm_alternative": null,
    "energy": null,
    "tuning_cents": null,
    "key_timeline": [],
    "beat_grid": null,
    "cues": [],
    "needs_review": null,
    "set_by_user": [],
    "quality": null,
    "error": "cannot decode /Users/you/Music/DJ/notes.txt: unsupported format: unsupported feature: core (probe): no suitable format reader found"
  }
]
```

`key`, `bpm` and `energy` are `null` when nothing was detected. `energy` is a whole number from 1 to 10. `error` is set only when the file could not be analyzed.

A key is written with its Camelot code, its name and its [Open Key](../concepts/keys-and-camelot.md#open-key-codes) code. The key of the track also has a `confidence` of `high`, `medium` or `low`; the keys of the timeline and the runner up have none. `runner_up_key` is the key that matched the track second best, which is the right key for many tracks whose detected key is wrong, and often the relative key or a key a fifth away. It is `null` when there is no key, and in builds without libkeyfinder.

`bpm` is reported in the `--tempo-range`, and `bpm_confidence` rates how sure the beat itself is, whatever octave it is counted in. `bpm_alternative` is half or double `bpm` when a DJ could plausibly count the track that way, between 75 and 200 BPM or the ends of the range when those are further out, and `null` otherwise. With the default range that is half of a tempo from 150 up and double of one up to 100, where tempo detection most often lands an octave off.

`needs_review` is `true` when the key or the tempo is rated low, `false` otherwise, and `null` for a file that could not be analyzed. A value you corrected is no longer doubtful, so it does not count.

`set_by_user` lists the values you [corrected](#corrections) or [edited](#edited-cues) in the desktop app, any of `"key"`, `"bpm"` and `"cues"`, and is empty otherwise. A corrected key has no `confidence` and no `runner_up_key`, and a corrected tempo no `bpm_confidence` and no `bpm_alternative`: they rate what was detected, which is no longer shown. A corrected tempo also has no `beat_grid`, and none of the cues analysis placed, since both were fitted to the detected tempo; cues you edited stay.

`tuning_cents` is how far the track is tuned from A = 440 Hz, in cents from -50 to 50, where 100 cents is a semitone. Positive is sharp: at 30 the A above middle C sounds at 447.7 Hz, and at -31.3 at 432.1 Hz. It is `null` when the tuning could not be measured, which is the case for silence, for noise and for many tracks that are mostly drums. When it is 20 or more either way, the key is detected after bringing the track back to 440 Hz, so `key` names the nearest key at concert pitch.

`key_timeline` is the key over time, as sections in order that together cover the track from its start to `duration_seconds`, except where the track is silent. Silence has no key, so it belongs to no section: a track with silence in the middle has a gap between two sections there, and one that starts or ends in silence has a first section that starts late or a last one that ends early. A track in which no change of key was found has one section in the key of the track, or one on each side of every silence, all in that key. A track with no key has none. A passage with sound but no harmony, such as a drum break, is not silence and keeps the key of the music around it. The timeline stays in `key` and shows another key only when that key is clearly stronger over at least three windows with evidence, about 22 seconds of audio. Silent gaps remain empty, but windows in the same key on either side count together toward this minimum. Entering and leaving another key each carry a cost, so a brief change in the middle needs stronger or longer evidence than one at the end. On dance music most tracks have one section and `key` is one of the sections. Section boundaries are accurate to a few seconds.

`beat_grid` places every beat and bar of the track with one constant tempo: `first_downbeat_seconds` is the time of the first beat of the first bar, from 0 up to the length of one bar, and `bpm` the tempo of the grid, in the `--tempo-range` like `bpm`, to a thousandth. It can differ from `bpm` by up to 2%, since it is fitted to the beats themselves and `bpm` is not. Every following beat falls 60 / `bpm` seconds after the one before, and every fourth beat starts a bar. `confidence` rates how far to trust the beats, not the choice of downbeat, which is right less often. `beat_grid` is `null` when the track has no tempo. The grid is not shown in the table. See [beat grid](../concepts/accuracy.md#beat-grid).

`cues` lists the cues of the track in order: `seconds` from the start of the track, to a thousandth, `name`, and `hot_cue`, the hot cue pad from `"A"` to `"H"`, or `null` for a memory cue only. The cues analysis placed mark where the sections of the track start, named `Start`, `Main`, `Breakdown`, `Drop` and `Outro`, each on the first beat of a bar of `beat_grid` at a phrase boundary, on pads from A in order, at most 8. They are empty when the track has no beat grid. See [cue points](../concepts/accuracy.md#cue-points). When you [edited the cues](#edited-cues) of a track in the desktop app, `cues` lists yours instead, with the names and pads you gave them, and `set_by_user` includes `"cues"`; an empty list then means you deleted every cue.

[Accuracy](../concepts/accuracy.md#tuning-and-key-over-time) describes how both are measured and how far to trust them, and [confidence](../concepts/accuracy.md#confidence) how the ratings, the runner up and the alternative tempo were set and how often they are right.

`quality` is the [audio quality](../concepts/audio-quality.md) report, or `null` for a file that could not be analyzed:

| Field | Meaning |
| --- | --- |
| `codec` | The audio codec, such as `mp3`, `aac`, `flac`, `alac` or `pcm_s16le`. |
| `lossless` | Whether the codec keeps every bit. |
| `bitrate_kbps` | The average bit rate of the audio data in kbit/s, measured from the audio packets. |
| `sample_rate`, `bit_depth`, `channels` | As the file declares them. `bit_depth` is `null` for lossy files. |
| `cutoff_hz` | The frequency where the content stops, or `null` when it reaches the top of the spectrum. |
| `clipped_share` | The share of samples in runs of three or more at full scale, from 0 to 1. |
| `integrated_loudness_lufs` | Integrated loudness, as EBU R128 describes, on the mono mixdown. `null` for silence. |
| `true_peak_dbtp` | The highest true peak in any channel, in dB relative to full scale. |
| `verdict` | `lossless`, `good`, `acceptable`, `weak-on-a-big-system` or `likely-reencoded`. |
| `reason` | One or two sentences that explain the verdict. |

### Progress and exit status

When standard error is a terminal, each file being analyzed has a line with a spinner and the stage it is in: decoding, with how much is decoded, then finding the tempo, finding the key, rating the energy, finding the beat grid and checking the quality. After five seconds the line also shows how long the file has taken, so a long track never looks stuck. With more than one file, a bar above the lines counts the finished files and estimates the time left. Files served from the cache get no line. Nothing of this is written when standard error is not a terminal.

A debug build analyzes many times slower than a release build. When a run of a debug build has taken ten seconds, it says so once on standard error. Release builds never do.

When some results came from the cache, a line such as `120 of 124 tracks came from the cache.` is written to standard error, so piped output holds only the table or the JSON.

The command exits with status 0 when every file was analyzed. If any file failed, each one is listed on standard error as `error: <path>: <reason>` after the table, and the command exits with status 1. It also exits with status 1, after printing the results, if saving them to the cache failed partway through the run. A cache that cannot be used at all only causes a [warning](#when-the-cache-cannot-be-used) and does not change the exit status.

## euphoria export rekordbox

Analyzes audio files and writes the results as a rekordbox XML file, which rekordbox imports with keys, tempos and playlists. [Export to rekordbox](../guides/export-to-rekordbox.md) walks through the export and the import.

```
euphoria export rekordbox [OPTIONS] --output <FILE> <PATHS>...
euphoria export rekordbox [OPTIONS] --output <FILE> --playlist <NAME>
```

Paths are searched as in [`euphoria analyze`](#euphoria-analyze), and results come from the [cache](#the-analysis-cache) when the tracks were analyzed before. Each path is turned into a full path first, taken from the current directory when it is relative, with any symbolic link in it followed, because rekordbox finds tracks by their full path.

```
$ euphoria export rekordbox ~/Music/DJ --output ~/Music/euphoria.xml
Exported 340 tracks in 12 playlists to /Users/you/Music/euphoria.xml (336 from the cache)
To import it in rekordbox:
  1. Preferences > View > Layout: tick rekordbox xml.
  2. Preferences > Advanced > Database: choose this file.
  3. Refresh rekordbox xml in the tree, then import its tracks or playlists.
Full guide: https://github.com/lvndry/euphoria/blob/main/docs/guides/export-to-rekordbox.md
```

### Options

| Option | Effect |
| --- | --- |
| `-o`, `--output <FILE>` | The file to write. Required. |
| `--force` | Replace the output file if it exists. Without it, an existing file is an error and nothing is analyzed. |
| `--key-notation <NOTATION>` | `camelot` (the default) writes keys such as `8A`, `musical` writes keys such as `Am`. |
| `--no-playlists` | Write the tracks only, without playlists. |
| `--beat-grid` | Write each track's [beat grid](../concepts/accuracy.md#beat-grid), except grids rated low confidence. See [beat grids](#beat-grids). |
| `--playlist <NAME>` | Export a playlist built in the [desktop app](../guides/desktop-app.md#playlists) in place of paths: its tracks, and one playlist in its order. The name is matched ignoring case; [`euphoria playlists`](#euphoria-playlists) lists them. Each file is read and compared with the track that was added; one that is no longer anywhere, or that was replaced at its path by other audio, is left out with a warning. Cannot be combined with paths or `--no-playlists`. |
| `--cues` | Write each track's [cue points](../concepts/accuracy.md#cue-points) as memory cues and hot cues, on tracks whose beat grid is written, or the cues you [edited](#edited-cues) in the desktop app in their place. Needs `--beat-grid` and `--collection`. See [cue points](#cue-points). |
| `--collection <FILE>` | Your rekordbox collection exported as XML. Tracks that have cues or loops there are left out of the file. Only with `--cues`. It is never replaced by the export, even with `--force`. |
| `--allow-unmatched-cued-tracks` | Export even though some tracks with cues in the collection match no file on this computer. See [cue points](#cue-points). |
| `--tempo-range <SLOWEST-FASTEST>` | The range tempos are written in, `90-180` by default, as for [`euphoria analyze`](#options). |
| `-j`, `--jobs <N>` | Analyze N files at once. The default is the number of cores, at most 8. |
| `--no-cache` | Analyze every file, and neither read nor update the [cache](#the-analysis-cache). |
| `--reanalyze` | Analyze every file again and replace its cached result. Cannot be combined with `--no-cache`. |
| `-h`, `--help` | Show help. |

### What is written

Each track has its full path, its duration, its key and tempo when Euphoria found them, its energy rating as the comment `Energy 7` when it has one, and its title and artist. Title and artist are read from the file's own tags: ID3 in MP3 and AIFF, Vorbis comments in FLAC, MP4 metadata in M4A and the `INFO` list in WAV. A file with no title is given its file name without the extension. So is a file whose tags cannot be read, which is exported without an artist.

Playlists mirror the folders:

- Each folder named on the command line becomes one entry at the top level, in the order given.
- A folder holding only tracks becomes a playlist. A folder holding other folders with tracks becomes a playlist folder, and tracks sitting directly in it go into a playlist of the same name, first inside it.
- A folder with no tracks anywhere inside is left out.
- Files named directly go into one last playlist, named after the output file without its extension.
- When one named folder is inside another, each gets its own entry.
- An entry that would have the same name as an earlier entry beside it gets ` (2)`, ` (3)` and so on after its name.

### Beat grids

With `--beat-grid`, a track whose beat grid is rated medium or high gets it in the file, as its first downbeat and its tempo with two decimals; that tempo is then the track's tempo in the file too. Rounding to two decimals changes the tempo by at most 0.005 BPM, which moves a beat T seconds into the track by at most T × 0.005 / BPM seconds. At the end of a seven minute track, T = 420, that is 2.1 / BPM seconds: 23 ms at 90 BPM, 16 ms at 128 BPM and 12 ms at 180 BPM, or 30 ms at 70 BPM with `--tempo-range 70-140`. A track whose grid is rated low, or that has none, is written without one, and rekordbox draws its own grid when it analyzes the track. A line on standard error says how many tracks got a grid:

```
Beat grids for 312 of 340 tracks; the others have no clear beat or a grid rated low confidence, so rekordbox analyzes them.
```

**Import To Collection** of a track that is already in your collection replaces its key, tempo and comment with the ones in the file (tested in rekordbox 7.2.19). Expect it to replace a grid you adjusted by hand in rekordbox too; that was not tested. Read [export to rekordbox](../guides/export-to-rekordbox.md#import-into-rekordbox) before importing tracks you have already prepared.

### Cue points

With `--cues`, each track whose beat grid is written also gets its cues, each twice: as a memory cue and as a hot cue, on pads from A in order. Cues are off unless asked for.

A track whose cues you [edited](#edited-cues) in the desktop app gets yours instead, with your names: each as a memory cue, and also as a hot cue on its pad when it has one. Your cues are exported whether or not the track's beat grid is, since you placed them yourself, and a track whose cues you all deleted gets none. The rule below still applies: a track with cues of its own in your rekordbox collection is left out, whatever cues you edited in Euphoria.

Euphoria never changes the cues of a track that already has some of its own in rekordbox. `--cues` therefore needs `--collection`, naming your collection exported from rekordbox with **File > Export Collection in xml format**. A track listed there with any cue, hot cue or loop is left out of the file entirely, from the collection and from every playlist. **Import To Collection** replaces what rekordbox has for a track with what the file says, and was not tested on cues, so leaving such a track out is the only way to be sure no import of the file can change its cues. Export again without `--cues` to update the key and tempo of those tracks. Standard error gives the counts:

```
Cues for 296 of 324 tracks; tracks without a beat grid in the file get none, unless you edited their cues in the app.
Left out 16 tracks that already have cues in your rekordbox collection, so importing this file cannot change them. Export again without --cues to update their key and tempo.
```

When every track already has cues, nothing is written and the command exits with status 1.

A track with cues in the collection is recognised by its file. When such a track matches no file on this computer, because it has no location, its location is not a local file, or no file is at its path (a collection exported on another computer, a moved or deleted file, a drive that is not mounted), Euphoria cannot tell whether it is one of the tracks being exported and so cannot protect its cues. The export then stops before analyzing anything, lists those tracks by title, artist and location, and writes nothing. Export the collection from rekordbox on this computer, or relocate or remove those tracks in rekordbox. If none of them is a track you are exporting, pass `--allow-unmatched-cued-tracks`: the export goes ahead and lists them again under "Not protected".

The output file is never the collection file, however either path is written, through a symbolic link, with `.` or `..`, or another spelling of the same file, and `--force` does not change this.

The file is written under a temporary name in the folder of the output file and moved into place once it is complete, so the output file is never half written. Without `--force` it only takes a name that is free: a file that another program creates at the output path while the export runs is kept, and the export fails.

### Output and exit status

Nothing is written to standard output. Standard error gets the progress when it is a terminal, one line saying how many tracks and playlists were exported and where, with `--beat-grid` one line saying how many got a grid, with `--cues` one line saying how many got cues, and the steps to import the file into rekordbox. The summary ends with how many tracks came from the cache when some did.

Only a file that could not be analyzed is left out of the export. Such files are listed on standard error with the reason, before the summary, and the command exits with status 1 after writing the file for the other tracks:

```
error: /Users/you/Music/DJ/notes.txt: cannot decode /Users/you/Music/DJ/notes.txt: unsupported format: unsupported feature: core (probe): no suitable format reader found
Exported 339 tracks in 12 playlists to /Users/you/Music/euphoria.xml
Left 1 file out of the export; the errors above say why.
```

A track whose key and tempo are known but whose tags cannot be read is still exported, under its file name and without an artist. This happens when the result came from the cache and the file can no longer be opened, for example after its permissions changed. Such tracks are listed as warnings before the summary, and a warning alone does not change the exit status:

```
warning: /Users/you/Music/DJ/Low Tide.flac: named after its file, as its title could not be read: cannot open /Users/you/Music/DJ/Low Tide.flac: Permission denied (os error 13)
Exported 340 tracks in 12 playlists to /Users/you/Music/euphoria.xml (340 from the cache)
```

A damaged tag does not cause the warning. Either the file still opens and is exported under its file name, or the damage stops the file from being decoded and it is listed as left out.

The command also exits with status 1, and writes no file, when:

- the output file exists and `--force` was not given, or its folder does not exist,
- the output file is one of the audio files to export, reached by any path or symbolic link, or it has an audio file extension and is inside a folder to export. `--force` does not change this, so an export never replaces a track,
- no audio file was found, or none could be exported,
- folders are nested more than 32 levels deep, the most a rekordbox XML file can hold. The error names the first folder beyond the limit.

As with `euphoria analyze`, a cache that cannot be used only causes a [warning](#when-the-cache-cannot-be-used), and a failure to save results to the cache partway through gives status 1 after the file is written.

## euphoria export traktor

Analyzes audio files and writes the results as a Traktor collection file (NML), which Traktor Pro imports with **Import Another Collection**. [Export to Traktor](../guides/export-to-traktor.md) walks through the import. **Not tested in Traktor.**

```
euphoria export traktor [OPTIONS] --output <FILE> <PATHS>...
euphoria export traktor [OPTIONS] --output <FILE> --playlist <NAME>
```

It works as [`euphoria export rekordbox`](#euphoria-export-rekordbox) does: the same paths, cache, playlists per folder, `--playlist`, refusals, exit status and temporary file. The differences:

| Option | Effect |
| --- | --- |
| `--key-notation <NOTATION>` | `camelot` (the default), `musical` or `open-key`, for the Key Text column. The Key column is filled from the key itself. |
| `--beat-grid` | A grid marker at each track's first downbeat, with the grid's tempo in six decimals, except grids rated low confidence. |
| `--cues` | Hot cues 1 to 8 at each track's sections, on tracks whose grid is written. No memory copy of each hot cue is added. Needs `--beat-grid` and `--collection`. |
| `--collection <FILE>` | Your Traktor `collection.nml`, only read. Tracks with a cue, hot cue or loop of their own there are left out of the file. Grid markers and Traktor's `AutoGrid` hot cue on a grid marker do not count. It is never replaced, even with `--force`. |
| `--startup-volume <NAME>` | The name of the Mac disk the system is on, which Traktor writes as the volume of every path outside `/Volumes`. Found from `/Volumes` when not given. |

Each track has its location as volume, folders and file, its title and artist from its tags, its duration, its key in `INFO KEY` and `MUSICAL_KEY`, its tempo, and its energy rating as the comment `Energy 7`. A path that is not valid Unicode cannot be written in the file and is an error. Two paths that name the same file to Traktor, such as `/a.mp3` and `/Volumes/Macintosh HD/a.mp3`, are an error too.

## euphoria export m3u8

Writes a playlist built in the desktop app as an M3U8 file, the playlist format nearly every DJ and music program opens: Serato, Traktor, Engine DJ, VirtualDJ, Mixxx and music players.

```
euphoria export m3u8 --playlist <NAME> --output <FILE> [--force] [--no-cache]
```

```
$ euphoria export m3u8 --playlist "Peak time" --output ~/Music/peak-time.m3u8
Exported 14 tracks of Peak time to /Users/you/Music/peak-time.m3u8
```

The file starts with `#EXTM3U` and `#PLAYLIST:` and the playlist's name, then gives each track as an `#EXTINF:` line with its length in seconds and "Artist - Title" from its tags, followed by its full path, in UTF-8. A length the [cache](#the-analysis-cache) does not know is written as `-1`. Nothing is analyzed.

| Option | Effect |
| --- | --- |
| `--playlist <NAME>` | The playlist, matched ignoring case. Required. |
| `-o`, `--output <FILE>` | The file to write. Required. |
| `--force` | Replace the output file if it exists. |
| `--no-cache` | Neither look for files that moved where the cache last saw them, nor read lengths from it. |

Each file is read and compared with the track that was added before it is exported. A track that is no longer anywhere, or whose file was replaced at its path by other audio, is left out with a warning, and the command then exits with status 1. A path the format cannot hold, such as one with a line break in it, is an error and nothing is written. The file is written in one step, as for [`euphoria export rekordbox`](#what-is-written).

## euphoria playlists

Lists the playlists built in the [desktop app](../guides/desktop-app.md#playlists), in the app's order, each with its label and how many tracks it holds:

```
$ euphoria playlists
🔥 Peak time  (14 tracks)
Warmup  (9 tracks)
```

Playlists are kept in `playlists.sqlite3` beside the [cache](#where-the-cache-is-stored), each track by the content of its file and its last path, so `euphoria cache clear` leaves them as they are.

## euphoria tags write

Analyzes audio files and writes the key, tempo and energy into their tags, after showing every change and asking for confirmation. [Write tags into your files](../guides/write-tags.md) walks through it, and [Tags](tags.md) lists the fields and describes how a file is rewritten.

```
euphoria tags write [OPTIONS] <PATHS>...
```

Paths are searched as in [`euphoria analyze`](#euphoria-analyze), and results come from the [cache](#the-analysis-cache) when the tracks were analyzed before.

```
$ euphoria tags write ~/Music/DJ
/Users/you/Music/DJ/low tide.flac
  INITIALKEY: (none) -> 8A
  KEY: (none) -> 8A
  BPM: (none) -> 124.04
  COMMENT: https://example.com/track/42 -> 8A - Energy 7 - https://example.com/track/42
340 files to change, 2 already up to date.
Write these changes to 340 files? [y/N] y
Wrote the tags of 340 files.
hint: To put the old tags back, run `euphoria tags undo`.
```

### Options

| Option | Effect |
| --- | --- |
| `--dry-run` | Show the preview and write nothing. Cannot be combined with `--yes`. |
| `-y`, `--yes` | Write without asking for confirmation. |
| `--key-notation <NOTATION>` | `camelot` (the default) writes keys such as `8A`, `musical` writes keys such as `Am`, `open-key` writes keys such as `1m`. Applies to the key field and to the label. |
| `--no-tempo` | Leave the tempo field alone. |
| `--tempo` | Write the tempo with the `serato` preset, which leaves it out otherwise. Cannot be combined with `--no-tempo`. |
| `--label <WHERE>` | Where the key and energy label, such as `8A - Energy 7`, goes: `comment-start` (the default), `comment-end`, `grouping`, or `none` to write no label. The text already in the field is kept. |
| `--preset <PROGRAM>` | The DJ program whose tag fields are written: `generic` (the default), `rekordbox`, `serato`, `traktor`, `engine-dj`, `virtualdj` or `mixxx`. See [use Euphoria with your DJ program](../guides/dj-programs.md). |
| `--tempo-range <SLOWEST-FASTEST>` | The range tempos are written in, `90-180` by default, as for [`euphoria analyze`](#options). |
| `-j`, `--jobs <N>` | Analyze N files at once. The default is the number of cores, at most 8. |
| `--no-cache` | Analyze every file, and neither read nor update the [cache](#the-analysis-cache). |
| `--reanalyze` | Analyze every file again and replace its cached result. Cannot be combined with `--no-cache`. |
| `-h`, `--help` | Show help. |

### Preview and confirmation

The preview goes to standard output: the path of each file that would change, then one line per field with the value it has and the value it would get. `(none)` stands for a field the file does not have. Files that already hold the values are not listed and are never touched. Producing the preview only reads the files.

Standard error then gets one line with the number of files to change, the number already up to date and the number left out because of errors, and the question. Only `y` or `yes` writes. With `--dry-run` there is no question. When there is nothing to change there is no question either.

When standard input is not a terminal and `--yes` was not given, nobody can answer the question. The command then writes nothing, says so with `error: nothing was written` and a hint, and exits with status 1. An answer sent through a pipe is not read.

### What is written and how

Each file is written as described in [Tags](tags.md#how-a-file-is-rewritten): only the fields in the preview change, every other byte of the file is carried over, and the new file is checked against the original before it takes its place. The backup of each file is saved in the `tag-writes` folder of the [data folder](#where-the-cache-is-stored) before the file is changed.

### Exit status

Paths, tag values and error messages come from the files, so before they are shown, characters that could act on the terminal, such as a carriage return, an escape sequence or a text direction override, are replaced by visible escapes such as `\r` and `\u{1b}`. A file cannot erase or disguise the preview you are asked to confirm. The same holds for the table of `euphoria analyze` and the messages of `euphoria export`. What is written into the tags is the real text. JSON output carries the real text with JSON's own escapes.

A file that fails does not stop the others. A file that cannot be analyzed, or whose tags cannot be read or written, is listed on standard error as `error: <path>: <reason>` and left unchanged. That includes the files the tag writer refuses, such as a file with more than one hard link or a file another program changed during the write.

If a file was changed but the log of the write could not note it, for example because the disk filled up, the file is listed as `warning: <path>: the file was changed, but the record of it could not be saved`. Its backup was saved before the change, so `euphoria tags undo` still restores it.

The command exits with status 0 when every file was written or was already up to date, and also when you answered no. It exits with status 1 when any file failed, when a change could not be recorded, when it could not ask for confirmation, and, after writing, when saving analysis results to the cache failed partway.

## euphoria tags set

Edits tag fields by hand across many files at once: give a field a value, remove it, find and replace in it, or add or remove a prefix or a suffix. It shows the same preview and asks the same question as [`euphoria tags write`](#euphoria-tags-write), writes the same safe way, and [`euphoria tags undo`](#euphoria-tags-undo) puts the old tags back.

```
euphoria tags set [OPTIONS] <PATHS>...
```

```
$ euphoria tags set --field "genre=Deep House" --field year=2024 ~/Music/DJ/new
/Users/you/Music/DJ/new/low tide.flac
  GENRE: House -> Deep House
  DATE: (none) -> 2024
...
14 files to change, 0 already up to date, 0 left out.
Write these changes to 14 files? [y/N]
```

Nothing is analyzed. The fields are the same in every format:

| Field | ID3 (MP3, WAV, AIFF) | MP4 (M4A, MP4) | Vorbis (FLAC, Ogg Vorbis) |
| --- | --- | --- | --- |
| `title` | `TIT2` | `©nam` | `TITLE` |
| `artist` | `TPE1` | `©ART` | `ARTIST` |
| `album` | `TALB` | `©alb` | `ALBUM` |
| `genre` | `TCON` | `©gen` | `GENRE` |
| `year` | `TYER`, or `TDRC` in an ID3v2.4 tag | `©day` | `DATE` |
| `track` | `TRCK` | `trkn` | `TRACKNUMBER` |
| `comment` | `COMM` | `©cmt` | `COMMENT` |
| `grouping` | `TIT1` | `©grp` | `GROUPING` |
| `key` | `TKEY` | `----:com.apple.iTunes:initialkey` | `INITIALKEY` |
| `bpm` | `TBPM` | `----:com.apple.iTunes:BPM` | `BPM` |

`tempo`, `date` and `tracknumber` are accepted for `bpm`, `year` and `track`. When a file also has another field of the same meaning, such as the `KEY` field of a FLAC or Ogg Vorbis file, the integer `tmpo` tempo of an M4A file or the `GRP1` grouping of an MP3, that field gets the same change, so programs that read either agree. A WAV file's title, artist, album, genre, date, track number and comment also go into its RIFF `INFO` list, which rekordbox and others read in place of the ID3 chunk.

A comment or a grouping keeps the key and energy label that `tags write` put there, such as `8A - Energy 7`: the edit changes your own text next to it. `--field "comment=Peak opener"` turns `8A - Energy 7 - Purchased` into `8A - Energy 7 - Peak opener`, `--clear comment` leaves `8A - Energy 7`, and a value typed with its label, such as `8A - Energy 7 - Opener`, replaces the label rather than adding a second one. Only a label with an energy rating is recognised, so text of yours that starts with a key is kept as it is.

### Options

| Option | Effect |
| --- | --- |
| `--field <FIELD=VALUE>` | Give a field a value. An empty value, such as `album=`, removes the field. May be given more than once. |
| `--clear <FIELD>` | Remove a field. May be given more than once. |
| `--replace <FIELD>` with `--find <TEXT>` and `--with <TEXT>` | Replace every occurrence of the text in the field. Letters match in either case unless `--match-case` is given. `--with ""` deletes the text. |
| `--add-prefix <FIELD=TEXT>`, `--add-suffix <FIELD=TEXT>` | Put text in front of or after a field, unless it is already there. A file without the field is left without it. |
| `--remove-prefix <FIELD=TEXT>`, `--remove-suffix <FIELD=TEXT>` | Take text off the front or the end of a field, where it is there. |
| `-y`, `--yes` | Write without asking. |
| `--dry-run` | Show the preview and write nothing. |
| `-h`, `--help` | Show help. |

Several edits of the same field apply in the order of this table. A value that leaves nothing removes the field. A tempo must be a positive number, and a track number a number such as `7` or `7/12`; a file whose value would not be one is listed as an error and left unchanged.

The preview, the question, what is written and the exit status are as for [`euphoria tags write`](#preview-and-confirmation). A file the [cache](#the-analysis-cache) knew stays known after its tags change, so it is not analyzed again.

## euphoria tags undo

Puts back the tags that the last `euphoria tags write` or `euphoria tags set` replaced, or the last write or edit made in the desktop app.

```
$ euphoria tags undo
/Users/you/Music/DJ/low tide.flac
...
Restored the tags of 340 files.
```

Each restored file is listed on standard output and is byte for byte what it was before the write. Running the command again undoes the write before that one. When there is nothing to undo it says so and exits with status 0.

A file whose tags changed after the write is not restored, so that the later change is not lost. It is listed on standard error with the reason, the other files are restored, and the command exits with status 1. The backup of such a file is kept, and running the command again tries it again. An earlier write cannot be undone while such a file remains.

A write that was interrupted can be undone as well. Files it had not changed yet are counted in the summary as not changed by the write.

## euphoria shift

Writes a copy of a track in another key, with the same tempo. The pitch moves by up to 6 semitones either way while every beat stays where it was. The copy is a 24 bit WAV or FLAC file tagged with the new key, and the original is never changed.

```
euphoria shift [OPTIONS] -s <N> -o <FILE> <FILE>
```

```
$ euphoria shift -s 2 track.flac -o "track (+2).flac"
Shifted up 2 semitones, from 8A A minor to 9A B minor, to /Users/you/Music/track (+2).flac
```

### Options

| Option | What it does |
| --- | --- |
| `<FILE>` | The track to shift. It is only read, never changed. |
| `-s`, `--semitones <N>` | How many semitones to move the key, from -6 to 6: 2 moves A minor to B minor, -3 moves it to F# minor. A shift of 0 is an error, since nothing would be written. |
| `-o`, `--output <FILE>` | The file to write, ending in .wav or .flac. Required. |
| `--force` | Replace the output file if it exists. |
| `--key-notation <NOTATION>` | How the shifted key is written into the copy's tags: `camelot` (the default), `musical` or `open-key`. |
| `-j`, `--jobs <N>` | Analyze N files at once. The default is the number of cores, at most 8. |
| `--no-cache` | Analyze the track without reading or updating the [cache](#the-analysis-cache). |
| `--reanalyze` | Analyze the track again and replace its cached result. Cannot be combined with `--no-cache`. |
| `--tempo-range <SLOWEST-FASTEST>` | The range tempos are reported in, `90-180` by default, as for [`euphoria analyze`](#options). |

The key of the original comes from the [cache](#the-analysis-cache) when the track was analyzed before, with the key you [corrected](#corrections) when there is one.

On success, one line goes to standard error: how far the track moved, its key before and after, and where the copy is. The command exits with status 0 when the file is written. It exits with status 1 with a message on standard error when the shift is 0 semitones, the file is not audio Euphoria reads, the output does not end in `.wav` or `.flac`, the output exists without `--force`, or the analysis fails.

## euphoria stems

Separates a track into vocals, drums, bass and the rest, with the Demucs htdemucs model, and writes one WAV file per stem. The model, 84 MB, is downloaded the first time. Separation runs on this computer.

```
euphoria stems [OPTIONS] -o <DIR> <FILE>
```

```
$ euphoria stems track.flac --output stems
Drums   /Users/you/Music/stems/drums.wav
Bass    /Users/you/Music/stems/bass.wav
Other   /Users/you/Music/stems/other.wav
Vocals  /Users/you/Music/stems/vocals.wav
```

### Options

| Option | What it does |
| --- | --- |
| `<FILE>` | The audio file to separate. |
| `-o`, `--output <DIR>` | The folder to write the stems to, created when missing. Required. |
| `--force` | Replace stem files already in the folder. |

The stems are written as `vocals.wav`, `drums.wav`, `bass.wav` and `other.wav`, 16 bit stereo WAV files at 44.1 kHz. Each one is listed on standard output with its path, and standard error gets a note: how long the separation took, or that the stems came from the cache. Separated tracks are cached, so separating the same track again is instant.

The command exits with status 0 when the stems are written. It exits with status 1 with a message on standard error when the file is not there, a stem file already exists in the folder without `--force`, an output would replace the input file, or the model download or the separation fails.

## euphoria corrections export

Writes every key and tempo you [corrected](#corrections) as [JSON Lines](https://jsonlines.org/): one JSON object per line, one line per stored content hash. It is meant for training and evaluating a key or tempo model on the tracks you checked by ear, and it is written on your computer and sent nowhere.

```
euphoria corrections export [--output <FILE>] [--force] [--include-paths]
```

```
$ euphoria corrections export -o corrections.jsonl
Exported 42 corrections to /Users/you/corrections.jsonl
```

| Option | Effect |
| --- | --- |
| `-o`, `--output <FILE>` | The file to write. Without it the lines go to standard output and the summary to standard error. |
| `--force` | Replace the output file if it exists. The source cache and its SQLite companion files cannot be outputs, even with this option. |
| `--include-paths` | Add the path each file was last seen at. |

Nothing is analyzed and no audio file is read: the lines come from the [cache](#the-analysis-cache), oldest correction first. With no cache, or a cache without corrections, nothing is written to standard output, an output file is empty, and a note says so. A cache that cannot be read is an error, and the command exits with status 1. The file is written in one step, as for [`euphoria export rekordbox`](#what-is-written).

### What each line holds

By default a line names its track only by the hash of the file's content: no path and no file name. A program that has the same files finds each one by hashing it the same way. The hash does not hide which track a line is about from someone who has the same file, since they can hash it and look for the result, so treat an export as a list of the tracks you own.

```json
{"schema_version":1,"content_hash":"708c038ada074ff39e9b15496e5116710665a6e916e354e580232fea61674e4e","corrected_key":{"name":"E minor","camelot":"9A","tonic":4,"mode":"minor"},"corrected_bpm":null,"corrected_at":1794409200,"detected_key":{"name":"A minor","camelot":"8A","tonic":9,"mode":"minor"},"detected_bpm":124.0,"analyzer_version":"12+libkeyfinder","analyzed_at":1794408000}
```

| Field | Meaning |
| --- | --- |
| `schema_version` | `1`. It goes up when a field is removed or changes meaning; a new field can appear without it changing. |
| `content_hash` | The BLAKE3 hash of every byte of the file, tags and artwork included, as 64 lowercase hexadecimal digits: what `b3sum` prints for the file. |
| `corrected_key` | The key you set, or `null` when you corrected only the tempo. `name` is the musical name, `camelot` the Camelot code, `tonic` the pitch class of the tonic from 0 (C) to 11 (B), and `mode` `"major"` or `"minor"`. |
| `corrected_bpm` | The tempo you set, as you typed it, or `null` when you corrected only the key. |
| `corrected_at` | When you set the correction, in seconds since 1 January 1970 UTC. |
| `detected_key` | The key Euphoria detected for the same content, in the same shape, or `null` when it found none or the track has no stored analysis. |
| `detected_bpm` | The tempo Euphoria detected, between 90 and 180 BPM, or `null`. |
| `analyzer_version` | The version of the analyzer that detected them, which can be older than the one you run, or `null` when the track has no stored analysis. |
| `analyzed_at` | When that analysis was stored, in seconds since 1 January 1970 UTC, or `null`. |
| `path` | Only with `--include-paths`: the absolute path the file was last seen at, or `null` when the cache never recorded one. The file may have moved since. |

A correction follows its file through tag writes. The export includes both its new content hash and older entries retained so a tag write can be undone. One audio track can therefore have several lines for different versions of its tags, and an older hash need not match a file that still exists. A file whose tags another program changed since also has a different hash, and a program matching the export by hash will not find it.

## The analysis cache

`euphoria analyze` and `euphoria export` remember every result in a SQLite database on your computer, so a track is analyzed once. A later run uses the remembered result instead of analyzing the file again.

A track is recognised by its content, not by its name or folder:

- A file that was renamed or moved is still recognised, and is not analyzed again.
- A file whose content changed is analyzed again. Editing tags changes the content, so a file retagged by another program is analyzed once more. `euphoria tags write` and `euphoria tags undo`, and the app, record the files they change, so those are not analyzed again; with `--no-cache` the write records nothing.
- A file that could not be analyzed is not remembered, and is tried again on the next run.
- Results from a version of Euphoria that detected keys or tempo differently, or rated energy with a different version of its scale, are ignored and replaced. This includes results stored before Euphoria rated energy, results stored before tempo was reported between 90 and 180 BPM, which changed the tempo of many tracks and the energy rating of some, results stored before the tuning and the key over time were measured, and results stored before keys and tempos were rated for confidence. The first run after such an update analyzes every track once more.
- A file that changes while it is being analyzed is not remembered, and is analyzed again on the next run.

A file found where it was last seen, with the same size and modification time, is not read at all. Any other file is read once to compute a BLAKE3 hash of its content, which takes a small fraction of the time an analysis does.

### Corrections

When you correct a key or a tempo in the [desktop app](../guides/desktop-app.md#check-the-key-by-ear-and-correct-it), the correction is kept in the cache, recognised by the file's content like an analysis. `euphoria analyze` shows the corrected value and marks it `you` in the terminal table, `set by you` in the detailed view, `U` in the Conf column of the plain table and in `set_by_user`, `euphoria export rekordbox` exports it, and `euphoria tags write` writes it. With `--no-cache`, or without a cache, corrections are not read and the detected values are used. To show a correction, a file at its recorded path with its recorded size and modification time is trusted, as for an analysis; before a correction goes into tags or an export, the whole file is read to be sure it is the file that was corrected. When that check fails, because the file cannot be read or the cache cannot be queried, the file is left out of the write or the export and listed with the reason, rather than given the detected values.

Corrections are kept apart from the analyses, so neither `--reanalyze`, a new version of Euphoria analyzing every track again, nor `euphoria cache clear` loses them. They follow a file that is moved or renamed, and one whose tags `euphoria tags write`, `euphoria tags undo` or the app change. A file whose content is changed by another program is a new file to the cache, and loses its corrections. Resetting a value in the app removes its correction. [`euphoria corrections export`](#euphoria-corrections-export) writes them all out, to train a key model on.

### Edited cues

When you [edit the cues](../guides/desktop-app.md#cue-points) of a track in the desktop app, your set of cues is kept in the cache in the same way, in a table of its own: by the file's content, apart from the analyses, so `--reanalyze`, a new version of Euphoria and `euphoria cache clear` keep it, and it follows a file that is moved, renamed or has its tags written. Your set replaces the cues analysis placed, even when you deleted every cue: `euphoria analyze --json` lists your cues and `"cues"` in `set_by_user`, and [`euphoria export rekordbox --cues`](#cue-points) exports them. As for corrections, the whole file is read before its cues go into an export, and a file whose edits cannot be checked is left out with the reason. **Reset to automatic cues** in the app forgets your set. With `--no-cache` edited cues are not read.

### Where the cache is stored

| System | Location |
| --- | --- |
| macOS | `~/Library/Application Support/euphoria/analysis-cache.sqlite3` |
| Linux | `$XDG_DATA_HOME/euphoria/analysis-cache.sqlite3`, or `~/.local/share/euphoria/analysis-cache.sqlite3` when `XDG_DATA_HOME` is not set |

Set the `EUPHORIA_DATA_DIR` environment variable to keep the database in another folder:

```
EUPHORIA_DATA_DIR=/Volumes/Music/.euphoria euphoria analyze /Volumes/Music
```

A relative value is taken from the directory the command is run in, so `EUPHORIA_DATA_DIR=data` run in `/Users/you` means `/Users/you/data`. `euphoria cache path` prints the absolute location that results.

Several `euphoria analyze` and `euphoria export` commands can run at the same time and share one database.

### When the cache cannot be used

The cache only saves time, so a problem with it never stops an analysis. If the database cannot be opened or read, for example because the file is damaged, its folder is read only, or it was written by a newer version of Euphoria, `euphoria analyze` prints a warning on standard error and analyzes every file without the cache:

```
warning: analyzing without the cache: cannot use the analysis cache at /Users/you/Library/Application Support/euphoria/analysis-cache.sqlite3: file is not a database
hint: Run `euphoria cache clear` to reset the cache, or pass --no-cache to skip it.
```

The results and the exit status are the same as with `--no-cache`. Nothing is read from or saved to the cache in that run.

`euphoria tags write` and `euphoria export rekordbox` stop instead, before analyzing anything, and write nothing: the cache may hold keys and tempos you [corrected](#corrections) and cues you [edited](#edited-cues), and the detected values must not replace them without your knowing. Pass `--no-cache` to go on with the detected values, or run `euphoria cache clear` to reset a damaged cache.

## euphoria cache

Manages the analysis cache.

```
euphoria cache <COMMAND>
```

| Command | Effect |
| --- | --- |
| `path` | Print the location of the cache database. |
| `clear` | Forget every cached result. Every track is analyzed again on the next run. [Corrections](#corrections) and [edited cues](#edited-cues) are kept. |

`clear` empties the database and keeps the file, so it is safe while an analysis is running: that analysis carries on, and the results it saves after the clear are kept. A database that cannot be opened, such as a damaged file, is deleted instead, and the next run creates a new one. Unlike `euphoria analyze` and `euphoria export`, both commands report a problem as an error and exit with status 1.

```
$ euphoria cache path
/Users/you/Library/Application Support/euphoria/analysis-cache.sqlite3
```

## Messages

Every command writes its results to standard output and everything else to standard error, in one shape:

| Starts with | Means |
| --- | --- |
| `error:` | Something failed: the file, if there is one, then the reason. The exit status is 1. |
| `warning:` | Something went wrong that did not stop the command, such as a cache that could not be used. |
| `hint:` | What to do next. |
| `note:` | Something worth knowing that needs no action. |

`euphoria --help` and the help of every command end with examples.

## Colour

Colour is used only for text that goes to a terminal: the key in the hue of its Camelot number, red for errors, yellow for warnings and for ratings worth a look, green for what is fine, bold for what matters and dim for what is secondary. JSON and the plain table never hold colour unless asked.

| Setting | Effect |
| --- | --- |
| `--color auto` | The default. Colour a terminal, unless `NO_COLOR` is set to any value or `CLICOLOR` is `0`, or colour everything when `CLICOLOR_FORCE` is set to anything but `0`. `NO_COLOR` wins over `CLICOLOR_FORCE`. |
| `--color always` | Colour everything, even into a pipe. |
| `--color never` | Never colour. |

`--color` can be given before or after the command, and applies to the help as well. Keys use 24 bit colour when `COLORTERM` is `truecolor` or `24bit`, and the nearest of 256 colours otherwise.
