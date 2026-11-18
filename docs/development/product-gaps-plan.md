---
title: Plan for the product gaps
description: How Euphoria fills the gaps found in the Mixed In Key research, in priority order.
---

# Plan for the product gaps

Written on 18 November 2026 from a comparison with Mixed In Key. Items are in priority order. The first four make the decks the place where a DJ chooses and tries the next track, which is where Mixed In Key Pro is strongest and where Euphoria's transition planner is not yet visible. Sizes are rough: small is a day or two, medium about a week, large more than that.

Paths without a crate are under `apps/euphoria-desktop/src/`.

## Already done or in progress

- Opening the decks from the focus view now leaves the focus view and shows the decks (pull requests 254 and 255).
- The decks can be resized by dragging their top edge, giving the track table more rows (branch `feat/resizable-decks`).
- Tracks can be dragged from the table onto deck A or B (pull request 256).

## 1. Show the best transition window on the decks

**Outcome.** With a track on each deck, both deck waveforms show where to mix: the bars where A goes out, where B comes in, and where the bass swaps. The other windows the planner found are markers you can click to switch.

**How it fits.** The engine exists: `plan_transition` in `crates/euphoria-core/src/harmony/transition/mod.rs` scores windows, and `transition_plan.rs` computes plans in the background, caches them (`last_ready`, `plan_state`) and exposes windows (`window`, `window_at`). `preview_on_decks` in `transition_plan.rs` already loads a planned pair onto the decks, but carries only cues across. The work is to request the plan for the pair on the decks whenever either deck loads, and draw the window in the deck waveforms in `views/decks.rs`. `PlanTiming` in `planner_playback.rs` converts bars to seconds on each track (`a_seconds_at`, `b_seconds_at`).

**Size.** Medium.

**Depends on.** Nothing. Plans for a pair whose stems are not separated yet have no clash check; the window still shows, marked as not checked.

**Verify.** A UI test loads two planned tracks and checks that the window is drawn at the bars the plan gives. By hand: load two tracks that mix and see the window on both waveforms.

## 2. A transition verdict for each "Next on deck B" suggestion

**Outcome.** Each suggestion shows whether its best window is clean, has vocals or bass overlapping, or is not checked yet, so suggestions can be judged by how the mix will sound, not only by key, tempo and energy.

**How it fits.** The focus view already does this for its list of matches: `clash_label` in `transition_plan.rs` turns a plan into a label with `clash_verdict`, and `request_plans` computes plans for the rows in view, a few at a time. The suggestion list (`suggestion_indexes` and `refresh_suggestions` in `decks.rs`) needs the same requests, with deck A as the from track, and a column in `suggestion_rows` in `views/decks.rs`. Optional second step: sort clean windows first.

**Size.** Small to medium.

**Depends on.** Item 1 shares the plan requests.

**Verify.** A UI test with stubbed plans checks the label per row. A check that scrolling the list requests plans only for rows in view.

## 3. "Test transition" on the decks

**Outcome.** One button plays A from a few bars before the best window into B, with the planned fades, then leaves both decks playing.

**How it fits.** `play_plan` in `planner_playback.rs` already plays a window through the decks with the planned gains (`gains`), and `stop_plan_playback` ends it. The button calls it with the window from item 1. The planner's own transport stays as it is.

**Size.** Small.

**Depends on.** Item 1.

**Verify.** A UI test presses the button and checks both decks start at the planned positions, the same way the planner's playback tests do.

## 4. "Move B to A" and "Add to playlist" from the decks

**Outcome.** After a transition, one click moves deck B's track to deck A, clears B and refreshes the suggestions. Another adds the track to a chosen playlist, so a set is built while auditioning.

**How it fits.** Move B to A is `load_on_deck(DeckId::A, b_index, cx)` followed by clearing B and `refresh_suggestions` in `decks.rs`, keeping A's position where B was. Adding to a playlist uses `add_to_playlist` in `playlist_flow.rs`; the decks need a playlist picker, which the sidebar's playlist menu can provide.

**Size.** Small.

**Depends on.** Nothing.

**Verify.** UI tests for both buttons: the deck contents after the move, and the playlist contents after the add.

## 5. Stems on each deck in mix mode

**Outcome.** Each deck has the full, acapella and instrumental switch in the next track mode too, not only in mashup mode.

**How it fits.** `set_deck_stems` in `mashup.rs` already plays a deck's stems at chosen levels and reopens the sound in time. The switch in `views/mashup.rs` moves into the deck element in `views/decks.rs` for both modes.

**Size.** Small.

**Depends on.** Separated stems for the track (`stems.rs`), as today.

**Verify.** A UI test switches a deck to instrumental in the next track mode and checks the stem levels sent to the deck.

## 6. Cues and energy that arrive in the DJ program

**Outcome.** Exports work in every supported program without manual steps, each one checked in the program itself, and energy no longer depends on the comment field, which users overwrite.

**How it fits.** The writers are in `crates/euphoria-core/src/export/` (`rekordbox.rs`, `traktor.rs`, `engine.rs`, `virtualdj.rs`) and the tag writers in `crates/euphoria-core/src/tags/`. Today only rekordbox has been tested in the program, and only for key, tempo and the comment: beat grids and cues in rekordbox, and every route into Traktor, Engine DJ, Serato and VirtualDJ, are checked by Euphoria's own tests only. [DJ program support](../reference/dj-program-support.md) keeps that status per route and is the place to update as each one is tested. Every route writes energy into the comment.

Steps:

1. Test each route in its program with a fixed test kit, as was done for rekordbox in [Use Euphoria with your DJ program](../guides/dj-programs.md), starting with beat grids and cues in rekordbox, then Traktor and Serato. This needs access to each program.
2. For each program, find a field other than the comment that it shows and keeps (for example a rating, color or grouping field, or a custom tag), and write energy there, keeping the comment as an option.
3. Remove manual steps where the program allows it, and document the remaining ones in each export guide.

**Size.** Large, mostly testing time.

**Depends on.** Access to Traktor, Serato, Engine DJ and VirtualDJ.

**Verify.** Each route's row in DJ program support turns to tested, with the program version and date.

## 7. Energy DJs trust, in the suggestions

**Outcome.** The deck suggestions show each track's energy on Euphoria's scale with a short description of what the number means, and its set stage, so the Energy up, Same energy and Energy down buttons explain themselves.

**How it fits.** The scale is described in [energy rating](../concepts/energy.md) and computed in `crates/euphoria-core/src/energy/`. The set stage comes from `crates/euphoria-core/src/set_stage.rs` and is already a filter in the track table. The suggestion rows (`suggestion_rows` in `views/decks.rs`) show an energy pill with the difference from deck A today, and no stage.

**Size.** Small.

**Depends on.** Nothing.

**Verify.** A UI test checks the stage and the energy hint on a suggestion row.

## 8. Downbeat accuracy

**Outcome.** Bars, cues and transition windows land on the real first beat of the bar far more often.

**Why it matters.** The [accuracy page](../concepts/accuracy.md#the-downbeat) measures the downbeat right on 46% of the 100 held out benchmark excerpts with the tempo as reported, while the grid's beats are right on 84%. When it is wrong, the bars start one to three beats off, so every cue snapped to a bar and every transition window built on bars is off by the same amount. Items 1 to 3 inherit this. The same page notes that telling the first beat from the third causes most misses.

**How it fits.** The grid and downbeat are found in `crates/euphoria-core/src/detect/`. The benchmark in `crates/euphoria-bench/` measures it, so each change can be scored against the 46%.

**Size.** Large, research.

**Depends on.** Nothing, but it improves items 1 to 3.

**Verify.** The downbeat column of the benchmark table rises without the beat F-measure or the share of grids right going down.

## Not planned

- Pitch correction like Platinum Notes. `prepare_files.rs` covers loudness and true peak, which matter more for playing evenly on a club system.
- Stability on large libraries has no evidence either way yet. A benchmark with a library of tens of thousands of tracks would be the first step, before any work.
