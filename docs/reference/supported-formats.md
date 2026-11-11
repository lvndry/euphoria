---
title: Supported formats
description: Audio formats Euphoria can read.
---

# Supported formats

| Format | Extensions | Notes |
| --- | --- | --- |
| MP3 | `.mp3` | |
| AAC in MP4 | `.m4a`, `.mp4` | AAC-LC. HE-AAC files cannot be decoded yet. |
| ALAC (Apple Lossless) in MP4 | `.m4a`, `.mp4` | |
| FLAC | `.flac` | |
| Ogg Vorbis | `.ogg` | Opus in Ogg cannot be decoded yet. Tags are read and written in Ogg files that hold one Vorbis stream. |
| WAV | `.wav` | |
| AIFF | `.aif`, `.aiff` | |

Extensions are matched in any letter case. When searching a folder, Euphoria only picks up files with these extensions. A file named directly on the command line is always tried, and reported as a failure if it cannot be read.

Stereo and multichannel files are mixed down to mono before analysis.

An `.mp4` file is read for its audio; a video track in it is ignored. Euphoria tells AAC from ALAC by the file's contents, not its extension, and the [quality report](../concepts/audio-quality.md) names the codec.

Every format here can also have its tags written: see [tags](tags.md). Not every DJ program plays every format. rekordbox, for one, plays AAC in `.m4a` and `.mp4` and ALAC in `.m4a`, and does not list Ogg Vorbis among the formats it plays; [use Euphoria with your DJ program](../guides/dj-programs.md#formats-each-program-plays) has the details.

Sample rates from 8 kHz to 384 kHz are supported. A file that declares a rate outside that range, which only a damaged file does, is reported as a failure.
