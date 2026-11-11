---
title: Use Euphoria with your DJ program
description: What Euphoria writes for rekordbox, Serato, Traktor, Engine DJ, VirtualDJ and Mixxx, what to set in each program, and how much of it is confirmed.
---

# Use Euphoria with your DJ program

Euphoria brings its keys, tempos and energy ratings into a DJ program in one of two ways:

- **Tags.** `euphoria tags write --preset <program>` writes them into the files' own tags, which the program reads when it imports a track or reloads its tags. See [write tags into your files](write-tags.md).
- **rekordbox XML.** `euphoria export rekordbox` writes a library file that rekordbox and Engine DJ can import. Your audio files are not changed. See [export to rekordbox](export-to-rekordbox.md).

No DJ software vendor publishes which tag fields its program reads. What this page says about each program comes from the program's own manual where it says anything, from the source code of programs that read the same files, from the names stored inside the rekordbox application, and from user reports. Each statement is marked:

| Mark | Meaning |
| --- | --- |
| **Confirmed** | Stated by the vendor, or visible in the program's own source code. |
| **Reported** | Stated by another vendor's guide, such as Mixed In Key's, or by user reports, or suggested by the names stored inside the program, and not contradicted. |
| **Tested** | Checked in the program itself with the test kit. |
| **Unconfirmed** | Sources disagree, or nothing was found. |

The rekordbox facts marked **Tested** were checked in rekordbox 7.2.19 on macOS 27 on 11 November 2026, with the test kit described [below](#check-it-yourself-in-rekordbox) and automatic analysis switched off. Other versions may behave differently; the kit lets you check yours. The other presets have not yet been tried in the program they are named after.

## The same in every program

Every preset writes these fields. The key is written in the notation you choose: a Camelot code (`8A`, the default), a musical name (`Am`) or an Open Key code (`1m`).

| | MP3, AIFF and WAV (ID3v2) | M4A | FLAC |
| --- | --- | --- | --- |
| Key | `TKEY` | `----:com.apple.iTunes:initialkey` and `----:com.apple.iTunes:KEY` | `INITIALKEY`, and `KEY` when the file already has one |
| Tempo | `TBPM` | `tmpo` | `BPM` |
| Label | `COMM`, such as `8A - Energy 7` in front of your comment, and in WAV also the RIFF INFO comment `ICMT` | `©cmt` | `COMMENT` |

Why these choices:

- **Two key fields in M4A.** rekordbox reads the key only from `KEY` (tested), the other programs are reported to read `initialkey`, and no program is known to misread a file that has both. Most of a typical library is M4A, so this is the field that matters most.
- **No new `KEY` in FLAC.** VirtualDJ is reported to read neither field when a FLAC file has both. When a file already has a `KEY`, left by another program, Euphoria gives it the same value, so the two never disagree.
- **Whole number tempos.** Only Mixxx is known to read a tempo with decimals. Every preset except Mixxx writes `128`, not `127.6`. A tempo with decimals belongs in the rekordbox XML, which keeps two.
- **The energy rating is in the comment.** No program has a tag field for it. The label `8A - Energy 7` has the form Mixed In Key writes, so sorting and searching on it works the same way.
- **WAV is the weakest case.** Euphoria writes an ID3 chunk into WAV files, as most tag editors do. rekordbox shows the title and comment from a WAV file's RIFF INFO list, not from its ID3 chunk (tested), so Euphoria also puts the label into the RIFF INFO comment, `ICMT`, in front of the comment already there, with every preset. RIFF INFO is the tag WAV defines for itself, and a program that does not read it ignores it. RIFF INFO has no key or tempo field, so those stay in the ID3 chunk. Serato's staff have said WAV holds no tags for Serato, and Mixed In Key does not tag WAV at all. For the key and tempo of WAV files in rekordbox, use the rekordbox XML, or convert the files to AIFF.

## rekordbox

`--preset rekordbox` writes the fields above.

What to set in rekordbox:

1. **Preferences > Analysis > Track Analysis > Track Analysis Setting**: untick **KEY**, so that rekordbox's analysis does not replace Euphoria's key. **Tested**: with KEY ticked, analyzing a track replaced the key it had been given, and the tempo too. Leave **BPM / Grid** ticked if you want rekordbox's own beat grid, knowing that its analysis then sets the tempo. This is what Mixed In Key tells its users.
2. Leave **Preferences > Analysis > Key Analysis > Write the value to the ID3 tag** off, or rekordbox writes its own key over Euphoria's. **Confirmed** by the manual.
3. For tracks already in your collection, select them, right click and choose **Reload Tag**. rekordbox reads tags when a track is added and on Reload Tag. **Confirmed** by the manual.

Reload Tag replaces information you edited inside rekordbox with what the file says, comments included. **Confirmed** by the manual.

| What | Status |
| --- | --- |
| Key from `TKEY` in MP3 (ID3v2.3 and 2.4) and AIFF | **Tested**: read when the track is added. |
| Key from M4A | **Tested**: read from `----:com.apple.iTunes:KEY`. A file with only `initialkey` shows the text in the Alphanumeric display but no key in the Classic display, so rekordbox does not take it as a key. Euphoria writes both names. |
| Key from FLAC | **Tested**: read from `INITIALKEY`. A file with only `KEY` shows no key. |
| Title and comment from WAV | **Tested**: read from RIFF INFO, not from the ID3 chunk. Euphoria writes the label into the RIFF INFO comment too. That rekordbox shows a comment written by Euphoria is **unconfirmed**: test T11 of the kit checks it. |
| Tempo from `TBPM`, `tmpo` and FLAC `BPM` | **Tested**: not read when a track is added; the BPM column stays empty until rekordbox analyzes the track. Use the rekordbox XML to hand rekordbox a tempo. |
| `8A`, `Am` and `1m` in a key tag | **Tested**: all three are recognised as keys and shown in the display format you chose. **Display key information on the database** shows the text as written. |
| Comment from `COMM`, M4A `©cmt`, FLAC `COMMENT` and AIFF | **Tested**: read when the track is added. |

The rekordbox XML is the route Pioneer documents for handing rekordbox a key, a tempo with decimals and a comment, for any format, and the only one that brings in a tempo. **Tested**: a new track imported from the XML gets its key, its tempo with decimals and its comment. For a track already in your collection, importing only a playlist changes nothing, while **Import To Collection** on the track replaces its key, tempo and comment. [Export to rekordbox](export-to-rekordbox.md) has the steps.

`euphoria export rekordbox --beat-grid` also writes a beat grid for each track whose grid is rated medium or high, so rekordbox needs no grid analysis of its own for it. **Unconfirmed**: Import To Collection on a track already in your collection is expected to replace a grid or cue points you set by hand there with what the file holds; the test covered key, tempo and comment, not grids or cues. Try it on a few tracks first. `--cues` adds cue points at the sections of each track, and leaves tracks that already have cues in your collection out of the file entirely, reading them from a collection you export from rekordbox; see [export to rekordbox](export-to-rekordbox.md#cue-points).

### Check it yourself in rekordbox

The repository has a script that builds a test kit: a few short clips, each tagged one way, a rekordbox XML file, and a ten minute checklist with a table to fill in. It answers the questions above for your rekordbox version.

```
cargo build --release -p euphoria-cli
uv run bench/rekordbox_test_kit.py --clips bench/data/key/audio --output /tmp/euphoria-kit
```

It needs [uv](https://docs.astral.sh/uv/), ffmpeg, a release build of Euphoria, which tags one of the WAV files, and the benchmark clips from `./bench/download_datasets.sh`. It reads the clips and writes only into the new folder. Back up your rekordbox library before you follow the checklist; it ends with how to remove the test tracks again.

## Serato DJ

`--preset serato` writes the fields above, except the tempo, unless you add `--tempo`. Serato does not run its own tempo detection on a file that already has a tempo (**confirmed** by Serato), so a tempo written by Euphoria would stand in for Serato's own and Serato would build no beat grid from its analysis.

What to set in Serato:

1. In the analysis options, untick **Set Key** before analysing, so Serato keeps Euphoria's key. Leave **Set Beatgrid / BPM** ticked. **Confirmed** by Serato.
2. Choose how keys are shown in **Setup > Library + Display**: Camelot, Open Key, musical, or **Original Tag**, which shows the text exactly as written. **Confirmed** by Serato.
3. For tracks already in your library, use **Rescan ID3 Tags** in the **Files** panel. **Confirmed** by Serato.

| What | Status |
| --- | --- |
| Key from the ID3 key field in MP3 | **Confirmed** by Serato's staff, who do not name the frame; `TKEY` is the frame the ID3 standard has for it. |
| Comment shows the `8A - Energy 7` label | **Reported**: Mixed In Key's guide shows its label in Serato's Comment column. |
| Key from M4A (`initialkey`) | **Unconfirmed**: stated in a comment in Mixxx's source code only. |
| Key from FLAC | **Unconfirmed**, with old reports of FLAC key bugs. |
| Key and tempo from WAV | Not read: Serato's staff said WAV holds no tags for Serato. |
| Whether a key written as `8A` or `1m` is converted for display and Key Sync | **Unconfirmed**. **Original Tag** always shows it. |

## Traktor

`--preset traktor` writes the fields above. With `--key-notation open-key` the key is written as Traktor writes its own, such as `1m`.

The key Euphoria writes appears in Traktor's **Key Text** column, not in the **Key** column, which holds only Traktor's own analysis. Traktor shows the text unchanged, so a Camelot `8A` stays `8A`. **Reported** by Mixed In Key's guide and by users.

What to set in Traktor:

1. **Preferences > File Management**: set tag writing to write no tags, or only Traktor's own tags. With all tags written, Traktor's analysis can replace the key field with its own notation. **Confirmed** that the setting exists; the replacement is reported by users.
2. When you import tracks, untick BPM and Key in the import dialog's analysis options, or Traktor may rewrite the key and tempo fields of the files. **Unconfirmed**: one detailed user report for Traktor Pro 4.1.
3. For tracks already in your collection, select them and run **Check Consistency**, as Mixed In Key advises. Users report that in Traktor Pro 4 this no longer refreshes every field. **Unconfirmed**.

| What | Status |
| --- | --- |
| Key Text from `TKEY` in MP3 | **Reported** |
| Comment from `COMM` in MP3 | **Reported**: a real Traktor collection file has the comment, though not where it came from |
| Key Text from M4A and FLAC | **Unconfirmed** |
| Tempo from tags when Traktor's analysis is off | **Unconfirmed** |

Euphoria does not write Traktor's `collection.nml`. Its key numbering, for anyone who builds on it: `MUSICAL_KEY VALUE` 0 to 11 are the major keys from C upwards by semitone, and 12 to 23 the minor keys from C minor, checked against a real Traktor Pro 4 collection. Mixxx's Traktor reader is one semitone off from this.

## Engine DJ

`--preset engine-dj` writes the fields above.

What to set in Engine DJ:

1. After writing tags, right click the tracks and choose **Re-import track information**. **Confirmed** by the Engine DJ user guide.
2. Do not choose **Re-analyze** for those tracks: the guide says re-analysis uses Engine's own BPM and key detection regardless of the tags. **Confirmed**.
3. To bring in a tempo with decimals, use the rekordbox XML: Engine DJ's Import Assistant reads a rekordbox XML file. A track that was imported before is not updated by a second import; remove it from the Engine collection and import it again. **Confirmed** by the guide, and users report the same.

| What | Status |
| --- | --- |
| Key from MP3 tags | **Reported**: users see Mixed In Key's keys. The frame name is **unconfirmed**. |
| Whether the first analysis keeps a tagged key | **Unconfirmed**: users disagree. |
| Key from FLAC | **Unconfirmed**: reported missing in Engine 2.2.2. |
| Key from M4A, AIFF and WAV | **Unconfirmed** |
| Whether the key, tempo and comment of a rekordbox XML are imported, or the file's tags | **Unconfirmed** |

## VirtualDJ

`--preset virtualdj` writes the fields above.

What to set in VirtualDJ (in **Options**):

1. Set `useKeyFromTag` to yes, so the key in the tag is preferred over VirtualDJ's own. **Confirmed** by the options list.
2. Consider setting `setTagsAuto` to no. Users report automatic tag writing replacing keys written by other programs. **Unconfirmed**.
3. `getCommentFromTags` controls whether the comment, and with it the energy label, is read. **Confirmed**.

VirtualDJ reads a tempo tag when it first meets a file, then replaces it with its own analysis, and has no option to prefer the tag (**reported** by VirtualDJ's staff and users). Writing the tempo does no harm, but it does not last.

| What | Status |
| --- | --- |
| Key from FLAC `INITIALKEY` and `KEY` | **Confirmed** by VirtualDJ's CTO, who said both are read and `KEY` wins when both exist. One user reports neither being read when both exist, which is why Euphoria never adds a `KEY` and keeps an existing one equal. |
| Key from MP3 | **Reported**: users see Mixed In Key's keys. The frame name is **unconfirmed**. |
| Key from M4A, WAV and AIFF | **Unconfirmed** |

## Mixxx

`--preset mixxx` writes the fields above, keeps up to two decimals in the tempo, and adds `----:com.apple.iTunes:BPM` with decimals in M4A next to `tmpo`, which can hold only whole numbers. Everything in this section is **confirmed** in Mixxx's source code.

- Mixxx reads every field Euphoria writes, both key names in M4A and FLAC, Camelot, Open Key and musical notation, and Camelot with a leading zero.
- A track that already has a key is not analysed for its key again.
- Mixxx does not notice tag changes made by another program on a rescan. Select the tracks and choose **Metadata > Import From File Tags**.
- Mixxx does not read tags from WAV files beyond RIFF INFO, which has no key or tempo.

## Generic

`--preset generic`, the default, writes the fields above with a whole number tempo, and in M4A adds `----:com.apple.iTunes:BPM` next to `tmpo`. It is the choice when you use several programs.

## Sources

The research behind this page lists every source for each statement: the rekordbox 7 manual and Pioneer's XML format description, the strings stored in rekordbox 7.2.19, the Serato, Native Instruments, Engine DJ and VirtualDJ documentation, the source code of Mixxx, libdjinterop, pyrekordbox and dj-data-converter, Mixed In Key's integration guides, and user reports on each vendor's forum. Where those sources disagree, this page says so.
