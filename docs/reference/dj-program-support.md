---
title: DJ program support
description: What each DJ program receives from Euphoria, by which route, and whether that route was tested in the program itself.
---

# DJ program support

Euphoria hands its analysis to a DJ program in one of three ways: a library file the program imports, the tags of the audio files, or the program's own data in the files. This page lists what each program receives. [Use Euphoria with your DJ program](../guides/dj-programs.md) says what to set in each program.

**Tested in the program** means someone opened the result in that program and checked it. Only rekordbox is installed on the machine Euphoria is developed on, so every other route is checked by Euphoria's own tests only: they write the data, read it back and compare, and check it against the format's published or reverse engineered description and against real files where those are available.

## By program

| Program | Route | Key | Tempo | Beat grid | Cue points | Energy | Tested in the program |
| --- | --- | --- | --- | --- | --- | --- | --- |
| rekordbox | [rekordbox XML](../guides/export-to-rekordbox.md), `euphoria export rekordbox` | Yes | Yes, two decimals | Yes, `--beat-grid` | Yes, memory and hot cues A to H, `--cues` | In the comment | **Yes** for key, tempo and comment (rekordbox 7.2.19). **No** for beat grids and cues |
| rekordbox | [Tags](../guides/write-tags.md), `euphoria tags write --preset rekordbox` | Yes | Not read by rekordbox | No | No | In the comment | **Yes** (rekordbox 7.2.19) |
| Traktor Pro | [Traktor NML](../guides/export-to-traktor.md), `euphoria export traktor` | Yes, Key and Key Text columns | Yes, six decimals | Yes, `--beat-grid` | Yes, hot cues 1 to 8, `--cues` | In the comment | **No** |
| Traktor Pro | Tags, `--preset traktor` | Key Text column only | Unconfirmed | No | No | In the comment | **No** |
| Serato DJ | Tags, `--preset serato`, with [`--serato-markers`](../guides/serato-cues.md) for cues and grid | Yes | Only with `--tempo` | With `--serato-markers` | With `--serato-markers`, hot cues 1 to 8, merged with yours | In the comment | **No** |
| Engine DJ | Tags, `--preset engine-dj` | Yes | Unconfirmed | No | No | In the comment | **No** |
| Engine DJ | rekordbox XML, through Engine's Import Assistant | Unconfirmed | Unconfirmed | Yes, by Engine's guide | Hot cues, by Engine's guide | Unconfirmed | **No** |
| VirtualDJ | Tags, `--preset virtualdj` | Yes, with `useKeyFromTag` | Replaced by VirtualDJ's analysis | No | No | In the comment | **No** |
| Mixxx | Tags, `--preset mixxx` | Yes | Yes, with decimals | No | No | In the comment | **No** |
| Any program | [M3U8 playlist](cli.md#euphoria-export-m3u8), `euphoria export m3u8` | No | No | No | No | No | Not needed: a list of paths |

## By piece of information

| Information | rekordbox XML | Traktor NML | Tags |
| --- | --- | --- | --- |
| Key | `Tonality`, Camelot or musical | `INFO KEY` in any notation, and `MUSICAL_KEY` 0 to 23 | `TKEY`, `initialkey` and `KEY`, `INITIALKEY` |
| Tempo | `AverageBpm`, two decimals | `TEMPO BPM`, six decimals | `TBPM`, `tmpo`, `BPM`, whole numbers except for Mixxx |
| Beat grid | One `TEMPO` at the first downbeat | One grid marker at the first downbeat | None |
| Cue points | `POSITION_MARK`, memory cue and hot cue for each | `CUE_V2` hot cues | None |
| Energy | `Comments` | `INFO COMMENT` | The comment field |
| Your cues | Tracks with cues in the collection given with `--collection` are left out | Same, with your `collection.nml` | Not touched |
