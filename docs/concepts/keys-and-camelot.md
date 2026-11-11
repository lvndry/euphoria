---
title: Keys and the Camelot wheel
description: What a musical key is, how Camelot codes name keys, and which keys mix well together.
---

# Keys and the Camelot wheel

## Why key matters

Every track is built around a key: a home note and a scale, such as A minor or C major. When you blend two tracks, their keys either agree or clash. Tracks in compatible keys sound like they belong together; clashing keys sound out of tune during the mix, even when the tempos match.

## Camelot codes

Musical key names are hard to compare at a glance: it is not obvious that A minor and E minor mix well. The Camelot system gives each of the 24 keys a code made of a number from 1 to 12 and a letter:

- **A** for minor keys, **B** for major keys.
- The numbers go around a wheel in steps of a fifth, the most consonant interval after the octave.

| Code | Key | Code | Key |
| --- | --- | --- | --- |
| 1A | G# minor | 1B | B major |
| 2A | Eb minor | 2B | F# major |
| 3A | Bb minor | 3B | Db major |
| 4A | F minor | 4B | Ab major |
| 5A | C minor | 5B | Eb major |
| 6A | G minor | 6B | Bb major |
| 7A | D minor | 7B | F major |
| 8A | A minor | 8B | C major |
| 9A | E minor | 9B | G major |
| 10A | B minor | 10B | D major |
| 11A | F# minor | 11B | A major |
| 12A | C# minor | 12B | E major |

Euphoria shows both: the Camelot code for mixing decisions and the musical key for reference. Keys are spelled the way their key signatures are written, so the minor key on 12A is C# minor and the major key on 3B is Db major, although both have the same tonic.

## Open Key codes

Open Key is a second way to number the same wheel, used by Traktor among others. It starts at C major and A minor, so its number is the Camelot number minus seven, wrapping around the wheel, and the letter is **m** for minor keys and **d** for major keys. A minor is 8A in Camelot and 1m in Open Key; C major is 8B and 1d; G# minor is 1A and 6m.

Wherever Euphoria reads a key, it accepts a musical name (A minor), a Camelot code (8A) or an Open Key code (1m).

## Which keys mix well

From a track in a given code, these moves stay in tune:

- **Same code** (8A to 8A): the same key.
- **One number up or down, same letter** (8A to 7A or 9A): a fifth apart. The most common move.
- **Same number, other letter** (8A to 8B): the relative major or minor, which shares all its notes and changes the mood.

Euphoria looks for these three unless you ask for more.

## Bolder moves

Four more moves have a name in harmonic mixing. They change more notes, so the result is more noticeable, and most work best as a short mix. From smoothest to boldest:

| Move | From 8A (A minor) | From 8B (C major) | How it sounds |
| --- | --- | --- | --- |
| **Diagonal** | 7B | 9A | One step and the other letter. The keys share six of their seven notes, so the blend holds together while the mood shifts. |
| **Energy boost** | 10A | 10B | Two numbers up, same letter. The key rises by a whole tone, a clear lift in energy. |
| **Mood change** | 11B | 5A | The same home note in the other mode, such as A minor to A major. A dramatic change of mood. |
| **Semitone boost** | 3A | 3B | Seven numbers up, same letter. The key rises by a semitone, a dramatic lift whose melodies clash. |

The diagonal and the mood change depend on the letter you start from. From a minor key the diagonal is one number down and the mood change is three numbers up; from a major key the diagonal is one number up and the mood change is three numbers down.

The two boosts only go up. Going from 10A back to 8A is not an energy boost, and Euphoria does not suggest it.

Numbers wrap around the wheel: one step up from 12A is 1A, and an energy boost from 11B lands on 1B.

## What mixes into a track

When you ask what can follow a track, Euphoria keeps the tracks whose key makes one of the moves you allow and whose tempo is close enough, 6 percent by default. A track at half or double the tempo can count as a tempo match if you allow it. Tracks without a detected key are left out; tracks without a detected tempo are kept and listed last.

The list is ordered by move, from the same key to the boldest move you allowed, then by tempo difference, then by how close the energy is.

## Tracks off concert pitch and tracks that change key

Instruments are tuned so that the A above middle C sounds at 440 Hz. A track transferred from vinyl, or one that was sped up or slowed down, can sit between two keys of that scale. Euphoria measures how far a track is from 440 Hz in cents, hundredths of a semitone, and takes that into account when it detects the key, so the key it reports is the nearest one at 440 Hz. A track 30 cents sharp or more will sound sour against one at concert pitch even when their Camelot codes match, unless you move its pitch back.

Some tracks change key partway through. Euphoria reports one key for the track, the one that fits the whole of it, and also the key of each stretch, so you can see that the last minute of a track is in another key than its first before you plan a mix out of it.

Both are in the [JSON output](../reference/cli.md#json-output) of `euphoria analyze`.

## How sure is the detected key?

Key detection is good but not perfect, especially on tracks with little harmony, vocals alone, or a key change. See [accuracy](accuracy.md) for measured results.
