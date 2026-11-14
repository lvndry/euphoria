---
title: Audio quality
description: How Euphoria judges whether a file sounds good enough for a DJ set, what it measures, where the thresholds come from, and what it cannot tell.
---

# Audio quality

Euphoria gives every track a quality verdict for DJ use, with a one sentence reason:

| Verdict | What it means |
| --- | --- |
| Lossless | A lossless file (FLAC, ALAC, WAV, AIFF) with a full spectrum. Nothing better exists. |
| Good | Lossy audio at a high bit rate, such as 320 kbit/s MP3 or 256 kbit/s AAC, with the spectrum that bit rate keeps. Fine on any system. |
| Acceptable | Lossy audio at a medium bit rate, around 160 to 192 kbit/s. Fine in most rooms; a 320 kbit/s or lossless copy holds more detail. |
| Weak on a big system | Lossy audio at a low bit rate, such as 128 kbit/s. Fine for practice and small rooms; on a club system the top end and the detail suffer. |
| Likely re-encoded | The file's bit rate promises more than its content holds: its sound stops where a lower quality file's would, so it was most likely made from one. A 320 kbit/s file made from a 128 kbit/s one sounds like the 128 kbit/s one. |

The verdict is about the file, not the music. A great track in a weak file is still a great track: the verdict tells you which files are worth replacing with a better copy before you play them loud.

## What is measured

| Measurement | What it is |
| --- | --- |
| Codec | The audio codec, such as MP3, AAC, Vorbis, FLAC, ALAC or PCM, and whether it is lossless. |
| Bit rate | The average bit rate of the audio data, measured from the size of every audio packet and the length of the audio. Tags and artwork are not counted. For a variable bit rate file this is the real average, which can be well below the setting it was encoded with. |
| Sample rate, bit depth, channels | As the file declares them. Bit depth only exists for lossless files. |
| Cutoff | The frequency where the content stops. See below. |
| Clipping | The share of samples that lie in runs of three or more at full scale, in any channel. |
| Loudness | Integrated loudness in LUFS, measured as EBU R128 describes, on the mono mixdown. |
| True peak | The highest peak between samples in any channel, in dB relative to full scale, estimated by interpolating four times between samples near the loudest ones. |

Everything is measured during the existing decode for key, tempo and energy, with its loudness measurement reused. The cutoff processes each channel separately. [Benchmark results](https://github.com/lvndry/euphoria/blob/main/bench/results/quality.md) record the measured CPU cost.

### The cutoff

Lossy encoders save space by removing the highest frequencies, above a cutoff that depends on the bit rate: around 16 kHz for 128 kbit/s MP3, around 20 kHz for 320 kbit/s. Above the cutoff the spectrum falls by 30 to 60 dB within a few hundred hertz. Music on its own never falls that steeply, even when it is dark: a muffled track loses its highs gradually.

Euphoria measures each channel separately and combines their spectral energy across the whole track. This keeps opposite-phase stereo content from cancelling as it would in a mono mixdown. It looks for the steepest fall above 8 kHz between two bands 1 kHz wide. A fall of at least 20 dB is the cutoff; anything less means the content runs to the top of the spectrum.

Re-encoding cannot bring back what was removed. A 128 kbit/s file turned into 320 kbit/s keeps its cutoff at 16 kHz, which is how it gives itself away.

![Range bars of where the content stops for each encoder and bit rate. Encoded once from a lossless source, every file stops at or above what its bit rate keeps, from about 15 kHz at 96 kbit/s to 20 kHz at 320 kbit/s MP3. Transcoded to a higher bit rate or to FLAC, the files still stop where their low bit rate source did, between 16 and 19 kHz, below what their new bit rate keeps.](../images/audio-quality/cutoffs.svg)

## Where the thresholds come from

The thresholds were set by measurement, not from received wisdom about bit rates. `bench/quality_benchmark.py` took six lossless tracks of electronic music published under Creative Commons (from the album *Lithium Toolbox*, Argali Records, CC BY-SA 3.0) and three synthesized ones (a wideband dance loop, a dark version of it through a gentle 3 kHz lowpass, and a heavily clipped version). It encoded each at 96, 128, 192, 256 and 320 kbit/s with LAME (MP3), FFmpeg's AAC encoder and Apple's AAC encoder, made transcodes from low bit rates up to high ones, and measured each file. The full table is in [bench/results/quality.md](https://github.com/lvndry/euphoria/blob/main/bench/results/quality.md). What it showed:

| Encoding | Average bit rate measured | Cutoff measured | Encoding error below 15 kHz |
| --- | --- | --- | --- |
| MP3 96 | 96 | 15.2 to 15.4 kHz | -24 dB |
| MP3 128 | 128 | 16.0 to 16.8 kHz | -26 dB |
| MP3 192 | 192 | 18.6 to 18.9 kHz, or none | -30 dB |
| MP3 256 | 256 | 19.3 to 19.6 kHz | -47 dB |
| MP3 320 | 320 | 20.1 to 20.3 kHz | -52 dB |
| Apple AAC 128 | 106 to 126 | 17.3 to 18.5 kHz, or none | -33 dB |
| Apple AAC 192 | 155 to 190 | 19.4 to 19.7 kHz, or none | -40 dB |
| Apple AAC 256 | 208 to 261 | none | -42 dB |
| Apple AAC 320 | 297 to 317 | none | -43 dB |

The encoding error is the difference between the decoded file and the original below 15 kHz, below every cutoff, relative to the music. It is a plain measure of how much the encoder changed, not a measure of what you hear, but it shows where the big step is: from 192 to 256 kbit/s for MP3, and from 128 to 192 kbit/s for AAC.

![Line chart of the encoding error below 15 kHz against the bit rate the encoder was set to. MP3 stays between -24 and -30 dB up to 192 kbit/s, then drops to -47 dB at 256 and -52 dB at 320. The two AAC encoders fall more evenly, to about -43 dB at 320 kbit/s.](../images/audio-quality/encoding-error.svg)

The thresholds that follow:

- **Good** from an average of 200 kbit/s. Apple's encoder set to 256 kbit/s averaged 208 on the quietest track, so the line sits below the nominal rate.
- **Acceptable** from 144 kbit/s. Encoders set to 128 kbit/s measured at most 129, and set to 192 kbit/s at least 155.
- **Weak on a big system** below that.
- **Likely re-encoded** when the cutoff is below what the bit rate keeps: below 18 kHz at 200 to 287 kbit/s, below 19.5 kHz from 288 kbit/s, and below 15.5 kHz at 144 to 199 kbit/s. For lossless files, below 19 kHz. These lines sit under the lowest cutoff any encoder reached at that bit rate from a lossless source, and above the cutoffs of most transcodes. 38 of 42 music transcodes are caught. The 4 that are missed show no clear fall in their spectrum after re-encoding: three times because Apple's encoder had kept the whole spectrum at 128 kbit/s, once because re-encoding added content above the earlier cutoff.
- Clipping never changes the verdict. When at least 0.1% of the samples are clipped, the reason adds that the master is clipped in places.

### Ogg Vorbis

Vorbis is encoded at a quality level, not a bit rate, and averages far below the level's nominal rate: quality 6, nominally 192 kbit/s, averaged 116 to 162 kbit/s on the same music, and quality 8, nominally 256, 148 to 221. Judged by the lines above, most quality 6 files, the level most people use, would be called weak. Vorbis files get lines of their own, from the same benchmark run with the reference encoder, `oggenc`:

| Verdict | Vorbis average bit rate |
| --- | --- |
| Good | From 145 kbit/s: quality 8 and above, and busy tracks at quality 6. |
| Acceptable | From 110 kbit/s: quality 6. |
| Weak on a big system | Below 110 kbit/s: quality 4 and below. |
| Likely re-encoded | A cutoff below 18.5 kHz from 145 kbit/s, or below 18 kHz from 110 kbit/s. The reference encoder stopped no lower than 18.9 kHz at quality 6. A Vorbis file made from a 128 kbit/s MP3 keeps the MP3's cutoff and is caught. |

The reference encoder keeps more of the top end than LAME at each level, often all of it, so a Vorbis file made from another Vorbis file of low quality, and a lossless file made from one, are recognised only when the first encoding cut below the line. The [benchmark results](https://github.com/lvndry/euphoria/blob/main/bench/results/quality.md#ogg-vorbis) have the full table.

![Scatter of the lowest cutoff of each encoding against its measured bit rate, over the verdict zones: weak below 144 kbit/s, acceptable to 199, good from 200. Below 144 kbit/s the verdict stays weak regardless of cutoff. From 144 kbit/s, the lowest cutoff of each encoding made once from a lossless source sits above the step line; the lowest cutoff of each transcode to a higher bit rate sits below it, in the likely re-encoded zone. Some transcoded files have no cutoff and cannot be identified.](../images/audio-quality/verdicts.svg)

## Damaged files

A file that is truncated or corrupt used to be analyzed as if it were fine. Euphoria now counts what went wrong while decoding and compares the length it decoded with the length the container declares. A file is **damaged** when the decoder dropped any packet, or the decoded audio is shorter than the declared length by more than 0.5% of it and at least a quarter of a second. Containers round their length, and encoder delay moves it by a few hundredths of a second, so smaller differences are not damage. A file that declares no length can only be damaged by dropped packets.

A packet the decoder rejects, or a stretch a reader had to skip to find the next valid packet, is replaced by silence of the same length. The times after it do not move, so a cue or beat grid time stays right, and the length of the track is still the length of the file. The quality report keeps the number of dropped packets, the seconds of silence, the length the file declares and the length decoded.

A file is **heavily damaged** when the audio lost, the silence put in place of dropped packets plus what is missing against the declared length, is 2% or more of the track, about 8 seconds of a 7 minute track. Below that a gap is shorter than a phrase and the beat grid still fits the rest. From 2% a gap can hide a whole phrase and the sections around it, so Euphoria does not write a beat grid or cues for the file, and says so in the notes. The key, tempo and energy are still reported, with the damage noted, since they come from the whole sound. The threshold is a judgement, not a measurement; it is a constant, `SEVERE_LOST_SHARE`.

Damage shows as a `damaged` flag with the details in `damage` and `notes` in the [JSON](../reference/cli.md#json-output), as a line in the detailed view of `euphoria analyze`, and as a note under the sound quality in the desktop inspector. It does not change the verdict: the verdict is about how the file was encoded.

Reading stops at a read error, such as the end of a file cut inside a packet. That counts as damage only when the audio decoded is short of the declared length: some formats end a normal file with the same error. Tests cover a truncated WAV and MP3, a FLAC with corrupt frames and a heavily damaged file with no cues; they use small generated files. A dropped packet in an MP3 is probably not found from timestamps, since the MP3 reader appears to number packets as it reads them, so an MP3 with one corrupt frame in the middle may show only when its decoded length falls short of its declared one. That case was not tested.

## Channels that cancel

Euphoria mixes a stereo file down to one channel by averaging. Two channels that are inverted copies of each other average to silence, and a file like that, from a phase mistake in a recording or an export, used to be reported as silent. Now every block of about 0.4 seconds of stereo is checked: when its mono mixdown holds less than 5% of the energy of its two channels, the left channel is used for that block. For two channels of equal level, 5% means a correlation below -0.9, which real music does not reach. Wide stereo stays above 20%. A file with 1% or more of its length mixed this way gets a note, and `cancelled_share` in the JSON gives the share.

The [cutoff](#the-cutoff) and the other measurements that read every channel are not affected.

## Limits

- **It judges the file, not the master.** A badly mastered track in a lossless file is still lossless.
- **Medium bit rates hide their source.** Between 144 and 199 kbit/s the line sits at 15.5 kHz, so only a source of 96 kbit/s or less is recognised. The three encoders measured all stopped at 18.6 kHz or higher at 192 kbit/s, but the line stays low because encoders not measured here may cut lower at these bit rates.
- **Good sources leave no trace.** A file made from a 320 kbit/s MP3 or from a 256 kbit/s AAC file keeps a high cutoff and passes as what it claims.
- **Some files have their own lowpass.** A master with a deliberate lowpass, or an old recording, can stop early without having been encoded twice. The verdict then says "likely", and the cutoff in the details shows where it stops.
- **Lossy encoding hides clipping.** A master clipped hard measured about 1% of its samples clipped as a lossless file, and between 0% and 0.2% after lossy encoding. A clipping figure of zero in a lossy file does not prove a clean master.
- **Peaks above 0 dB are normal in lossy files.** Decoding a loud master often gives true peaks of +1 or +2 dB. Most players and DJ software handle them; the figure is there for reference.
- **The thresholds come from four encoders.** Files from other encoders, such as Fraunhofer's, aoTuV's Vorbis encoder or those of streaming services, were not measured, nor Vorbis set to a bit rate rather than a quality level.
- **The loudness is measured on the mono mixdown**, as for the energy rating. It can differ from a stereo measurement by about a decibel.
