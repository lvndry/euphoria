# Audio fixtures

Each file is one second of a 440 Hz sine wave at 44.1 kHz with an amplitude
of 0.125, generated with ffmpeg's `sine` source. The integration tests in
`tests/decode.rs` check their sample rate, duration and loudness.

| File | Format | Channels |
| --- | --- | --- |
| `sine-440hz-mono.wav` | WAV, 16-bit PCM | mono |
| `sine-440hz-stereo.wav` | WAV, 8-bit PCM | stereo, both channels equal |
| `sine-440hz-mono.flac` | FLAC | mono |
| `sine-440hz-mono.mp3` | MP3, 64 kbit/s | mono |
| `sine-440hz-mono.m4a` | AAC in MP4, 64 kbit/s | mono |
| `sine-440hz-mono.mp4` | AAC in MP4 with the `.mp4` extension, 64 kbit/s | mono |
| `sine-440hz-mono-alac.m4a` | ALAC in MP4, 16-bit | mono |
| `sine-440hz-mono.ogg` | Ogg Vorbis from the reference encoder, quality 0 | mono |
| `sine-440hz-mono.aiff` | AIFF, 8-bit PCM | mono |
| `sine-440hz-opposite-phase-stereo.flac` | FLAC | stereo, right channel is the negated left channel |

The opposite-phase file mixes down to silence, which shows that the decoder
averages channels. The stereo WAV and the AIFF use 8-bit samples, and the
opposite-phase file uses FLAC, to keep the fixtures under 300 KB in total.
`../quality-fixtures` holds files that prove a quality-report line end to
end, such as a 24-bit ALAC stream for the cookie parser. They live outside
this folder too, so that tests that scan it keep finding the ten files.

## Tagged fixtures

`../tagged-fixtures` holds one file per format with the title "Low Tide" and the artist "Saffron & Co", for the tests of tag reading in `tests/tags.rs`. Each is a quarter of a second of the same tone at 8 kHz. They live outside this folder so that tests that scan it keep finding the ten files above.

Run these commands from `../tagged-fixtures` to regenerate them:
Run these commands from `../tagged-fixtures` to regenerate them:

```sh
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=8000:duration=0.25" -ac 1 -c:a libmp3lame -b:a 16k -metadata "title=Low Tide" -metadata "artist=Saffron & Co" tagged.mp3
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=8000:duration=0.25" -ac 1 -c:a aac -b:a 16k -metadata "title=Low Tide" -metadata "artist=Saffron & Co" tagged.m4a
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=8000:duration=0.25" -ac 1 -c:a flac -metadata "title=Low Tide" -metadata "artist=Saffron & Co" tagged.flac
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=8000:duration=0.25" -ac 1 -c:a pcm_u8 -metadata "title=Low Tide" -metadata "artist=Saffron & Co" tagged.wav
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=8000:duration=0.25" -ac 1 -c:a pcm_s8 -write_id3v2 1 -metadata "title=Low Tide" -metadata "artist=Saffron & Co" tagged.aiff
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=8000:duration=0.25" -ac 1 -c:a alac -metadata "title=Low Tide" -metadata "artist=Saffron & Co" tagged-alac.m4a
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=8000:duration=0.25" -ac 1 -c:a aac -b:a 16k -metadata "title=Low Tide" -metadata "artist=Saffron & Co" tagged.mp4
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=8000:duration=0.25" -ac 1 -c:a pcm_s16le tagged-source.wav
oggenc -q 0 -t "Low Tide" -a "Saffron & Co" -o tagged.ogg tagged-source.wav && rm tagged-source.wav
```

The Ogg Vorbis files are made with `oggenc` from vorbis-tools, the reference encoder, because FFmpeg's own Vorbis encoder is experimental. `oggenc` puts the comment and setup headers on one page, as most Ogg Vorbis files have them; `tests/tags_ogg.rs` builds the other layouts itself.

## Regenerating

Run these commands from this directory:

```sh
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -ac 1 -c:a pcm_s16le sine-440hz-mono.wav
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -af "pan=stereo|c0=c0|c1=c0" -c:a pcm_u8 sine-440hz-stereo.wav
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -ac 1 -c:a flac sine-440hz-mono.flac
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -ac 1 -c:a libmp3lame -b:a 64k sine-440hz-mono.mp3
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -ac 1 -c:a aac -b:a 64k sine-440hz-mono.m4a
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -ac 1 -c:a pcm_s8 sine-440hz-mono.aiff
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -af "pan=stereo|c0=c0|c1=-1*c0" -c:a flac sine-440hz-opposite-phase-stereo.flac
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -ac 1 -c:a aac -b:a 64k sine-440hz-mono.mp4
ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -ac 1 -c:a alac sine-440hz-mono-alac.m4a
oggenc -q 0 -o sine-440hz-mono.ogg sine-440hz-mono.wav
```

## Engine DJ library schemas

`engine/engine-desktop-<version>-m.db.sql` are the schemas of the empty `Database2/m.db` that Engine DJ Desktop 2.0.0 (schema 2.18.0), 4.0.1 (2.21.2), 4.3.0 (3.0.1) and 5.0.0 (3.0.2) create, as SQL dumps. They are copied unchanged from libdjinterop (<https://github.com/xsco/libdjinterop>, `testdata/ref/engine/desktop/`, commit `d61727c5`), which is licensed under the LGPL 3.0 or later. `tests/engine_export.rs` builds a library from each of them.
