---
title: Supported formats
description: Audio formats Euphoria can read.
---

# Supported formats

| Format | Extensions | Notes |
| --- | --- | --- |
| MP3 | `.mp3` | |
| AAC in MP4 | `.m4a`, `.mp4` | AAC-LC. HE-AAC and HE-AACv2 files are refused with an error that names the profile; they cannot be decoded yet. |
| ALAC (Apple Lossless) in MP4 | `.m4a`, `.mp4` | |
| FLAC | `.flac` | |
| Ogg Vorbis | `.ogg` | Opus in Ogg cannot be decoded yet. Tags are written in Ogg files that hold one Vorbis stream; a file with a second stream, such as one chained after the first, is refused. |
| WAV | `.wav` | |
| AIFF | `.aif`, `.aiff` | |

Reading or planning edits of Ogg tags stops with an error if the three Vorbis headers occupy more than 128 MiB or 4096 Ogg pages, or contain more than 65,536 comments. These limits allow large embedded artwork while bounding memory for unterminated packets, empty header pages and per-comment allocations. Audio pages are not included in these limits.

Extensions are matched in any letter case. When searching a folder, Euphoria only picks up files with these extensions. A file named directly on the command line is always tried, and reported as a failure if it cannot be read.

Audio files of other formats found in a folder are not skipped silently. Files ending in `.opus`, `.wma`, `.wv`, `.ape`, `.dsf`, `.dff`, `.mpc`, `.tta`, `.aac`, `.ac3`, `.dts`, `.amr`, `.mka`, `.mp2`, `.oga`, `.webm` or `.caf` are counted by extension and reported, in the command line output as `14 files skipped: 9 .opus, 5 .wma, which Euphoria can't decode yet`, in the JSON as entries with `"skipped": true`, and in the desktop app as a notice after adding the folder. Files that are not audio, such as pictures and playlists, are not counted. HE-AAC cannot be told from AAC-LC by its extension: it is refused when the file is opened, with an error naming the profile. A file whose AAC stream does not declare its profile can only be told by decoding it. This detection of HE-AAC is covered by a unit test of the profile check; it was not tried on a real HE-AAC file.

Stereo files are mixed down to mono before analysis by averaging the two channels. Files with more channels use only the first two, the front left and right, so the centre, the bass effects channel and the surrounds do not colour the key and tempo. A stereo file whose channels cancel each other, as when one is the other with its phase inverted, would average to silence, so for those stretches the left channel is used instead and the [quality report](../concepts/audio-quality.md#channels-that-cancel) says so.

An `.mp4` file is read for its audio; a video track in it is ignored. Euphoria tells AAC from ALAC by the file's contents, not its extension, and the [quality report](../concepts/audio-quality.md) names the codec.

Every format here can also have its tags written: see [tags](tags.md). Not every DJ program plays every format. rekordbox, for one, plays AAC in `.m4a` and `.mp4` and ALAC in `.m4a`, and does not list Ogg Vorbis among the formats it plays; [use Euphoria with your DJ program](../guides/dj-programs.md#formats-each-program-plays) has the details.

Sample rates from 8 kHz to 384 kHz are supported. A file that declares a rate outside that range, which only a damaged file does, is reported as a failure.
