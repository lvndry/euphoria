---
title: Euphoria Live
description: See the key and tempo of whatever is playing on your Mac, from rekordbox, Spotify or a browser, as it plays, with nothing recorded.
---

# Euphoria Live

Euphoria Live listens to the audio your Mac is playing and shows its key and tempo as it plays. It works with any source: rekordbox, Spotify, YouTube in a browser, a turntable through an audio interface that macOS plays out.

![The Live panel hearing a C major pad over a kick at 126 BPM, with the tracks of a demo library that mix in](../images/live/live-panel.png)

## Open it

- **In the app:** the **Live** button in the toolbar, **View, Live** (Shift Command L), or Command K and "Open Euphoria Live".
- **In the menu bar:** once Live has been started, Euphoria keeps an item in the macOS menu bar. Its title is the key heard, such as `8B`, or "Live" while it has nothing to show, and "Live paused" when paused. Click it to show or hide the Live panel. Right click it, or Control click, for a menu: Show Live, Pause listening, Keep Live running when the window is closed, Open Euphoria and Quit Euphoria.
- **Without the library window:** `open -a Euphoria --args --live` starts Euphoria with only the menu bar item and the panel, and no Dock icon.
- **In a terminal:** `euphoria live`, below.

Opening Live starts listening. The first time, macOS asks for permission, described below.

## The panel

The panel is a small window that stays on top of other windows, so it can sit beside rekordbox.

- **Listening** with a red dot, and "Nothing is recorded", whenever Euphoria is listening. When paused the dot is grey and the panel says "Not listening".
- **Listen to** chooses what is heard: *All audio*, or one of the applications playing audio right now, such as rekordbox. The list is read when you open it. Euphoria's own playback is never heard.
- **The key**, big, in the notation you chose in the settings, with its musical name and its confidence: high, medium or low. Below it the **BPM**, the **energy** from 1 to 10, and a level meter.
- **Next** lists the tracks of your library that mix in after what is playing, with the same ranking, the same Keep, Lift and Drop choice, the same pills and the same filters of the suggestion pool as the "Next after" list of the main window. It ranks from the key and tempo heard, so it works for Spotify or YouTube too; no library track needs to match. Tracks the cache has no analysis for are left out.
- **Pause listening** stops the capture and forgets the audio; **Start listening** resumes.
- **Keep Live running when the window is closed**, **Open Euphoria** and **Quit Euphoria** are at the bottom.

When the audio is silent or too quiet, or just started, the panel says "Listening…" and shows no key, no tempo and no list: nothing is guessed or kept from before.

## Part of the app, in the menu bar

Live is part of Euphoria, not a second app. Close the library window and Live keeps running in the menu bar, with its panel if you left it open, so you can close the library and keep Live beside rekordbox. **Open Euphoria** in the panel or the menu bar menu opens the library window again.

**Keep Live running when the window is closed** is on by default, and counts once Live has been started. Turn it off and closing the library window quits Euphoria, as before. It is kept in a file of its own, `live.json`, beside the settings. The panel on its own never keeps Euphoria open: only the menu bar item does.

While only Live runs, no library table, waveform or analysis worker exists. The library is read from the analysis cache the first time the Next list is shown, once, and nothing is analyzed. Measured on an Apple silicon Mac, with a build of the `fast` profile, listening to a generated track with the library window closed: about 6 percent of one core while analyzing, near zero while the audio is silent, and about 140 MB of memory with the panel closed and about 170 MB with it open and the library of 40 tracks read. Most of that memory is the window system, not the analysis: `euphoria live` in a terminal needs 65 to 90 MB.

## Privacy

**The audio never leaves memory.** Live keeps the last 25 seconds or so of audio in a buffer in memory, reads the key, tempo and energy from it every two seconds, and overwrites the oldest audio as new audio arrives. Nothing is written to disk, nothing is sent anywhere, and the buffer is emptied when you pause, when listening stops and after two seconds of silence. Euphoria does not listen to its own playback.

macOS shows its own indicator while an app hears system audio, and Live adds its own: the red "Listening" dot in the panel and the key in the menu bar whenever it is on, and in the terminal a line saying what it listens to.

## What macOS asks

Live uses a Core Audio *process tap*, which macOS 14.2 (December 2023) added to let an app hear what other apps play. The first time you start listening, macOS asks whether Euphoria may hear the audio of other applications. The text reads "Euphoria Live listens to the audio playing on this Mac to show its key and tempo. The audio stays in memory and is never recorded, saved or sent anywhere." Allow it once; macOS remembers the answer, and you can change it under System Settings, Privacy & Security, Screen & System Audio Recording.

If you refuse, macOS does not report an error: the tap delivers silence. Live then shows "Listening…" and, after ten seconds of exact silence, a hint to check that setting.

Before macOS 14.2 Live says it needs macOS 14.2. The alternative, ScreenCaptureKit, needs the Screen Recording permission, which macOS describes as recording the screen, and its audio capture needs a screen to be shared. That prompt is a poor fit for a tool that never touches the screen, so Euphoria asks for a newer macOS instead.

Linux is not supported yet. A PipeWire or PulseAudio monitor source would be the equivalent and can use the same analysis.

## From the terminal

```
euphoria live
```

`euphoria live` prints one line per update once enough audio has played:

```
$ euphoria live
Listening to All audio. Nothing is recorded; press Ctrl-C to stop.
listening…
8A  A minor  124.0 BPM  high
8A  A minor  124.0 BPM  high
```

The key appears after about 8 seconds of audio and the tempo after about 6. The confidence is that of the key (`low`, `medium` or `high`, as in [the analysis](../concepts/accuracy.md)). When the audio goes silent or too quiet, `listening…` is printed once, and values are never repeated from before. Ctrl-C stops it.

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
- **Not tested with every source.** It is tested with generated tones and kick patterns, with a generated track played by `afplay` and heard through the process tap (126 BPM and C major came back right, with the tap read at the output device's rate), and by hand against music playing on the Mac that built it. Sources that play through another device than the default output, Bluetooth outputs and rekordbox itself are not tested.
- **Process names are those macOS knows.** A browser plays audio from a helper process, so `--source` must name that process.
