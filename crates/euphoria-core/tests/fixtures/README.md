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
| `sine-440hz-mono.aiff` | AIFF, 8-bit PCM | mono |
| `sine-440hz-opposite-phase-stereo.flac` | FLAC | stereo, right channel is the negated left channel |

The opposite-phase file mixes down to silence, which shows that the decoder
averages channels. The stereo WAV and the AIFF use 8-bit samples, and the
opposite-phase file uses FLAC, to keep the fixtures under 300 KB in total.

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
```
