---
title: Supported formats
description: Audio formats Euphoria can read.
---

# Supported formats

| Format | Extensions | Notes |
| --- | --- | --- |
| MP3 | `.mp3` | |
| AAC in MP4 | `.m4a` | AAC-LC. HE-AAC files cannot be decoded yet. |
| FLAC | `.flac` | |
| WAV | `.wav` | |
| AIFF | `.aif`, `.aiff` | |

Extensions are matched in any letter case. When searching a folder, Euphoria only picks up files with these extensions. A file named directly on the command line is always tried, and reported as a failure if it cannot be read.

Stereo and multichannel files are mixed down to mono before analysis.

Sample rates from 8 kHz to 384 kHz are supported. A file that declares a rate outside that range, which only a damaged file does, is reported as a failure.
