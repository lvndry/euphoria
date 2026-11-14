---
title: Desktop app
description: Run the Euphoria desktop app from source and see what it does today.
---

# Desktop app

The desktop app shows your library as a table: add folders, watch each track get its key and tempo, then sort, search and find tracks that mix together. It uses the same analysis as the command line tool, so both give the same answers.

The app is an early version. It runs from a source checkout on macOS; there is no download yet.

## Run it from source

Install the requirements in [building from source](../development/building.md). On macOS you also need Xcode, which provides the Metal shader compiler the interface is built with. Then:

```
cargo run --release -p euphoria-desktop
```

On macOS the command opens the app as **Euphoria**, with its name and icon in the Dock.

Use `--release`: analysis is many times slower in a debug build.

## Add music

On first launch the window shows a drop zone. Drop a folder, or several folders and files, from Finder onto the window, or click **Choose folder**. Later, use **Add folder** in the toolbar, **File > Add Folder** or Command O.

Folders are searched all the way down for MP3, M4A, MP4, FLAC, Ogg Vorbis, WAV and AIFF files. A track Euphoria has analyzed before is filled in at once. Every other track appears as a row marked "Queued", changes to "Analyzing" when its turn comes, and fills in when its analysis finishes. The toolbar shows how many are done, for example "12 of 340 done". A file that could not be analyzed counts as done.

The app analyzes as many tracks at once as your computer has cores, up to 4. The command line tool goes up to 8 by default and lets you choose. See [large libraries and memory](analyze-your-library.md#large-libraries-and-memory).

Your files are only read, until you choose to [write tags](#write-tags) into them.

## The track table

| Column | Shows |
| --- | --- |
| Title | The title in the file's tags, or the file name when there is none |
| Artist | The artist in the file's tags |
| Key | The key's code, coloured by its Camelot number: Camelot (8A) unless you choose musical (Am) or Open Key (1m) in the [settings](#settings) |
| Musical key | The key by name, such as A minor |
| BPM | The tempo, to one decimal |
| Energy | The [energy rating](../concepts/energy.md) from 1 to 10, with a bar |
| Cues | How many cues the track has: the ones you set in the inspector if you changed them, otherwise the ones analysis placed, the same list the inspector shows. Empty until the track is analyzed. |
| Quality | The [audio quality](../concepts/audio-quality.md) verdict: Lossless, Good, Acceptable, Weak, or Re-encoded? for a file likely made from a lower quality one. Weak and Re-encoded? stand out; the others are quieter. |
| Time | The length |
| Format | The file type |
| Folder | Where the file is, from the folder on show |

The Folder column shows the path from the folder chosen in the sidebar to the file's own folder. With `DJ tracks` chosen, a file in `DJ tracks/afro_house/warmup` shows `afro_house/warmup`; with `afro_house` chosen, it shows `warmup`; a file directly in the chosen folder shows a dash. Under **All tracks** the path starts inside the folder you added, with that folder's name in front when you added more than one. The column grows to fit the longest path on show, as long as the title keeps room; a path still too long keeps its last folder names in view, and pointing at it shows the whole path. Sorting by Folder groups the files folder by folder, in the order of the sidebar.

Titles and artists are read from the files after they appear, so the file name shows for a moment first. Pointing at a title shows it in full.

### Resize the columns

Drag the line between two column headers to make the column on its left wider or narrower; the pointer turns into a resize arrow over it. Double click the line to fit the column to its longest text among the tracks on show. Each column has a narrowest width, so a key, a tempo and an energy bar always stay readable. **View > Reset Column Widths** returns every column to its usual width.

Until you resize them, Title and Artist share the room the other columns leave, and Folder is as wide as its longest path. In a narrow window, or when the widths you chose do not fit, the table leaves columns out rather than squeezing the title: first Cues, Format and Quality, which the inspector also shows, so that folder paths keep their room; then Folder, Artist, Time, Musical key and the row number. In a 1440 pixel wide window with both the sidebar and the inspector open, Cues and Quality are left out; fold the sidebar or close the inspector to bring it back. The title, key, tempo and energy always stay.

Click a column header to sort by it, and click again to reverse the order. Sorting by key follows the Camelot wheel: by number, then the minor key (A) before the major key (B). Sorting by Cues puts the tracks with the fewest cues first, and the most when reversed. Sorting by Quality puts the files most worth checking first: likely re-encoded, then weak, acceptable, good and lossless. Tracks with no value in the sorted column come last.

Type in the search field, or press Command F first, to filter as you type. Each word must match part of the file name, the title or the artist, the whole key code (`8a`), or the start of the tempo (`124`).

Click a row to select it. The arrow keys, Page Up, Page Down, Home and End move the selection. Command click adds a track to the selection or takes it out, Shift click selects every row between, and Command A selects every track on show.

## The sidebar

- **All tracks** shows the whole library.
- **Needs review** shows the tracks worth checking by ear: those whose key or tempo is rated low confidence and not yet [corrected by you](#check-the-key-by-ear-and-correct-it), those whose tags [name another key](#when-the-tags-name-another-key), and those that could not be analyzed, within the folder on show when one is chosen. Its count follows that folder, and a line under the heading says why the tracks are there. [Accuracy](../concepts/accuracy.md#confidence) explains how confidence is rated.
- **Quality to check** shows the tracks whose quality is weak on a big system or likely re-encoded: the files worth replacing with a better copy. Like Needs review, it works within the folder on show.
- **Favorites** shows the tracks you starred, in the order you starred them. See [Favorites](#favorites).
- **Missing files** shows the tracks whose file is no longer where it was, within the folder on show. See [Missing files](#missing-files).
- **Folders** shows each folder you added as a tree of the folders inside it, to any depth, with the number of tracks in each, counting the folders inside it. Folders with no audio files anywhere inside them are left out.

### Browse your folders

Click the triangle beside a folder to show the folders inside it, and again to hide them. Click a folder to show its tracks in the table, including those in the folders inside it; the heading above the table names it. Search and sorting work inside the chosen folder. **All tracks** shows everything again.

The tree lists folders only. The table is the list of files, with their key, tempo and energy, and the Folder column says where each file sits inside the chosen folder.

After clicking a folder, the keyboard moves through the tree:

| Key | Does |
| --- | --- |
| Up and Down | Show the folder above or below |
| Right | Show the folders inside, or go to the first of them when they are already shown |
| Left | Hide the folders inside, or go to the folder holding this one |
| Home and End | Show the first or the last folder listed |

A long folder name is cut short; point at it to read the full path. When the tree is taller than the window, it scrolls.

If you add a folder that is inside a folder you already added, it appears in the outer folder's tree rather than a second time.

To give the table more room, fold the sidebar away. Any of these folds it, and the same one brings it back:

- the sidebar button at the left of the toolbar, beside the window buttons, which stays there while the sidebar is folded
- Control Command S (Control Alt S on Linux and Windows)
- **View > Hide Sidebar**, which reads **Show Sidebar** while it is folded

## The inspector

The inspector shows the selected track: its name, format and length, its key, tempo and energy in three boxes, and its waveform. The waveform is read from the file when you select the track. Below them, **Sound quality** gives the quality verdict, the reason for it, and what it rests on: codec, bit rate, sample rate, where the content stops, loudness and true peak.

### How sure the key and the tempo are

The Key and BPM boxes say quietly how far to trust the value: **high**, **medium** or **low**, beside the box's name. Low is shown in red, and puts the track under **Needs review**. Point at the word for what it means.

When the key is not rated high, the key that matched second best sits beside it, as a chip such as "or 9A": play the track and compare the two by ear. When a DJ could plausibly count the track at half or double the tempo, that tempo is shown under it, such as "or 87.0". Click either to use it instead, as the next section explains.

### When the tags name another key

Euphoria reads the key already written in each file's tags, by Euphoria, rekordbox, Mixed In Key, Traktor or any other program, in Camelot, Open Key or musical notation. When it names another key than the one detected, a red **≠** sits beside the key in the table, the track is listed under **Needs review**, and the inspector says under the Key box what the tags say and how the two keys differ:

- **a fifth apart**: one step on the Camelot wheel, such as 8A and 9A. The keys share six of their seven notes, the most common mix-up between detectors, and both mix well with the same tracks.
- **relative key**: the same number with the other letter, such as 8A and 8B. The keys share all seven notes; listen for whether the track sounds minor or major.
- **parallel key**: the same home note in the other mode, such as A minor and A major. Listen for minor or major before you trust either.
- **different key**: the keys share few notes, so one reading is wrong or the track changes key. Check by ear before you mix on it.

Point at the **≠** for the key from the tags. When the tags agree, the inspector says so quietly; text that is not a key, such as a word typed into the key field, is shown as it is and does not put the track under Needs review. Setting the key yourself, as the next section explains, takes the track off the list. The tags are only read: nothing changes them until you [write tags](#write-tags), after which they name the key you see.

### Check the key by ear and correct it

Under the three boxes, **Check by ear** helps you settle a key you are not sure of while the track plays:

- **8A tone**, named in the key notation you chose, plays a soft drone of the key's root and fifth over the track. When the runner up key is offered, a second button plays its drone. The drone that sounds right under the music is the key; one that clashes is not. Click the button again to stop it. Only one drone plays at a time, and it stops when you select another track. The drone leaves out the third on purpose, so it sounds the same for a minor key and its parallel major; the piano settles that.
- The **piano** covers one octave from middle C. The notes of the key's scale are tinted and named, and its root is marked. Click a key to play it over the track: notes of the right scale fit the melody, others stand out.
- **Volume** sets how loud the drone and the piano are over the track, in five steps, and moves a drone that is already playing.

Both follow the tuning analysis measured, so a track tuned a little sharp or flat gets a reference that matches it.

When your ear disagrees with Euphoria, correct the value:

1. Click the runner up chip ("or 9A") to take that key, or the other tempo ("or 87.0") to take that tempo.
2. Or click **Correct the key, BPM or energy** to pick any of the 24 keys, laid out in Camelot order, type a tempo such as 124 or 87.5 and press Return or **Use this tempo**, or pick an energy from 1 to 10.
3. The box then says **by you** where it said how sure the value was, and the line under it offers **reset to 8A**, the detected value, to undo your correction. For the energy, **Reset to detected** in the editor does the same.

An energy you pick is the same 1 to 10 rating the analysis reports, for the tracks whose energy you disagree with, for example a quiet track that lifts a room. It is kept and used like a corrected key or tempo, and a track with a corrected energy keeps its detected key, tempo, beat grid and cues.

A value you set wins everywhere: in the table and search, in **Mixes well with**, in **Needs review**, which no longer lists a track for a value you corrected, in the tags you write, and in `euphoria analyze` and `euphoria export rekordbox` on the command line, which mark it as set by you. Corrections are kept in the [analysis cache](../reference/cli.md#corrections), apart from the analyses: analyzing a track again, or a new version of Euphoria analyzing everything again, never loses them, and they follow a file that was moved, renamed or had its tags written. `euphoria cache clear` keeps them too.

**Mixes well with** lists up to eight other tracks in your library whose key mixes with the selected one and whose tempo is close to it. It uses the same saved controls as the suggestions under the decks, described under [Narrow the suggestions](#narrow-the-suggestions): the key moves that count (the same key, the keys one step either way on the Camelot wheel and the relative major or minor, until you change them), the tempo window (6 percent), the tempo and energy ranges, the genre, and the pool chosen with **Whole library**, **Tracks shown**, **Favorites** or a playlist. The smoothest key moves come first, then the closest tempos, then the closest energy. Transitions you marked as favorites come first and stay even when the controls would leave them out. Each row shows the key, the name, how much faster or slower the track is, and how the keys relate. Click a row to go to that track. When a saved filter narrows the list, a line under the heading says what is narrowed, and clicking it opens the decks, where the controls are. When the filter leaves nothing, the list says so. The Camelot wheel tints the keys that the same key moves reach. [Keys and the Camelot wheel](../concepts/keys-and-camelot.md) explains these moves.

### Resize the inspector

Drag the left edge of the inspector to make it wider or narrower. The pointer turns into a resize arrow over the edge. The waveform, the three boxes and the list of mixes follow the new width. Double click the edge to return to the usual width.

| | Width |
| --- | --- |
| Usual | 320 |
| Narrowest | 288 |
| Widest | 640, or less when the window is small: the table always keeps at least 400 |

In the Deck theme the track detail is a strip under the table, and you drag its top edge to change its height in the same way. The waveform grows with the strip, and a taller strip lists more mixes. The strip is 180 high to begin with, and goes from 142 to 420, always leaving at least 200 for the table.

A size you chose in a large window is kept when the window gets smaller. It is only drawn smaller while the window has no room for it.

## Listen to a track

Select a track and press Space, or click the round play button, to hear it. Press Space again to pause. Playing starts at once, however long the track is, and carries on while other tracks are being analyzed. Space types a space as usual while you are in the search field.

- Selecting another track while one is playing switches to it. While paused or stopped, selecting a track only gets it ready.
- The position and the length are shown as "2:23 / 6:48".
- When a track reaches its end it stops there. Play starts it again from the beginning.

In most themes the player is a bar along the bottom of the window, with the play button, the track, the position, a thin progress line and the length. In the Deck theme the play button and the position sit in the strip under the table, beside the wide waveform.

If a track cannot be played, the player says why in a few words, for example "This file is no longer there" or "No audio output to play through", and everything else keeps working.

## Decks

Two decks under the table let you try a transition before the set: **Decks** in the toolbar, **View > Decks** or Command Shift D shows them, and the same again hides them and stops their sound. While they are on screen they take the sound, so the player pauses. Close the decks to use the track player again.

- **Load a track**: select it in the table and click **Load selected** on deck A or deck B, or click a track in **Next on deck B** to put it on deck B.
- **Each deck** shows its key, its tempo, its waveform with a mark on every bar of its beat grid and on each cue, its energy over time, and its cues as buttons. Click or drag on the waveform to move the deck there.
- **Play** starts a deck, and **Pause** holds it. When the other deck is playing, playback is scheduled so their upcoming downbeats line up. It starts from the selected position and may wait additional bars to align them; the button says so while it waits. A cue button plays from that cue with the same alignment.
- **Sync to A**, on by default, plays deck B at deck A's tempo: a track at 125 BPM next to one at 120 plays at 96 percent of its speed. A track counted at half or double tempo is matched at that tempo. **Key lock**, on by default, keeps a deck's pitch when its speed changes; off, the pitch follows the speed as on a turntable. The deck shows its own tempo and the tempo heard, such as 125.0 → 120.0 BPM.
- **The mixer** has a volume fader for each deck and a crossfader between them. The crossfader moves equal power from one deck to the other: at the middle both play at about 71 percent, and a deck at the near end of the fader plays at full. Use the deck faders to balance the combined level. Under the mixer, the share of one core each deck takes and any frames the decoders could not fill in time are shown, for checking that playback keeps up.
- **Loop bars** 1, 2, 4, 8 and 16 loop that many bars of a deck from its next downbeat. The loop is marked on the waveform, and the lit button shows the length. The deck keeps the samples of the loop's first pass and plays them again, so every repeat is the same sound and joins the next on the beat, with no gap. Press the lit button again to leave: the loop plays to its end once more and the track carries on from there, still on the beat. Another length starts a new loop at the next downbeat. Moving the deck, playing a cue or loading a track ends the loop. Looping needs the track's beat grid, so a track whose tempo you corrected by hand does not loop.

Time stretching is done by the [Signalsmith Stretch](https://github.com/Signalsmith-Audio/signalsmith-stretch) library. Decks only line up on downbeats when both tracks have a beat grid, which is found during analysis; a track whose tempo you corrected by hand has no grid, since the grid was fitted to the detected tempo, and starts at once.

### Next on deck B

With a track on deck A, **Next on deck B** lists the tracks that would mix well after it: keys that mix with it, tempos within 6 percent, at half or double tempo too, ranked by how smooth the key move is, how close the tempo is and how well the energy fits where you want the set to go. **Energy down**, **Same energy** and **Energy up** choose that direction: up prefers a track one or two points higher, down one or two lower. **Whole library**, **Tracks shown**, **Favorites** and a button for each playlist choose where the suggestions come from: every track, the tracks the table shows, such as a folder or a search, the tracks you starred, or the tracks of one playlist. Each row gives the key, the tempo difference, the change of energy and how the keys relate. Click a row to load it on deck B.

### Narrow the suggestions

Above the list, a row of controls narrows the tracks the suggestions are chosen from. They apply to **Next on deck B** and to the [mashup ideas](mashups.md) alike, take effect as you type, and are kept for the next launch.

- **Key moves.** A button per move chooses which ones count: **Same key**, **Adjacent**, **Relative**, **Diagonal**, **Energy boost**, **Mood change** and **Semitone boost**. A lit button counts. Same key, Adjacent and Relative are lit until you change them, and the last lit button stays lit, so there is always a move to suggest. [Keys and the Camelot wheel](../concepts/keys-and-camelot.md) shows where each move goes on the wheel.
- **Tempo window.** How far a track's tempo may be from deck A's, in percent. Empty keeps 6 percent for mixing and 8 percent for mashups.
- **Tempo from ... to ... BPM.** Only tracks whose own tempo lies in the range, both ends included. While a range is set, a track without a detected tempo is left out, since it cannot be checked.
- **Energy from ... to ... of 10.** The same for the energy rating. It decides which tracks are in the pool; **Energy down**, **Same energy** and **Energy up** still order what is left.
- **Genre.** Only tracks whose genre tag contains the text, in any case, so "house" keeps Deep House and Tech House. While the field has text, a track without a genre tag is left out.

A range needs both ends. A field that does not read, such as one end of a range on its own or an energy of 11, shows what is wrong in red and leaves that part of the pool open until it reads.

**Mixes well with** in the inspector uses these controls too, so the list beside a track and the list under the decks always agree on what counts as a mix.

### Mashup mode

**Mashup** above the list under the decks swaps the tracks to play next for tracks that layer over deck A, with a key shift and stems for each, and **Export mashup…** renders the pair to a WAV file.

See [Mashups](mashups.md).

## The waveform

The waveform shows the whole track from left to right, and its colours show what is sounding:

| Colour | Band | Typical sounds |
| --- | --- | --- |
| Blue | Bass, below 200 Hz | Kick drums and bass lines |
| Amber | Middle, 200 Hz to 2.5 kHz | Vocals, chords and snares |
| White, or near black in the Paper theme | Treble, above 2.5 kHz | Hi hats and cymbals |

The three are drawn on top of one another, bass at the back, each as tall as that band is loud. A stretch with a kick is tall and blue, a breakdown without drums loses its blue and shrinks to amber, and hi hats show as light tips.

The waveform is also where you move through the track. A line marks where playing is, and the part already played is fainter. Click anywhere on the waveform to jump there, or hold the button and drag. The progress line in the player bar works the same way. Before you press play, a click sets where playing will start.

### Cue points

A row of markers above the waveform shows the track's cues, each at its place on the waveform with its hot cue letter and its name, for example "A Start" and "B Drop", and a line in the accent colour runs down the waveform at each one. When analysis places [automatic cues](../concepts/accuracy.md#cue-points), the track starts with them on the bars of its beat grid, on hot cue pads from A in order. Click a marker to play from its cue.

To change the cues:

- Drag a marker to move its cue. It lands on the nearest beat of the beat grid, or on the nearest first beat of a bar while you hold Shift. A track without a beat grid, or with a [tempo you set](#check-the-key-by-ear-and-correct-it), has no grid to snap to, and a cue stays where you drop it.
- Double click the waveform to add a cue there, or click **Add at playhead** in the **Cues** list under the waveform, or press C, to add one where playing is. A new cue is called "Cue" and a number, lands on the nearest beat like a moved one, and takes the first free hot cue pad. A ninth cue, with every pad from A to H taken, is a memory cue only.
- In the **Cues** list, click a name to rename the cue, and press Return to keep the new name. Click the letter before it to put the cue on another pad from A to H, or on none for a memory cue only: a cue already on that pad takes the one the cue leaves. Click the time to play from the cue, and the cross to delete it.
- **Undo**, or Command Z when no text field has the keyboard, takes back the last change to the selected track's cues, one at a time. **Playback > Undo Cue Edit** does the same.
- **Reset to automatic cues** forgets your cues and brings back the ones analysis placed. It can be undone too.

The first change makes the track's cues yours: the list says **by you**, and your set replaces the automatic one wherever cues are used, deleting every cue included. In `euphoria analyze --json` its `cues` are yours and `set_by_user` lists `"cues"`, and [`euphoria export rekordbox --cues`](export-to-rekordbox.md#cue-points) exports them. Your cues are kept in the [analysis cache](../reference/cli.md#corrections) apart from the analyses, like corrections: analyzing a track again, a new version of Euphoria analyzing everything again, or `euphoria cache clear` never loses them, and they follow a file that was moved, renamed or had its tags written. A [tempo you set](#check-the-key-by-ear-and-correct-it) drops the automatic cues, which sit on the grid of the detected tempo, but keeps yours.

In the Deck theme the list sits in the strip beside the waveform once the track has cues, and scrolls when they do not fit.

### The lanes under the waveform

Under the waveform, each across the same width, run the lanes that say what the track does over time. They line up with the waveform, so what is under a part of it belongs to it, and the playhead runs through all of them. The part already played is fainter.

![A waveform with two lanes under it. The Energy lane has a bar every 4 seconds, as tall as the rating, falling to 1 in a breakdown around 40 to 60 seconds and back to 10 at the drop. The Vocals lane is filled from 0:08 to 0:38 and from 1:04 to 1:44. Two labels show what pointing shows: Energy 1 at 0:48 and Vocals from 1:04 to 1:44.](../images/lanes/lanes.svg)

**Energy.** A row of bars shows the [energy curve](../concepts/energy.md#energy-curve): the energy rating from 1 to 10 of each stretch of the track, as tall as the rating, with the label **Energy** at its left. The core rates an 8 second window every 4 seconds, so each bar is one rating, and the tooltip on the label says "Energy over time, 1 to 10, rated every 4 seconds". Point anywhere on the row to read the rating under the pointer, for example "Energy 3 at 2:15". Breakdowns show as low bars and drops as a jump back to full height. Silent stretches have no bar and read "Silent". The curve is worked out from the same decoded audio as the waveform, so it appears a moment after it. When it cannot be worked out, the row says "Energy over time not available", and pointing at it shows why.

**Vocals.** A thinner row under the energy, labelled **Vocals**, is filled where there are vocals and empty where there are none. Point at a filled stretch to read where it starts and ends, for example "Vocals from 1:02 to 1:48", or at an empty one to read "No vocals from 1:48 to 2:30". It is drawn from the track's [separated stems](../concepts/stems.md), with no separate detector: the level of the vocals stem is measured in the same 8 second windows as the energy, so a vocal stretch starts and ends on the same 4 second steps as the energy bars.

![The level of the vocals stem in each window of one separated track. Windows with vocals sit between about -16 and -31 dB below full scale and windows without between about -69 and -79 dB. A threshold line at -37 dB, 20 dB under the track's own vocal level, falls in the empty space between them.](../images/lanes/vocal-threshold.svg)

A window counts as vocals when its level is within 20 dB of the level the loudest tenth of the track's windows reach, which is the track's own vocal level, and above -50 dB below full scale. Judging against the track's own level means a quietly mixed vocal and a loud one are both found, and the floor keeps the faint leak of other instruments into the stem of an instrumental from drawing as vocals. A window where vocals sound for only a second or two of its 8 seconds can fall under the line, so a stretch can start or end a step later than the first note. The levels are measured once, off the main thread, and kept in the folder of the track's stems, so the lane appears at once the next time. It is drawn for a track whose stems are separated, in this session or an earlier one.

A track whose stems are not separated has the lane empty, with the quiet link **Separate the stems to see the vocals**. Pressing it separates the track, with the progress shown as for **Separate** in the track detail; the lane fills in when it finishes. Euphoria never starts a separation by itself. **Remove stems** empties the lane again. If the vocals stem cannot be read, the lane says "Vocals not available" and pointing at it shows why.

The decks draw the same two lanes under each deck's waveform, and the link there separates the track on that deck. Lanes for the sections of a track, such as intro, breakdown, drop and outro, will join them in the same place.

### Key changes

When a track changes key, a strip under the lanes shows each stretch in its key's colour with its Camelot code, aligned with the waveform. Point at a stretch to read its key and where it starts and ends. Silences between stretches stay empty. A track that stays in one key shows no strip, since its key is in the box above. For now the strip is also left out when the track's key is none of its stretches or a stretch is shorter than 30 seconds, because such a timeline can contradict the key above it. The strip is the detector's view, so it is also hidden while you have [corrected the key](#check-the-key-by-ear-and-correct-it), and comes back when you reset it.

## Favorites

Star the tracks you reach for, and mark the transitions you know work: a track into another that you tried on the decks or played out and want to find again.

### Star a track

- In the table, the star sits before the title. It shows on the row under the pointer, and stays on a starred track. Click it to star the track or take the star off; the click does not change the selection.
- Press F, or choose **View > Star or Unstar Selection**, to star the selected tracks. When all of them are starred already, F takes their stars off. F types a letter when a text field has the keyboard.
- The right click menu of a row has **Star** or **Remove star**, for the selected tracks.
- The star at the top of the inspector, beside the track's name, stars the selected track.
- Each deck has a star beside its track's name.

**Favorites** in the sidebar shows the starred tracks, in the order you starred them. Search and sorting work there as in a playlist.

### Favorite transitions

A favorite transition is one track into another: playing the second after the first mixes well. It has a direction, so a transition from A into B is not one from B into A, since a mix that works one way often does not work the other (an energy lift played backwards is a drop).

- On the decks, with a track on each, **Save transition A → B** under the decks marks the transition from deck A into deck B. Click it again to take the mark off. Once it is marked, click **Add a note** beside it to write how the mix goes, for example where to bring the second track in, and press Return. An empty note takes the note away.
- In **Mixes well with** in the inspector, the star at the start of a row marks the transition from the selected track into that one.

Marked transitions come first in **Mixes well with**, in the order you marked them, with a filled star, and their note shows when the pointer rests on the row. A marked transition is listed there even when its keys do not mix by the rules, since you found that they do; its relation then reads **Your mix**.

In **Next on deck B**, a marked transition out of the track on deck A moves up the ranking: it counts as much as four steps of key relation, so a mix you know beats one that is only smoother on paper, unless its tempo or energy is far off. A filled star marks it. It still has to mix in key and tempo to be suggested there.

### Export your favorites

Over the starred tracks, **Export to rekordbox** and **Export M3U8** ask where to save them and write them as a playlist named Favorites, in the order you starred them, the same way a [playlist export](#export-a-playlist) does. The line also counts your favorite transitions. Transitions and their notes are not part of the export: rekordbox XML and M3U8 have no place for them.

### Where favorites are kept

Favorites are kept with the playlists in `playlists.sqlite3`, so clearing the analysis cache never touches them. Like a playlist entry, each starred track and each side of a transition is kept by the content of its file and the path it was last seen at: a favorite survives a move or a rename once the app has seen the file in its new place, and follows Euphoria's own tag writes and undo, the way corrections and edited cues do.

## Write tags

The app can write the key, tempo and energy it found into the files themselves, with the same safety as [`euphoria tags write`](write-tags.md): you see every change before anything is written, and one step puts the old tags back.

![The Write tags preview over the window, for rekordbox. It says keys are written in Camelot, the energy label at the start of the comment and the tempo, and that nothing is written until you confirm. Buttons choose between the 11 selected tracks and all 11 shown. A list gives each file with its changes, such as TKEY from none to 8A, TBPM from none to 125, and the comment from "Purchased at Beatport.com" to "8A - Energy 5 - Purchased at Beatport.com". Cancel and Write 11 files buttons close it.](../images/desktop-app/write-tags-preview.png)

1. Select the tracks to write, or choose a folder in the sidebar.
2. Click **Write tags** in the toolbar, or choose **Tags > Write Tags…** (Command Shift T). The DJ program the tags are written for is shown beside the button.
3. The preview lists every file that would change, each field with the value it has now and the value it would get. **Selected tracks** and **All shown** switch between the selection and every track the table shows, such as a whole folder. Files that already hold the values, tracks not analyzed yet and files that cannot be written safely are counted or listed with the reason.
4. Click **Write** to write them. Nothing is written before that, and **Cancel** or Escape closes the preview without writing.
5. The app shows how far it has got, then the files written and any it could not write, with the reason. Those are left as they were.

**Undo this write**, **Tags > Undo Last Tag Write…** or the button in the settings puts back the exact tags the last write replaced, after asking. The app and `euphoria tags undo` share their backups, so either undoes a write the other made.

**Cue markers**: with the **Cue markers** setting in [How tags are written](#settings) on, the write also adds each track's cues and beat grid where Serato DJ, Mixxx and VirtualDJ read them. The cues you edited in the app go in on the pads you gave them; memory cues, ones on no pad, are left out. The preview shows the cue points and beat grid each file gets, and below it lists the files where a cue went to another pad, where cues were left out, where your own Serato beat grid is kept, or where a file gets no Serato data. Those files still get their key and other tags. **Undo this write** puts these back byte for byte, Serato fields included. [Cue points and beat grids for Serato](serato-cues.md) explains what is written and how to bring it into Serato.

A key or tempo you corrected is written in place of the detected one. Each file written, and each file put back by an undo, is recorded in the [analysis cache](../reference/cli.md#the-analysis-cache) as it is, so it is not analyzed again the next time the app starts, and its corrections stay with it.

## Playlists

Playlists are where you build a set: a folder holds the tracks you own, a playlist holds the order you will play them in.

- **Make one** with the **+** beside **Playlists** in the sidebar, under the folders, then type its name and press Return. Right click tracks in the table and choose **New playlist from selection** to start one from them.
- **Add tracks** by dragging them from the table onto a playlist in the sidebar, or with **Add to playlist** in their right click menu. A track is in a playlist once; adding it again changes nothing.
- **Rename** a playlist by double clicking it, or from its right click menu. The menu also gives it a **Label**, one emoji to tell it apart, and **Delete playlist**, which leaves the tracks in the library.
- **Reorder playlists** by dragging them up or down the sidebar.

Click a playlist to show its tracks in the table in their order, with their place in the **#** column. Drag rows to move them; a row dropped on another goes just above it, and a whole selection moves together. **Remove from playlist** in the right click menu takes tracks out. Search and sorting work as everywhere else, but sorting is only a view: the playlist keeps its order, and clicking the **#** header goes back to it. While the table is sorted or searched, rows cannot be dragged to a new place.

### A set at a glance

Above a playlist's table, a line shows it as a set:

- **The key path**: each track's key in play order, in the notation you chose. A `›` between two keys is a [compatible move](../concepts/keys-and-camelot.md); a red `×` marks a step that is not, the place to look for a smoother order or a key change you meant.
- **The tempo range**, from the slowest track to the fastest.
- **The energy flow**: a small line of each track's [energy](../concepts/energy.md) in order, rising from 1 to 10.

The count beside the name says how many of the playlist's tracks are not in the library, for example after their folder was removed; they come back when the folder is added again.

### Export a playlist

The **rekordbox** and **M3U8** buttons after **Export**, at the end of the line above the table, or the playlist's right click menu, ask where to save the playlist and write it there:

- **rekordbox** writes a rekordbox XML file holding the playlist's tracks and one playlist in its order, with their keys, tempos and energy, as [`euphoria export rekordbox`](../reference/cli.md#euphoria-export-rekordbox) does. [Export to rekordbox](export-to-rekordbox.md) explains the import.
- **M3U8** writes the playlist for nearly any other program: Serato, Traktor, Engine DJ, VirtualDJ, Mixxx or a music player. It holds the tracks' paths, lengths and names, not their keys; [write the tags](#write-tags) for those programs to see the keys.

Tracks not analyzed yet are left out of a rekordbox export, and the app says how many were. Every file is read and compared with the track that was added before it is exported, so a file replaced at the same path by other audio is left out too, with a note saying so.

### Where playlists are kept

Playlists are kept in `playlists.sqlite3`, beside the analysis cache in the [data folder](#what-the-app-remembers). Each track is kept by the content of its file, like a remembered result, together with its path. A file moved or renamed is found again once the app or `euphoria analyze` has seen it in its new place, and only when the file there holds the same content. Euphoria's own tag writes and undo keep playlist identity in step with the rewritten file; an unverified external replacement is still left out of exports. Playlists live in their own file so that clearing or deleting the cache never touches them. The command line lists them with [`euphoria playlists`](../reference/cli.md#euphoria-playlists) and exports one by name.

## Build a set

**Set builder** in the sidebar, **View > Set Builder** or Command Shift B (Control Shift B on Linux) opens a page that orders tracks into a set along an energy curve, with every mix in a compatible key and small tempo steps. It is the same search as [`euphoria set build`](build-a-set.md), which explains how it chooses.

1. **Draw the curve.** Drag across the chart: higher is more energy, from 1 at the bottom to 10 at the top. The stroke is smoothed when you let go. **Warm up**, **Peak time** and **Journey** put a preset curve back.
2. **Choose the pool.** All tracks, one of the folders you added, or a playlist. The pool starts as what the library showed when you opened the page.
3. **Set the length** with **−** and **+**, from 2 to 60 tracks, then **Build set** or press Command Return (Control Return on Linux). The length starts at 20, or at the size of the pool when it is smaller.

The chart then draws the energy of each track of the set over the curve, with tracks more than a point away in red. Under it, a summary gives the length of the set, how close it follows the curve and whether every mix keeps the rules, then one line for each place it falls short. Each row of the set shows the key move and tempo step from the track before, and the track's energy against the energy the curve asks for there.

- **Lock** keeps a track where it is. **Build around locked tracks** fills the rest of the set again, for a new curve, length or pool, around the tracks you locked.
- The arrows move a track one place, and **Remove** takes it out. The set is explained again at once: the moves, the steps and the curve follow your order.
- **Save as playlist** keeps the set as a playlist named after its curve, such as "Set: Journey, 20 tracks", in the sidebar with the others. **Export M3U8…** saves it first if needed, then asks where to write the file.

A pool with more than 4,000 tracks that have a key, a tempo and an energy is refused: choose a folder or a playlist.

## Edit tags

Select one track or thousands and click **Edit tags** in the toolbar, or choose **Tags > Edit Tags…** (Command I), to edit their title, artist, album, genre, year, track number, comment, grouping, key and BPM fields.

- Each field shows the value the selected tracks share. When they differ, it is empty, shows **mixed** and is marked **differs**; leave it empty to keep each track's own value, or type to give them all one.
- Type in a field to change it. Emptying a field that has a value removes it, and **Remove** beside a field removes it from every track; click **Keep** to take that back.
- **Change text in one field** works on one field at a time: **Find and replace** (with **Match case** for capital letters), **Add prefix**, **Add suffix**, **Remove prefix** and **Remove suffix**. Choose the field under the tool and type the text.
- **Preview changes** shows the same preview as [writing tags](#write-tags): each file with each field's value now and the value it would get. Click **Write** to write them, or **Cancel**.

The edit is written with the same safety as a tag write and shares its backups, so **Undo this write**, **Tags > Undo Last Tag Write…** and `euphoria tags undo` put the old tags back. The titles and artists in the table follow the edit. A comment or a grouping keeps the key and energy label a tag write put there, as [`euphoria tags set`](../reference/cli.md#euphoria-tags-set) explains, and the comment field shows only your own text. The key and BPM fields are the tags in the files; the key and tempo Euphoria detected stay as they were.

## Export the library

**File > Export Library...** opens a sheet that exports every track of the library, or only the tracks the table shows, to a DJ program. The analysis, corrections and cues you edited in the app are what is exported; nothing is analyzed again.

1. Pick the program: rekordbox (an XML file to import), Traktor (an NML file to import), Engine DJ or VirtualDJ (the program's own database, edited in place), or any program (an M3U8 playlist).
2. Pick the tracks: the whole library or the tracks shown.
3. Set the options the program has, as in `euphoria export`: key notation, the beat grid, cues, a playlist per folder (rekordbox and Traktor), and where the key and energy label goes (Engine DJ and VirtualDJ).
4. Choose the file to write, or the Engine Library folder or VirtualDJ `database.xml` to edit. The app proposes the ones it finds in your home folder.
5. **Review** checks everything and shows what would be written. It writes nothing. **Export** then writes, and the result appears in the notice strip.

For rekordbox and Traktor, cues need the beat grid and your own collection file (rekordbox: File > Export Collection in xml format; Traktor: `collection.nml`). Tracks that already have cues there are left out of the file so importing it cannot change them, and the export is refused when cued tracks of that collection match no file on this computer, unless you tick the box that allows it. The file is never written over your Traktor collection.

Engine DJ and VirtualDJ keep every safety of the command line, because the app calls the same code: a copy of the database is saved first and its path is shown in the review, the export is refused while the program is running, an Engine DJ library whose schema version Euphoria was not checked against is refused, and tracks with cues of their own in the program keep them.

The export runs off the main thread, but it has no per-track counter: the sheet says whether it is checking or writing and cannot be closed meanwhile.

The headless interface tests cover the sheet opening, a rekordbox export of a small fixture library, and the refusal of an Engine DJ database of an unknown version or while the program runs. The Engine DJ and VirtualDJ writes themselves are tested in the core, not against the real programs, and the file dialogs have not been checked by an automated test.

## Settings

Open the settings with **Euphoria > Settings…** (Command comma), or click the program name beside **Write tags**.

How keys and tempos are shown:

| Setting | Choices |
| --- | --- |
| Keys | Camelot (8A), musical (Am) or Open Key (1m), in the table, the track detail, the mixes and the key timeline. The wheel keeps Camelot codes. Search finds a key in any of the three. |
| Tempo range | House and techno, 90 to 180 (the default); hip hop, 70 to 140; drum and bass, 120 to 200; or a custom range of whole beats per minute, at most an octave wide, such as 80 up to 160. A track counted at half or double speed moves into the range. Nothing is analyzed again, and tags are written with the tempo as shown. A custom range that cannot be used is explained under it, and the last range that worked stays in use. [Choosing the range](../concepts/accuracy.md#choosing-the-range) says more. |

How tags are written:

| Setting | Choices |
| --- | --- |
| DJ program | rekordbox, Serato, Traktor, Engine DJ, VirtualDJ, Mixxx, or any program. Tags are written to the fields that program reads; [Use Euphoria with your DJ program](dj-programs.md) lists them. |
| Key notation | Camelot (8A), musical (Am) or Open Key (1m), in the key field and in the label. It is separate from how keys are shown, so the tags can follow what your DJ program expects. |
| Energy label | At the start of the comment, at its end, at the start of the grouping, or nowhere. |
| Tempo | Whether the tempo is written. Choosing Serato unticks it, because Serato detects the tempo itself only for files that have none; you can tick it again. |
| Cue markers | Whether each track's cues and beat grid are written as Serato cue points, which Serato DJ, Mixxx and VirtualDJ read. Off by default. Your edited cues go in on their pads; memory cues are left out. WAV and Ogg files get none. |

## When a file fails

A file that cannot be analyzed stays in the table with **Failed** in its key column and is counted under **Needs review**. Select it to read the reason in the inspector. The other files carry on. A file that is no longer there is not a failure; see [Missing files](#missing-files).

## Missing files

A track is missing when its file is no longer where it was: moved, deleted, or on a drive that is not connected. When the app opens, each track of a playlist or a favorite whose file is gone is first looked for where the analysis cache last saw the same audio, and followed there. A track found nowhere is added to the library as missing. A file that disappears while the app runs becomes missing too, not failed.

A missing track shows **Missing** in its key column and is listed under **Missing files** in the sidebar, not under **Needs review**. The inspector says where the file was.

To point a missing track at where its file is now, right click it and choose **Locate…**, then pick the file. Euphoria reads the file you picked and compares its audio with what the track held. When they match, the track, its playlists, its stars and favorite transitions, and the key, tempo and cues you set follow the file, and its analysis comes back from the cache without analyzing again. Every other missing track whose file sits in the same folder under the same name, with the same audio, is linked too, so locating one track of a moved folder finds the rest. When the picked file holds other audio, nothing changes and a notice says so. When nothing was recorded about the track's audio, Euphoria cannot check the file and asks first: **Link anyway** links it, **Cancel** leaves the track missing.

**Remove from Library** in the same menu takes the missing tracks of the selection out of the library, out of every playlist and out of the favorites. Tracks whose file is there are left alone. Until you remove a missing track, its playlists, favorites and edited cues keep it, so unplugging a drive loses nothing.

A remembered folder that is not there when the app opens is named in the notice and stays remembered: it is searched again at the next launch, and its tracks come back then.

The command line reports missing files the same way, on [`missing:` lines](../reference/cli.md#messages) with exit status 3.

Limits: there is no way yet to forget a remembered folder, and a deleted file of a remembered folder that is in no playlist or favorite is not shown as missing; it is simply not found when the folder is searched.

## Themes

Choose a theme from the menu at the right of the toolbar, or from **View > Theme**.

| Theme | Looks like |
| --- | --- |
| Slate | Dark grey with a blue accent. The default. |
| Paper | Light with a blue accent. |
| Deck | Slate colours, with the track detail and the player in a strip under the table and a wide waveform. |
| Graphite | Warm dark grey with an amber accent, compact rows, and keys as coloured text. |
| Key first | Slate colours with solid key chips, a Camelot wheel in the inspector, and the table sorted by key. The wheel has the minor keys (A) on its inner ring and the major keys (B) on its outer ring; the selected track's key is filled in the accent colour and the keys that mix with it, its neighbours and its relative major or minor, in a lighter tint. |

![The Paper theme: the same window on a light background, with pastel key chips, the house folder chosen in the sidebar and a track selected in the inspector.](../images/desktop-app/main-window-paper.png)

![The Key first theme: the table sorted by key from 1A to 12A, with solid key chips. The inspector shows the selected track in 5A with a Camelot wheel under its waveform, where 5A is filled in the accent colour and 4A, 6A and 5B, the keys that mix with it, are tinted.](../images/desktop-app/key-first.png)

## What the app remembers

Playlists and favorites are kept in `playlists.sqlite3`, as [Where playlists are kept](#where-playlists-are-kept) and [Where favorites are kept](#where-favorites-are-kept) explain. The folders you added, the theme, whether the sidebar is folded, the sizes you gave the inspector and the Deck strip, the folders you opened in the tree, the folder on show, the column widths you chose, how keys and tempos are shown and the tag writing settings are kept in `settings.json`. The backups of tag writes are kept in the `tag-writes` folder beside it:

| System | Folder |
| --- | --- |
| macOS | `~/Library/Application Support/euphoria` |
| Linux | `$XDG_DATA_HOME/euphoria`, or `~/.local/share/euphoria` |
| Windows | `%APPDATA%\euphoria` |

Set the `EUPHORIA_DATA_DIR` environment variable to keep it somewhere else.

Files dropped on their own, outside a folder, are not remembered. If the folder that was on show is gone the next time, for example on a drive that is not connected, the app starts on **All tracks**. The folder stays remembered and is searched again at the next launch.

## Remembered results

The app keeps each finished analysis in the same folder, in the cache the command line tool also uses. A track analyzed once, by the app or by `euphoria analyze`, is not analyzed again: when the app starts, the remembered folders are searched and their known tracks are filled in within a moment. Only new or changed files are analyzed. A file that was moved or renamed is recognised by its content when its turn comes.

The count above the table says how many tracks were filled in this way, for example "48 tracks, 44 from cache".

If the cache cannot be opened, the app says why in a notice along the bottom of the window and analyzes without it, so results are not remembered until the problem is fixed. `euphoria cache clear` resets a damaged cache. The [command reference](../reference/cli.md#the-analysis-cache) explains how files are recognised and when a track is analyzed again.

## Quitting

You can quit while tracks are being analyzed. The app closes at once: tracks already finished are remembered, and the ones that were waiting or in progress are analyzed the next time the app starts.

## Build Euphoria.app

To get an application you can keep in the Dock and put in Applications, build an installable bundle on macOS:

```
./scripts/bundle-macos.sh
open target/release/bundle/Euphoria.app
```

The script builds the app in release mode, makes the icon from `assets/logo`, copies libkeyfinder and FFTW into the bundle so it runs without Homebrew, signs it with an ad hoc signature, and writes `target/release/bundle/Euphoria.app` and a disk image, `Euphoria-<version>.dmg`, next to it. [Installation](../getting-started/installation.md#the-desktop-app) covers installing it and [opening it for the first time](../getting-started/installation.md#opening-the-app-for-the-first-time). Releases on GitHub carry the same disk image, built by CI.

## Try it without music

To see how the interface behaves with a large library, start the app with made up tracks. Their keys and tempos are invented and nothing is analyzed:

```
EUPHORIA_DEMO_TRACKS=10000 cargo run -p euphoria-desktop
```

## About the screenshots

The screenshots on this page and in the README show a demo library of GiantSteps Key previews, the clips the benchmarks use, never anyone's own music. [`make_screenshots.py`](https://github.com/lvndry/euphoria/blob/main/docs/images/desktop-app/make_screenshots.py) builds that library, runs the app on it and captures the window again when the app changes.

## Not there yet

- On the decks: mashups of more than two tracks, favorite transitions, nudging, pitch bend, and loops shorter than a bar or moved by hand.
- Rating favorites (more than one star), and exporting favorite transitions with their notes.
- Loops, and cue colours.
- Hearing the reference tone and piano has not been checked by an automated test, which can only check that their sound is made and mixed: listen for yourself and report anything off.
- Windows builds. On Linux the app is built, linted and its headless interface tests run on every change, and each release's AppImage is started on a clean Ubuntu 22.04 under a virtual X server, but playback, the file dialogs and a real Wayland session are not checked by an automated test there yet. Linux has no menu bar: the theme is chosen from the toolbar, but **Reset Column Widths** has no other control there yet. The keyboard shortcuts use Control where macOS uses Command.
