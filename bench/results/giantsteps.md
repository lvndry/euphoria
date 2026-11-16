# GiantSteps results

Measured on 10 November 2026 on an Apple M5 (10 cores), macOS 27.2, with 4 worker threads.

- Key: [GiantSteps Key](https://github.com/GiantSteps/giantsteps-key-dataset), 604 two minute previews of electronic dance music. All 604 decoded.
- Tempo: [GiantSteps Tempo](https://github.com/GiantSteps/giantsteps-tempo-dataset), 664 previews, scored against the version 2 annotations. 3 previews are annotated with a tempo of zero, meaning no tempo, and are left out, so 661 are scored. All 661 decoded.

## Without libkeyfinder

Measured on 15 November 2026 on an Apple M5, macOS 27.2, with three worker threads, in one run of `euphoria-bench key-parts bench/data --segments <GiantSteps+ new_annotations>`, a mode removed with libkeyfinder, since it needs it to compare. It compares each part libkeyfinder had in Euphoria with what replaces it: the pitch-class profile (now Euphoria's note spectrum), the key over time (now drone-v2's scores of 7 second windows), the `keyfinder` key engine and the key of audio shorter than drone-v2 reads (now drone-v2), and the tuning correction (now applied to drone-v2). The expert segment annotations are those of [GiantSteps+](https://zenodo.org/records/4153506). Genres with fewer than 20 previews are grouped.

### Pitch-class profile

Against the scale of drone-v2's key. Top 7 exact: the seven strongest notes are the scale. Top 7 share: how many of the seven strongest are in the scale. Contrast: the mean level of the scale's notes less that of the other five, with the strongest note at 100%.

| Genre | Previews | Top 7 exact, libkeyfinder | Top 7 exact, Euphoria | Top 7 share, libkeyfinder | Top 7 share, Euphoria | Contrast, libkeyfinder | Contrast, Euphoria |
| --- | --- | --- | --- | --- | --- | --- | --- |
| progressive-house | 88 | 25.0% | 35.2% | 83.8% | 87.0% | 26.6% | 39.7% |
| tech-house | 81 | 4.9% | 6.2% | 70.7% | 71.3% | 13.3% | 22.6% |
| deep-house | 77 | 19.5% | 23.4% | 82.9% | 84.0% | 21.5% | 35.1% |
| trance | 58 | 34.5% | 44.8% | 88.9% | 90.1% | 31.0% | 46.3% |
| electro-house | 51 | 13.7% | 19.6% | 78.2% | 81.2% | 18.4% | 32.9% |
| house | 47 | 17.0% | 19.1% | 81.5% | 83.0% | 21.7% | 37.7% |
| drum-and-bass | 38 | 15.8% | 18.4% | 74.1% | 75.6% | 13.5% | 24.3% |
| techno | 34 | 2.9% | 8.8% | 70.6% | 73.5% | 12.9% | 21.5% |
| dubstep | 22 | 9.1% | 22.7% | 74.7% | 78.6% | 18.0% | 29.9% |
| electronica | 20 | 20.0% | 20.0% | 77.9% | 78.6% | 19.9% | 30.2% |
| other genres | 88 | 18.2% | 27.3% | 78.6% | 80.4% | 19.7% | 30.4% |
| all | 604 | 17.4% | 23.5% | 79.1% | 81.0% | 20.4% | 32.7% |

The same against the annotated key:

| Genre | Previews | Top 7 share, libkeyfinder | Top 7 share, Euphoria | Contrast, libkeyfinder | Contrast, Euphoria |
| --- | --- | --- | --- | --- | --- |
| progressive-house | 88 | 79.7% | 82.5% | 23.7% | 35.3% |
| tech-house | 81 | 69.8% | 70.9% | 12.2% | 20.8% |
| deep-house | 77 | 81.3% | 81.8% | 19.1% | 31.5% |
| trance | 58 | 86.9% | 87.2% | 28.7% | 42.6% |
| electro-house | 51 | 77.0% | 79.0% | 17.1% | 31.2% |
| house | 47 | 78.7% | 80.2% | 19.3% | 33.2% |
| drum-and-bass | 38 | 71.1% | 71.8% | 11.8% | 19.9% |
| techno | 34 | 68.9% | 71.0% | 11.7% | 19.0% |
| dubstep | 22 | 74.0% | 77.9% | 16.6% | 27.2% |
| electronica | 20 | 76.4% | 77.1% | 19.1% | 29.1% |
| other genres | 88 | 77.3% | 79.4% | 18.4% | 28.6% |
| all | 604 | 77.2% | 78.7% | 18.5% | 29.7% |

### Key over time

Share of previews whose timeline holds more than one key, against drone-v2's key. With expert segment annotations, also the previews annotated with one key that are split, and the previews annotated with a change between two plain keys whose timeline shows both.

| Genre | Previews | Split, libkeyfinder | Split, drone-v2 | One expert key, split, libkeyfinder | One expert key, split, drone-v2 |
| --- | --- | --- | --- | --- | --- |
| progressive-house | 88 | 2.3% | 3.4% | 1.2% | 3.7% |
| tech-house | 81 | 2.5% | 0.0% | 2.6% | 0.0% |
| deep-house | 77 | 0.0% | 0.0% | 0.0% | 0.0% |
| trance | 58 | 3.4% | 0.0% | 3.4% | 0.0% |
| electro-house | 51 | 2.0% | 2.0% | 0.0% | 0.0% |
| house | 47 | 0.0% | 0.0% | 0.0% | 0.0% |
| drum-and-bass | 38 | 5.3% | 0.0% | 5.4% | 0.0% |
| techno | 34 | 0.0% | 0.0% | 0.0% | 0.0% |
| dubstep | 22 | 0.0% | 0.0% | 0.0% | 0.0% |
| electronica | 20 | 0.0% | 0.0% | 0.0% | 0.0% |
| other genres | 88 | 1.1% | 0.0% | 0.0% | 0.0% |
| all | 604 | 1.7% | 0.7% | 1.2% | 0.5% |

Previews annotated with a change of key: 13. Timeline shows two of the annotated keys: libkeyfinder 1, drone-v2 1.

### Key engine

The `keyfinder` key engine against drone-v2 as the `auto` engine runs it, with tuning correction, against the annotated key. The last column is drone-v2 without tuning correction, as it shipped before.

| Genre | Previews | Exact, libkeyfinder | Exact, drone-v2 | Exact or fifth, libkeyfinder | Exact or fifth, drone-v2 | Exact, fifth or relative, libkeyfinder | Exact, fifth or relative, drone-v2 | Exact, drone-v2 without tuning correction |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| progressive-house | 88 | 56.8% | 63.6% | 67.0% | 68.2% | 81.8% | 83.0% | 63.6% |
| tech-house | 81 | 56.8% | 65.4% | 70.4% | 72.8% | 72.8% | 74.1% | 65.4% |
| deep-house | 77 | 70.1% | 85.7% | 81.8% | 89.6% | 84.4% | 89.6% | 85.7% |
| trance | 58 | 46.6% | 53.4% | 63.8% | 67.2% | 77.6% | 82.8% | 53.4% |
| electro-house | 51 | 72.5% | 74.5% | 78.4% | 82.4% | 86.3% | 90.2% | 74.5% |
| house | 47 | 55.3% | 72.3% | 68.1% | 76.6% | 76.6% | 85.1% | 72.3% |
| drum-and-bass | 38 | 50.0% | 63.2% | 57.9% | 65.8% | 65.8% | 71.1% | 63.2% |
| techno | 34 | 52.9% | 67.6% | 67.6% | 73.5% | 70.6% | 73.5% | 67.6% |
| dubstep | 22 | 50.0% | 63.6% | 63.6% | 77.3% | 72.7% | 81.8% | 63.6% |
| electronica | 20 | 70.0% | 75.0% | 90.0% | 75.0% | 90.0% | 80.0% | 65.0% |
| other genres | 88 | 72.7% | 79.5% | 78.4% | 83.0% | 80.7% | 85.2% | 79.5% |
| all | 604 | 60.6% | 70.2% | 71.9% | 76.2% | 78.6% | 82.3% | 69.9% |

Previews on which only one of the two is exact or a fifth away, and the two-sided sign test on them:

| Genre | Only libkeyfinder | Only drone-v2 | Sign test p |
| --- | --- | --- | --- |
| progressive-house | 4 | 5 | 1.000 |
| tech-house | 6 | 8 | 0.791 |
| deep-house | 1 | 7 | 0.070 |
| trance | 5 | 7 | 0.774 |
| electro-house | 0 | 2 | 0.500 |
| house | 1 | 5 | 0.219 |
| drum-and-bass | 2 | 5 | 0.453 |
| techno | 3 | 5 | 0.727 |
| dubstep | 1 | 4 | 0.375 |
| electronica | 3 | 0 | 0.250 |
| other genres | 2 | 6 | 0.289 |
| all | 28 | 54 | 0.005 |

### Audio shorter than drone-v2 reads

1 second from 30 seconds into each preview, which libkeyfinder decided before and drone-v2 now reads followed by silence.

| Genre | Previews | Exact, libkeyfinder | Exact, drone-v2 | Exact, fifth or relative, libkeyfinder | Exact, fifth or relative, drone-v2 |
| --- | --- | --- | --- | --- | --- |
| progressive-house | 88 | 21.6% | 40.9% | 47.7% | 62.5% |
| tech-house | 81 | 25.9% | 42.0% | 44.4% | 59.3% |
| deep-house | 77 | 33.8% | 51.9% | 55.8% | 74.0% |
| trance | 58 | 20.7% | 43.1% | 44.8% | 65.5% |
| electro-house | 51 | 17.6% | 41.2% | 45.1% | 62.7% |
| house | 47 | 29.8% | 61.7% | 61.7% | 78.7% |
| drum-and-bass | 38 | 23.7% | 26.3% | 39.5% | 44.7% |
| techno | 34 | 29.4% | 52.9% | 52.9% | 64.7% |
| dubstep | 22 | 13.6% | 27.3% | 45.5% | 72.7% |
| electronica | 20 | 45.0% | 45.0% | 65.0% | 60.0% |
| other genres | 88 | 34.1% | 42.0% | 53.4% | 58.0% |
| all | 604 | 26.8% | 43.9% | 50.0% | 63.7% |

### Tuning

Chord progressions in all 24 keys, synthesized at each offset. The tuning is measured by the same code before and after. Key: the `keyfinder` engine and drone-v2 as the `auto` engine runs it, both with tuning correction. Profile: the seven strongest notes are the notes played. At 50 cents either neighbouring scale counts.

| Offset (cents) | Largest tuning error (cents) | Key, libkeyfinder | Key, drone-v2 | Profile, libkeyfinder | Profile, Euphoria |
| --- | --- | --- | --- | --- | --- |
| -50 | 0.2 | 100.0% | 100.0% | 100.0% | 100.0% |
| -40 | 0.2 | 100.0% | 100.0% | 100.0% | 100.0% |
| -30 | 0.3 | 100.0% | 100.0% | 100.0% | 100.0% |
| -20 | 0.2 | 100.0% | 100.0% | 100.0% | 100.0% |
| -10 | 0.1 | 100.0% | 100.0% | 100.0% | 100.0% |
| +0 | 0.2 | 100.0% | 100.0% | 100.0% | 100.0% |
| +10 | 0.2 | 100.0% | 100.0% | 100.0% | 100.0% |
| +20 | 0.3 | 100.0% | 100.0% | 100.0% | 100.0% |
| +30 | 0.3 | 100.0% | 100.0% | 100.0% | 100.0% |
| +40 | 0.2 | 100.0% | 100.0% | 100.0% | 100.0% |
| +50 | 0.2 | 100.0% | 100.0% | 100.0% | 100.0% |

### Speed

CPU seconds per preview for the key analysis of the `auto` engine beyond decoding: before, drone-v2 then libkeyfinder for the tuning and the key over time (whose chromagram also gives the profile); after, drone-v2 with the key over time, the tuning and the note spectrum, with the tuning measured twice (about 0.03 s), so the after column is a little high.

| Before | After |
| --- | --- |
| 0.434 | 0.379 |

On 20 full length tracks from a private library, 6.4 minutes long on average, the key analysis took 1.33 CPU seconds per track before and 1.09 after, and 1 track had more than one key in its timeline before and none after.

## Key detection with drone-v2

Measured on 14 November 2026 on an Apple M5 (10 cores), macOS 27.2, with the benchmark's default worker threads, in one run of `euphoria-bench bench/data --only key`. Euphoria ships drone-v2: `--key-engine auto` takes its key and uses libkeyfinder for the tuning and the key over time.

| Detector | Exact | Fifth | Relative | Parallel | Other | No key or error | MIREX score | CPU seconds per track |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| drone-v2 | 69.9% | 6.0% | 6.1% | 3.8% | 14.2% | 0.0% | 75.4% | 0.31 |
| libkeyfinder 2.2.8 with tuning correction | 60.6% | 11.3% | 6.8% | 5.5% | 15.9% | 0.0% | 69.4% | 0.27 |
| libkeyfinder 2.2.8 without tuning correction (not used) | 60.4% | 11.1% | 6.5% | 5.6% | 16.4% | 0.0% | 69.0% | 0.24 |
| libkeyfinder 2.2.8 with tuning correction, sections vote (not used) | 60.9% | 11.3% | 7.0% | 5.5% | 15.4% | 0.0% | 69.7% | 0.27 |
| stratum-dsp 1.0.0 | 30.6% | 20.7% | 6.1% | 12.9% | 29.6% | 0.0% | 45.4% | 2.39 |

Previews each detector scores higher and lower than drone-v2, by MIREX score:

| Detector | Higher | Lower |
| --- | --- | --- |
| libkeyfinder with tuning correction | 41 | 97 |
| libkeyfinder without tuning correction | 40 | 97 |
| libkeyfinder with tuning correction, sections vote | 43 | 95 |
| stratum-dsp | 43 | 289 |

On exact matches alone, drone-v2 is right on 83 previews where libkeyfinder with tuning correction is wrong, and libkeyfinder on 27 where drone-v2 is wrong (two-sided sign test p = 8.3e-8). Resampling the previews 20,000 times, drone-v2's gain lies between 6.0 and 12.6 points of exact matches and between 3.5 and 8.7 points of MIREX score 95% of the time.

Against the [GiantSteps+](https://zenodo.org/records/4153506) expert labels, from the per-preview readings of `euphoria-bench key-readings`, exact:

| Labels | Previews | drone-v2 | libkeyfinder | Gain (95% interval) |
| --- | --- | --- | --- | --- |
| GiantSteps Key | 604 | 69.9% | 60.6% | 9.3 (6.0 to 12.6) |
| GiantSteps+, the key of the longest segment | 571 | 80.7% | 70.1% | 10.7 (7.2 to 14.2) |
| GiantSteps+, previews with one segment and one key | 457 | 88.0% | 76.2% | 11.8 (8.1 to 15.5) |

The 571 leave out previews whose expert annotation gives no major or minor key and one annotation without audio.

| drone-v2 key confidence | All 604 | Development half | Held out half |
| --- | --- | --- | --- |
| High, share of tracks | 41.4% | 40.1% | 42.7% |
| High, exact | 87.2% | 90.9% | 83.7% |
| Medium, exact | 67.5% | 67.2% | 67.7% |
| Low, exact | 34.3% | 40.7% | 26.1% |
| High, key or runner up exact | 90.4% | 94.2% | 86.8% |
| Medium, key or runner up exact | 81.5% | 82.8% | 80.3% |
| Low, key or runner up exact | 56.2% | 57.6% | 54.3% |

The edges, low below a probability margin of 0.16 and high from 0.72, were set on the development half so that about one preview in five is low and two in five high. Of the 182 previews whose drone-v2 key is wrong, the runner up is the annotated key for 66 (33 and 33 in the two halves).

Taking libkeyfinder's key when drone-v2's margin is below a threshold chosen on the development half (0.03) changes the held out MIREX score by +0.9 points (95% interval −0.2 to +2.2), so drone-v2 decides alone.

A whole `euphoria analyze --no-cache --json` of one file, one run each with `/usr/bin/time -l`, wall seconds and peak memory:

| Key engine | Two minute preview | Ten minute file |
| --- | --- | --- |
| `keyfinder`, libkeyfinder alone | 1.12 s, 273 MB | 5.60 s, 1.49 GB |
| `drone`, drone-v2 alone | 1.11 s, 246 MB | 5.62 s, 1.10 GB |
| `auto`, the default | 1.31 s, 308 MB | 6.65 s, 1.49 GB |

The ten minute file is five previews joined into one 192 kbit/s MP3.

### drone-v2 by genre

Measured on 15 November 2026 on the same machine, in one run of `euphoria-bench key-readings bench/data` with two worker threads. The key is drone-v2's most probable key; each preview's genre is its file in `annotations/genre` of the GiantSteps Key dataset. Genres with fewer than 20 previews are grouped. The totals equal the table above. Intervals are 95% Wilson intervals on exact.

| Genre | Previews | Exact | Exact (95% interval) | Exact or fifth | Exact, fifth or relative | Parallel | Other | MIREX score |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| progressive-house | 88 | 63.6% | 53 to 73 | 68.2% | 83.0% | 8.0% | 9.1% | 71.9% |
| tech-house | 81 | 65.4% | 55 to 75 | 72.8% | 74.1% | 3.7% | 22.2% | 70.2% |
| deep-house | 77 | 85.7% | 76 to 92 | 89.6% | 89.6% | 1.3% | 9.1% | 87.9% |
| trance | 58 | 53.4% | 41 to 66 | 67.2% | 82.8% | 3.4% | 13.8% | 65.7% |
| electro-house | 51 | 74.5% | 61 to 84 | 82.4% | 90.2% | 0.0% | 9.8% | 80.8% |
| house | 47 | 72.3% | 58 to 83 | 76.6% | 85.1% | 8.5% | 6.4% | 78.7% |
| drum-and-bass | 38 | 63.2% | 47 to 77 | 65.8% | 71.1% | 5.3% | 23.7% | 67.1% |
| techno | 34 | 67.6% | 51 to 81 | 73.5% | 73.5% | 5.9% | 20.6% | 71.8% |
| dubstep | 22 | 63.6% | 43 to 80 | 77.3% | 81.8% | 4.5% | 13.6% | 72.7% |
| electronica | 20 | 65.0% | 43 to 82 | 65.0% | 70.0% | 0.0% | 30.0% | 66.5% |
| 13 other genres | 88 | 79.5% | 70 to 87 | 83.0% | 85.2% | 1.1% | 13.6% | 82.2% |
| all | 604 | 69.9% | 66 to 73 | 75.8% | 82.0% | 3.8% | 14.2% | 75.4% |

The 13 other genres: breaks 14, indie-dance nu-disco 14, chill-out 11, minimal 11, pop rock 7, glitch-hop 6, psy-trance 5, hip-hop 4, hard-dance 4, reggae dub 3, hardcore hard-techno 3, funk r&b 3, dj-tools 3.

The tables below this one are earlier measurements, before drone-v2.

## Key detection

| Detector | Exact | Fifth | Relative | Parallel | Other | MIREX score | CPU seconds per track |
| --- | --- | --- | --- | --- | --- | --- | --- |
| libkeyfinder 2.2.8 with tuning correction | 60.6% | 11.3% | 6.8% | 5.5% | 15.9% | 69.4% | 0.26 |
| libkeyfinder 2.2.8 without tuning correction (not used) | 60.4% | 11.1% | 6.5% | 5.6% | 16.4% | 69.0% | 0.23 |
| libkeyfinder 2.2.8 with tuning correction, sections vote (not used) | 60.9% | 11.3% | 7.0% | 5.5% | 15.4% | 69.7% | 0.26 |
| stratum-dsp 1.0.0 | 30.6% | 20.7% | 6.1% | 12.9% | 29.6% | 45.4% | 2.21 |
| Essentia, bgate profile (reference) | 61.3% | 8.3% | 8.8% | 7.3% | 14.4% | 69.5% | not measured |
| Essentia, edma profile (reference) | 57.6% | 8.3% | 6.3% | 10.8% | 17.1% | 65.8% | not measured |
| Essentia, shaath profile (reference) | 55.1% | 7.9% | 7.3% | 14.1% | 15.6% | 64.1% | not measured |
| Essentia, krumhansl profile (reference) | 49.0% | 8.8% | 8.6% | 15.6% | 18.0% | 59.1% | not measured |

Fifth, relative and parallel are near misses that still mix acceptably. The MIREX score weights them 0.5, 0.3 and 0.2 against 1.0 for an exact match.

The three libkeyfinder rows and the stratum-dsp row were measured on 11 November 2026 on the same machine, in one run, while it was busier than on the day before: stratum-dsp took 1.99 CPU seconds per track then. Previews each of the other two scores higher and lower than the first, which is what Euphoria uses:

| Detector | Higher | Lower | Development half, higher and lower | Held out half, higher and lower |
| --- | --- | --- | --- | --- |
| libkeyfinder without tuning correction | 0 | 4 | 0 and 4 | 0 and 0 |
| libkeyfinder with tuning correction, sections vote | 8 | 7 | 5 and 3 | 3 and 4 |

The halves are every other preview in name order, 302 each, as `--tracks development` and `--tracks held-out` select them.

## Tempo detection

| Detector | Accuracy 1 | Accuracy 2 | Below 70 BPM | Above 180 BPM | CPU seconds per track |
| --- | --- | --- | --- | --- | --- |
| stratum-dsp 1.0.0, tempo only, best of four candidates by onset repeats, reported between 90 and 180 BPM | 85.8% | 94.3% | 0 | 0 | 0.52 (not comparable with the rows below, measured under heavier load) |
| The same, made precise by the beat grid when it is rated medium or high and snapped to a whole number when that fits as well (shipped; 16 November 2026) | 85.8% | 94.3% | 0 | 0 | 0.56 (with the grid; 0.43 without it in the same run) |
| stratum-dsp 1.0.0, tempo only, first candidate, reported between 90 and 180 BPM (not used) | 77.6% | 85.5% | 0 | 0 | 0.38 |
| stratum-dsp 1.0.0, tempo only, estimate as it comes (not used) | 62.2% | 84.7% | 83 | 0 | 0.39 |
| stratum-dsp 1.0.0, full analysis with default settings (not used) | 59.9% | 84.9% | 26 | 0 | 3.30 |
| Essentia RhythmExtractor2013 (reference) | 59.8% | 82.5% | not measured | not measured | not measured |

The two count columns are how many of the 661 results fall below 70 and above 180 BPM. The annotations have 3 below 70 and 6 above 180.

Accuracy 1 counts an estimate within 4% of the annotated tempo. Accuracy 2 also accepts 2, 1/2, 3 and 1/3 times the annotated tempo, as in the standard tempo evaluation.

### Tempo errors by kind and half

Measured on 14 November 2026 by `euphoria-bench --only tempo --tracks development` and `--tracks held-out`, before and after the choice among the four best candidates. Intervals are 95% Wilson intervals.

| Half | Tracks | Accuracy 1 before | Accuracy 1 after | Accuracy 2 before | Accuracy 2 after |
| --- | --- | --- | --- | --- | --- |
| Development | 331 | 75.5% (70.6 to 79.8) | 84.9% (80.6 to 88.4) | 83.4% (79.0 to 87.0) | 93.1% (89.8 to 95.3) |
| Held out | 330 | 79.7% (75.0 to 83.7) | 86.7% (82.6 to 89.9) | 87.6% (83.6 to 90.7) | 95.5% (92.6 to 97.2) |

| Result | Development before | Development after | Held out before | Held out after |
| --- | --- | --- | --- | --- |
| Half | 2 | 3 | 5 | 6 |
| Double | 24 | 24 | 21 | 23 |
| 3:2 or 2:3 | 8 | 8 | 8 | 7 |
| 4:3 or 3:4 | 11 | 5 | 9 | 6 |
| 5:4 or 4:5 | 24 | 1 | 19 | 1 |
| Triple or a third | 0 | 0 | 0 | 0 |
| Other | 12 | 9 | 5 | 1 |

All 661 previews after: Accuracy 1 85.8% (82.9 to 88.2), Accuracy 2 94.3% (92.2 to 95.8). External check on 200 GTZAN disco and hip hop excerpts, tempo from the beat annotations: Accuracy 1 89.0% (83.9 to 92.6) before and 90.5% (85.6 to 93.8) after, Accuracy 2 96.0% (92.3 to 98.0) before and 98.0% (95.0 to 99.2) after.

## Confidence

Measured on 11 November 2026 with the same machine and settings, by the confidence sections at the end of the key and tempo benchmarks. The band edges were set on the development half (every other preview in name order, starting with the first) and checked on the held out half; the development column is therefore not a test of them.

| Key confidence | All 604 | Development half | Held out half |
| --- | --- | --- | --- |
| High, share of tracks | 40.2% | 40.7% | 39.7% |
| High, exact | 75.7% | 77.2% | 74.2% |
| Medium, exact | 56.4% | 55.8% | 57.0% |
| Low, exact | 38.3% | 35.6% | 41.0% |
| High, key or runner up exact | 86.4% | 91.1% | 81.7% |
| Medium, key or runner up exact | 75.5% | 72.5% | 78.5% |
| Low, key or runner up exact | 65.8% | 62.7% | 68.9% |

Of the 238 previews whose detected key is wrong, the runner up is the annotated key for 105 (53 and 52 in the two halves). Against the detected key, the runner up is a fifth away for 211 previews, the parallel key for 196, the relative key for 112 and another key for 85.

“Right modulo octaves” accepts the reported tempo multiplied by 1/4, 1/2, 1, 2 or 4, within the same 4% tolerance as Accuracy 1. This measures the beat period independently of its octave, allowing up to two doublings or halvings; it is separate from Accuracy 2.

| Tempo confidence, between 90 and 180 BPM | All 661 | Development half | Held out half |
| --- | --- | --- | --- |
| High, share of tracks | 39.2% | 39.3% | 39.1% |
| High, right modulo octaves | 98.1% | 98.5% | 97.7% |
| Medium, right modulo octaves | 85.6% | 81.2% | 89.8% |
| Low, right modulo octaves | 60.6% | 58.8% | 62.5% |
| High, Accuracy 1 | 85.7% | 88.5% | 82.9% |
| Medium, Accuracy 1 | 80.4% | 73.7% | 86.9% |
| Low, Accuracy 1 | 56.1% | 54.4% | 57.8% |

Half or double the tempo is offered as an alternative for 267 previews (40.4%), and is the annotated tempo for 46 of the 52 previews reported an octave off: 24 of 26 in the development half, where the span of 75 to 200 BPM was chosen, and 22 of 26 in the held out half.

Rating confidence costs no measurable time: the tempogram already scores the candidate tempos the margin is taken from, and the key margin is the difference of two scores libkeyfinder already computed. On the same machine, under heavy load from other work, `euphoria analyze --no-cache` took 1.10 CPU seconds on one preview before and 1.06 after, 4.92 and 4.69 on an eight minute file, and 42.8 and 42.4 on 40 previews with 4 workers, with the same peak memory.

## Reading the numbers

- libkeyfinder is level with Essentia's best profile on key (69.4% against 69.5% MIREX) and about 8 times cheaper than stratum-dsp.
- stratum-dsp is far behind on key (45.4%). On tempo its raw estimate is level with Essentia, and reporting it between 90 and 180 BPM puts it well ahead on Accuracy 1.
- Tempo only is what `detect_tempo` runs: the stratum-dsp tempogram estimate without its multi-resolution pass, and without the onset lists, beat grid and key that a full analysis also computes. `detect_tempo` then doubles an estimate below 90 BPM and halves one at 180 or above until it lies between the two. The raw estimate often lands on half the tempo a DJ counts, which is what the 83 results below 70 BPM are. Applied to the full analysis, the same rule gives 77.5% and 85.5%, so the cheaper estimate loses nothing. 50 previews are annotated below 90 BPM and 6 at 180 or above; the rule reports those at double or half, and they count as Accuracy 1 misses. The full analysis row is `analyze_audio` with its default settings, which `detect_tempo` ran before, measured in the same session for comparison. Dropping the multi-resolution pass changes the estimate on 262 previews, for 15 more within 4% and 1 fewer within the Accuracy 2 factors.
- Tuning correction is on, and the benchmark does not show a gain from it: 4 previews score higher with it and none lower, which is within chance (sign test p = 0.125). It acts on the 5 previews measured 20 cents or more off. It is on because detuned tracks are rare here and common in a DJ library, because synthesized progressions lose their key from about 45 cents off without it, and because the tuning is measured anyway. [Accuracy](../../docs/concepts/accuracy.md#tuning-and-key-over-time) has the reasoning and what two minute previews of dance music cannot show.
- The vote between sections scores 8 previews higher and 7 lower than the shipped detector, so it is not used.
- The rows with tuning correction cost 0.03 CPU seconds more per track, which is the tuning measurement. The vote costs nothing measurable, since it scores the chromagram libkeyfinder has already computed.
- CPU seconds are the CPU time of each detector call on its own thread, averaged over tracks, so they do not depend on how many threads run in parallel. They were measured while other work ran on the machine, so compare them with each other and not with other sessions.
- The tempo benchmark alone takes 83 seconds and peaks at 0.9 GB of memory with 4 worker threads. With the full analysis it took 10.6 minutes and peaked at 5.7 GB. The whole run, key and tempo, took 8.6 minutes and peaked at 4.8 GB, which comes from stratum-dsp key detection.

## Cost of a whole analysis

What `euphoria analyze --no-cache` costs for decoding, key (libkeyfinder), tempo and energy together, measured with `/usr/bin/time -l` on the same machine. Memory is the peak memory footprint of the process. The full length tracks are 40 tracks of 4 to 8 minutes from a private library, and the 80 track runs analyze each of them twice.

| Run | CPU seconds | Peak memory |
| --- | --- | --- |
| One two minute preview, 1 worker | 0.8 | 221 MB |
| One seven minute track, 1 worker | 2.6 | 559 MB |
| 40 full length tracks, 4 workers | 100 | 1.9 GB |
| 40 full length tracks, 8 workers | 121 | 4.8 GB |
| 80 full length tracks, 4 workers | 204 | 2.1 GB |
| 80 full length tracks, 8 workers | 245 | 6.2 GB |

With the full stratum-dsp analysis for tempo, the same build took 2.8 CPU seconds and 871 MB on the preview, 10.9 CPU seconds and 2.7 GB on the seven minute track, and 539 CPU seconds and 9.8 GB on the 40 tracks with 4 workers.

Measuring the tuning, correcting for it and following the key over time are part of every analysis. Their cost was measured on 11 November 2026 against the build before them, on the same machine and files, three runs each. CPU seconds are user and system time together:

| Run | CPU seconds before | CPU seconds after | Peak memory before | Peak memory after |
| --- | --- | --- | --- | --- |
| One two minute preview, 1 worker | 0.79 | 0.82 | 221 MB | 221 MB |
| One eight minute track, 1 worker | 3.51 | 3.56 | 661 MB | 661 MB |
| 40 two minute previews, 4 workers | 32.1 | 33.0 | 0.66 to 0.71 GB | 0.74 to 0.75 GB |

The eight minute track is four previews joined into one FLAC file, so it is not comparable with the seven minute track above, and the machine was less busy than for the table above. The cost is the tuning measurement, 0.02 to 0.03 CPU seconds per track whatever its length, since it reads at most 256 frames. The correction and the key over time use the chromagram libkeyfinder computes for the key. One of the three runs of 40 previews after the change took 35.2 CPU seconds and the other two 31.6 and 32.3, so the difference on that row is within the spread of the runs.

Tempo detection reads at most the middle ten minutes of a file, decimated to between 44.1 and 88.2 kHz when the file has a higher sample rate, so its share of the memory stops growing there. Decoding and key detection read the whole file. On a six minute file at 384 kHz, decoding and tempo detection together peak at 1.0 GB, of which decoding is 0.5 GB, and tempo takes 3.8 CPU seconds. Without the decimation they peaked at 4.3 GB and tempo took 10.7 CPU seconds.

## Reproducing

```
./bench/download_datasets.sh
cargo run --release -p euphoria-bench
cargo run --release -p euphoria-bench -- bench/data --only tempo
uv run bench/essentia_reference.py bench/data
```
