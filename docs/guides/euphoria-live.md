---
title: Euphoria Live
description: See the key and tempo of whatever is playing on your Mac, from rekordbox, Spotify or a browser, as it plays, with nothing recorded.
---

# Euphoria Live

Euphoria Live listens to the audio your Mac is playing and shows its key and tempo as it plays. It works with any source: rekordbox, Spotify, YouTube in a browser, a turntable through an audio interface that macOS plays out.

```
euphoria live
```

## Privacy

**The audio never leaves memory.** Live keeps the last 25 seconds or so of audio in a buffer in memory, reads the key, tempo and energy from it every two seconds, and overwrites the oldest audio as new audio arrives. Nothing is written to disk, nothing is sent anywhere, and the buffer is emptied when you pause, when listening stops and after two seconds of silence. Euphoria does not listen to its own playback.

macOS shows its orange microphone-style indicator while an app listens, and Live adds its own: in the app a clear "Listening" mark whenever it is on, and in the terminal a line saying what it listens to.

## What macOS asks

Live uses a Core Audio *process tap*, which macOS 14.2 (December 2023) added to let an app hear what other apps play. The first time you start listening, macOS asks whether Euphoria may hear the audio of other applications. The text reads "Euphoria Live listens to the audio playing on this Mac to show its key and tempo. The audio stays in memory and is never recorded, saved or sent anywhere." Allow it once; macOS remembers the answer, and you can change it under System Settings, Privacy & Security, Screen & System Audio Recording.

If you refuse, macOS does not report an error: the tap delivers silence. Live then shows "Listening…" and, after ten seconds of exact silence, a hint to check that setting.

Before macOS 14.2 Live says it needs macOS 14.2. The alternative, ScreenCaptureKit, needs the Screen Recording permission, which macOS describes as recording the screen, and its audio capture needs a screen to be shared. That prompt is a poor fit for a tool that never touches the screen, so Euphoria asks for a newer macOS instead.

Linux is not supported yet. A PipeWire or PulseAudio monitor source would be the equivalent and can use the same analysis.

## From the terminal

`euphoria live` prints one line per update once enough audio has played:

```
$ euphoria live
Listening to All audio. Nothing is recorded; press Ctrl-C to stop.
listening…
8A  A minor  124.0 BPM  high
8A  A minor  124.0 BPM  high
```

The key appears after about 8 seconds of audio and the tempo after about 6. The confidence is that of the key (`low`, `medium` or `high`, as in [the analysis](../concepts/confidence.md)). When the audio goes silent or too quiet, `listening…` is printed once, and values are never repeated from before. Ctrl-C stops it.

| Option | Meaning |
| --- | --- |
| `--source <name>` | Listen to one application, by the start of its name, such as `rekordbox`. It must be playing audio. Without it, everything the Mac plays is heard except Euphoria. |
| `--interval <seconds>` | Seconds between two readings, 0.5 or more. The default is 2. |
| `--json` | One JSON object per line, for scripts and stream overlays. |

With `--json`, a line is `{"listening":true}` or an object with `source`, `key` (`camelot`, `name`, `confidence`, or `null`), `bpm`, `bpm_confidence`, `energy` and `level_db`.

macOS asks for permission for the program that runs `euphoria`, so from a terminal the prompt names your terminal app.

## How it reads the audio

- **Key:** the drone-v2 network, on the last 25 seconds, every two seconds, with its confidence.
- **Tempo:** the tempo detector on the last 12 seconds, folded into 90 to 180 BPM.
- **Energy:** the energy rating of the last 12 seconds, on the scale of [energy](../concepts/energy.md).
- **Silence:** audio below about -50 dBFS counts as silence. Nothing is shown then, and after two seconds of it the buffer is emptied, so a new track is read on its own and no old value is shown.

Measured on an Apple silicon Mac with a build of the `fast` profile, which is optimised like a release: listening costs about 5 percent of one core at a two second interval, with the buffer full, and about 65 to 90 MB of memory.

## Limits

- **A mix of two tracks gives a blended key.** While two tracks play together, the key of the 25 seconds is that of the blend, with a lower confidence, and the tempo may sit between the two. Wait for the transition to finish.
- **A new track takes about 8 seconds** to show a key, and a key change takes longer than that to show, because the key is read from 25 seconds.
- **Not tested with every source.** It is tested here with generated tones and kick patterns, and by hand against music playing on the Mac that built it. Sources that play through another device than the default output are not tested.
- **Process names are those macOS knows.** A browser plays audio from a helper process, so `--source` must name that process.
