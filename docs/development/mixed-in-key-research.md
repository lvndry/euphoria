---
title: Mixed In Key research
description: What Mixed In Key does, what its users complain about, and how Euphoria compares, as of 18 November 2026.
---

# Mixed In Key research

Researched on 18 November 2026. Mixed In Key 11 Pro was the current version. The [plan for the product gaps](product-gaps-plan.md) turns the findings into work.

Sources were Mixed In Key's own pages, Trustpilot, the Algoriddim and Native Instruments forums, reviews, and Mixed In Key's FAQ and guides. Reddit, the Pioneer DJ forums and the old Mixed In Key forum could not be fetched, so the complaints below miss those communities. Each claim says whether a source confirms it or whether it is inferred.

## What Mixed In Key does

Confirmed by [mixedinkey.com/pro](https://mixedinkey.com/pro) and [What is Mixed In Key 11 Pro](https://mixedinkey.com/workflows/what-is-mixed-in-key-11-pro/):

- The Standard edition finds the key, the BPM and an energy level from 1 to 10, and places cue points automatically.
- Pro adds two deck players, Mashup mode, DJ Mix mode, an Idea Filter, stems, pitch shifting and favorite mashups.
- DJ Mix mode suggests the next song by key, energy and BPM. "Test DJ Mix" plays both decks in sync. "Move to Deck A" promotes deck B and refreshes the suggestions. "Add to Playlist" chains a set as you go.
- Clicking a cue point plays its segment beatmatched with the track on the other deck. Loops extend or shorten a mix.
- Cue points mark intros, breakdowns, drops and mix-out sections. A third party source says there are up to 8 cues, each with its own energy level.
- Companion products: Mixed In Key Live (the key of whatever audio is playing), Mashup, Platinum Notes (mastering) and DJ.Studio.

### Does it show where to do a transition?

Not as a computed window, as far as the sources show. Neither official page describes a suggested mix-in or mix-out window. The "where" in Mixed In Key is its cue points (intro, outro, mix-out) and beatmatched cue playback. This is inferred from the docs not describing such a feature. Euphoria's [transition planner](../concepts/plan-a-transition.md) already scores mix windows bar by bar, but only the focus view shows them.

## Feature comparison

| Mixed In Key feature | Euphoria | Where in Euphoria, or what is missing |
| --- | --- | --- |
| Key, BPM, Camelot | Has | `crates/euphoria-core/src/key.rs`, `harmony/`, `detect/` |
| Energy 1 to 10, energy per cue | Has | `crates/euphoria-core/src/energy/`, [energy rating](../concepts/energy.md), cue energy in `views/cues.rs` |
| Automatic cues (intro, drop, outro) | Has | `crates/euphoria-core/src/track_cues.rs`, edited in `cue_editing.rs` |
| Two decks, sync, key lock, crossfader, loops | Has | `decks.rs`, `views/decks.rs`, `deck_audio.rs` |
| Cue jump beatmatched to the other deck | Has | `play_deck_cue` in `decks.rs` |
| Next track suggestions and Idea Filter | Has | "Next on deck B" in `views/decks.rs`, filters in `decks.rs` |
| Mashup mode, stems, key shift | Has | `mashup.rs`, `views/mashup.rs`, `stems.rs`, `key_shift.rs` |
| Favorite mashups and transitions | Has | `favorites.rs`, `views/favorites.rs` |
| Where to transition | Has, focus view only | `transition_plan.rs`, `views/planner.rs`, `planner_candidates.rs`, `planner_playback.rs`. The decks show only cues, and only when opened from the planner |
| "Test DJ Mix" in one click | Partial | The planner plays a transition (`planner_playback.rs`); the decks have no such button |
| "Move to Deck A" | Missing | Nothing in `decks.rs` promotes deck B |
| "Add to Playlist" from the decks | Missing | Playlists exist (`playlists.rs`, `playlist_flow.rs`) but not from the decks |
| Stems in DJ Mix mode | Partial | Deck stems switch only in mashup mode (`set_deck_stems` in `mashup.rs`) |
| Mixed In Key Live | Has | `live_app.rs`, `views/live.rs` (macOS) |
| Platinum Notes style loudness | Partial | `prepare_files.rs` and `crates/euphoria-core/src/prepare/`: loudness and true peak, no pitch correction |

Paths without a crate are under `apps/euphoria-desktop/src/`.

## What users complain about

Counts are the distinct sources seen, not a survey.

### Energy levels are not useful

Four sources, confirmed. Users describe the energy levels as "all over the place". One DJ deleted the values after years of use ([Algoriddim community, November 2025](https://community.algoriddim.com/t/mixed-in-key-djay-pro-is-it-worth-it-for-energy-tagging/40191)). Undated forum snippets say ballads rate 6 or 7 and whole libraries sit between 6 and 8.

Euphoria: could avoid it. Its scale is documented and measured against listeners ([energy rating](../concepts/energy.md)), it draws an energy curve, and the [set stage](../concepts/set-stage.md) rates a track relative to your own library. None of that shows in the deck suggestions yet.

### DJ software integration is fragile

Six sources, confirmed. Mixed In Key's own guide needs an overwrite option, a separate XML import and Traktor closed ([export cue points to rekordbox](https://mixedinkey.com/workflows/export-cue-points-to-rekordbox/)). Users report cues missing in Traktor 4. Energy is kept in the comment field and lost when the comment is edited ([an issue on another DJ tool](https://github.com/geovannimp/mixar/issues/736)).

Euphoria: shares the risk. Only rekordbox has been tested in the program itself, and only for key, tempo and comment; see [DJ program support](../reference/dj-program-support.md). Euphoria also writes energy into the comment field for every program.

### Cue placement

Four sources, confirmed. Automatic cues land off the beat ([Trustpilot, January 2026](https://ie.trustpilot.com/review/mixedinkey.com)). Deleted cues came back in djay (February 2022).

Euphoria: partly avoids it. Cues snap to the grid and can be edited with undo (`cue_editing.rs`). But the [downbeat is right on only 46% of the benchmark excerpts](../concepts/accuracy.md#the-downbeat), so a cue snapped to a bar can sit one to three beats off. That is the same root cause.

### Key accuracy

Five sources, opinion split. Reviews called it inaccurate in October 2025 and April 2026 ([Trustpilot](https://ie.trustpilot.com/review/mixedinkey.com)), while djay users praise it.

Euphoria: could avoid it. It shows a runner-up key, has a Needs review queue (`crates/euphoria-core/src/review.rs`), a check by ear (`key_by_ear.rs`) and a published [accuracy](../concepts/accuracy.md) page.

### Licensing, platforms and price

Eight sources, confirmed. Moving from Windows to Mac needed a second license (May 2026). Activations failed (October 2025). Installs are capped at three computers ([FAQ](https://mixedinkey.com/faq/mixed-in-key/)). Pro at $99 was called a set of "deal-sweeteners" ([Digital DJ Tips, March 2025](https://www.digitaldjtips.com/mixed-in-key-11-pro/)). Users find the upgrade prompts pushy.

Euphoria: avoids all of it. It is open source, works offline, needs no account, and runs on macOS and Linux.

### Stability on large libraries

Four Trustpilot reviews from 2025 and 2026 report freezes and a corrupted database.

Euphoria: unknown. Nothing measures it yet.

### Pro tools stop at auditioning

[Digital DJ Tips, March 2025](https://www.digitaldjtips.com/mixed-in-key-11-pro/): you still build the actual mix in another program.

Euphoria: avoids it. It renders a whole set as one mix (`mix_render.rs`, `crates/euphoria-core/src/set_render/`) and keeps the transitions you locked (`crates/euphoria-core/src/locked_transitions.rs`).

## Where Euphoria could do better than Mixed In Key

In order of how much they would set Euphoria apart. The order is a judgment, not a finding.

1. Transition windows on the decks, scored on vocal and bass clashes from the stems. Mixed In Key's pages describe nothing like it.
2. Energy DJs trust: the documented scale, the curve and the set stage, shown where the next track is chosen.
3. Cues that arrive in the DJ program without manual import steps, checked in each program, with energy kept out of the comment field.
4. Free, offline, on more than one platform, with no license limits. Already true.
5. From preparing tracks to a finished mix in one program. Already true.

## Confirmed and inferred

- Confirmed by sources: the feature list, the Pro deck actions, and the complaints with their dates.
- Inferred: that Mixed In Key computes no transition windows (its docs do not describe any), the order of the opportunities, and how often each complaint comes up beyond the sources read.
