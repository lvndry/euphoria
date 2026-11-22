// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 17 tests: the decks, the suggestions, the loops and the transport.

/// The core's fixture files as three tracks with keys, tempos, energies,
/// beat grids and cues: A minor at 120, E minor at 125 and C major at 118.
pub(crate) fn deck_tracks() -> Vec<DemoTrack> {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/euphoria-core/tests/fixtures");
    [
        (
            "sine-440hz-mono.flac",
            9,
            euphoria_core::Mode::Minor,
            120.0,
            6,
        ),
        (
            "sine-440hz-mono.mp3",
            4,
            euphoria_core::Mode::Minor,
            125.0,
            7,
        ),
        (
            "sine-440hz-stereo.wav",
            0,
            euphoria_core::Mode::Major,
            118.0,
            5,
        ),
    ]
    .into_iter()
    .map(|(name, tonic, mode, bpm, energy)| DemoTrack {
        path: fixtures.join(name),
        outcome: Ok(TrackAnalysis {
            duration_seconds: 240.0,
            key: Some(euphoria_core::Key::new(tonic, mode)),
            bpm: Some(bpm as f32),
            energy: Some(energy),
            tuning_cents: None,
            key_runner_up: None,
            key_score_margin: None,
            key_source: None,
            loudness_range_lu: None,
            pitch_class_profile: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            tempo_octave_evidence: None,
            beat_grid: Some(euphoria_core::BeatGrid {
                first_downbeat_seconds: 0.2,
                bpm,
                salience: 3.0,
                drift_seconds: Some(0.005),
            }),
            cues: vec![
                euphoria_core::SectionCue {
                    seconds: 0.2,
                    section: euphoria_core::CueSection::Start,
                    energy: None,
                },
                euphoria_core::SectionCue {
                    seconds: 64.2,
                    section: euphoria_core::CueSection::Drop,
                    energy: None,
                },
            ],
            key_timeline: Vec::new(),
            quality: None,
        }),
    })
    .collect()
}

pub(crate) fn index_of_track(
    view: &Entity<EuphoriaApp>,
    name: &str,
    cx: &mut TestAppContext,
) -> usize {
    cx.update(|cx| {
        let library = view.read(cx).session.library();
        library
            .tracks()
            .iter()
            .position(|track| track.path.ends_with(name))
            .unwrap()
    })
}

#[gpui::test]
fn the_decks_load_a_track_and_suggest_what_mixes_after_it(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    use crate::views::decks::deck_part;
    let (window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    click(window, "toggle-decks", cx);
    cx.update(|cx| assert!(view.read(cx).decks.is_open));

    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    click(window, track(flac), cx);
    click(window, deck_part("deck-load", DeckId::A), cx);
    let suggested = cx.update(|cx| {
        let decks = &view.read(cx).decks;
        assert_eq!(decks.view(DeckId::A).track_index, Some(flac));
        assert_eq!(decks.view(DeckId::A).cues.len(), 2);
        decks
            .suggestions
            .iter()
            .map(|suggestion| suggestion.track_index)
            .collect::<Vec<_>>()
    });
    assert!(suggested.contains(&mp3), "E minor mixes after A minor");
    assert!(!suggested.contains(&flac), "not the track on deck A");

    click(window, ("suggestion", mp3), cx);
    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        let b = decks.view(DeckId::B);
        assert_eq!(b.track_index, Some(mp3));
        assert!((b.rate - 0.96).abs() < 1e-9, "B follows A's 120 BPM");
        assert!((b.heard_bpm().unwrap() - 120.0).abs() < 1e-9);
    });
    click(window, "deck-sync", cx);
    cx.update(|cx| assert_eq!(view.read(cx).decks.view(DeckId::B).rate, 1.0));
    click(window, "deck-sync", cx);
    cx.update(|cx| assert!((view.read(cx).decks.view(DeckId::B).rate - 0.96).abs() < 1e-9));
}

#[gpui::test]
fn mashup_mode_steps_through_layering_ideas_with_their_key_shift(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    use crate::views::decks::deck_part;
    let (window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    click(window, "toggle-decks", cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    click(window, track(flac), cx);
    click(window, deck_part("deck-load", DeckId::A), cx);
    click(window, ("deck-mode", 1_usize), cx);
    let ideas = cx.update(|cx| {
        let decks = &view.read(cx).decks;
        assert_eq!(decks.mode, crate::decks::DeckMode::Mashup);
        decks
            .ideas
            .iter()
            .map(|idea| (idea.track_index, idea.key.shift_semitones))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        ideas,
        [(wav, 0), (mp3, 0)],
        "C major is A minor's relative and E minor its neighbour"
    );
    assert!(is_shown(window, ("mashup-idea", wav), cx));

    click(window, "mashup-next", cx);
    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        assert_eq!(decks.idea_index, Some(0));
        assert_eq!(decks.view(DeckId::B).track_index, Some(wav));
    });
    click(window, "mashup-next", cx);
    click(window, "mashup-next", cx);
    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        assert_eq!(decks.idea_index, Some(0), "the arrows wrap around");
    });

    click(window, deck_part("deck-shift-up", DeckId::A), cx);
    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        assert_eq!(decks.view(DeckId::A).shift_semitones, 1);
        let wav_idea = decks
            .ideas
            .iter()
            .find(|idea| idea.track_index == wav)
            .expect("C major still layers after a shift");
        assert_eq!(
            wav_idea.key.shift_semitones, 1,
            "deck A in B flat minor asks for D flat major on deck B"
        );
    });
    click(window, ("deck-mode", 0_usize), cx);
    cx.update(|cx| assert_eq!(view.read(cx).decks.mode, crate::decks::DeckMode::NextTrack));
}

#[gpui::test]
fn clicking_a_deck_cue_seeks_the_sound_to_its_time(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    use crate::views::decks::cue_id;
    let mut tracks = deck_tracks();
    for track in &mut tracks {
        // The audio fixtures last one second; use a cue they can really play.
        track.outcome.as_mut().unwrap().cues[1].seconds = 0.6;
    }
    let output = RenderedDecks::default();
    let (window, view) = open_window_on(tracks, Box::new(output.clone()), cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_decks(cx);
            app.load_on_deck(DeckId::B, mp3, cx);
        })
    });
    cx.run_until_parked();
    click(window, cue_id(DeckId::B, 1), cx);
    output.pull(0.12);
    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        let position = decks.position_seconds(DeckId::B);
        assert!(decks.view(DeckId::B).playing);
        assert!(
            (0.6..0.8).contains(&position),
            "the cue seeks to 0.6s: {position}"
        );
    });
}

#[gpui::test]
fn moving_b_to_a_carries_on_from_b_and_empties_b(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    use crate::views::decks::deck_part;
    let output = RenderedDecks::default();
    let (window, view) = open_window_on(deck_tracks(), Box::new(output.clone()), cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_decks(cx);
            app.load_on_deck(DeckId::A, flac, cx);
            app.load_on_deck(DeckId::B, mp3, cx);
            app.set_crossfader(0.8, cx);
            app.set_deck_volume(DeckId::A, 0.6, cx);
        })
    });
    cx.run_until_parked();
    let suggests_mp3 = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            view.read(cx)
                .decks
                .suggestions
                .iter()
                .any(|suggestion| suggestion.track_index == mp3)
        })
    };
    assert!(suggests_mp3(cx), "deck A's suggestions offer the mp3");
    click(window, deck_part("deck-play", DeckId::B), cx);
    output.pull(0.3);
    let b_position = cx.update(|cx| view.read(cx).decks.position_seconds(DeckId::B));
    assert!(b_position > 0.05, "deck B has played: {b_position}");

    click(window, "deck-move-to-a", cx);
    cx.run_until_parked();
    output.pull(0.02);

    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        let a = decks.view(DeckId::A);
        let b = decks.view(DeckId::B);
        assert_eq!(a.track_index, Some(mp3));
        assert!(a.playing, "the music carries on");
        let position = decks.position_seconds(DeckId::A);
        assert!(
            (position - b_position).abs() < 0.05,
            "deck A takes over where B was: {position} against {b_position}"
        );
        assert_eq!(b.track_index, None);
        assert_eq!(b.path, None);
        assert!(!b.playing);
        assert!((decks.crossfader - 0.2).abs() < 1e-6);
        assert!((a.volume - 1.0).abs() < 1e-6);
        assert!((b.volume - 0.6).abs() < 1e-6);
    });
    assert!(!suggests_mp3(cx), "the suggestions follow the new deck A");
    assert!(!is_shown(window, "deck-move-to-a", cx), "deck B is empty");

    output.pull(0.3);
    let later = cx.update(|cx| view.read(cx).decks.position_seconds(DeckId::A));
    assert!(later > b_position + 0.05, "deck A plays on: {later}");
}

#[gpui::test]
fn a_load_on_deck_b_while_a_opens_calls_the_move_off(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    let (_window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_decks(cx);
            app.load_on_deck(DeckId::A, flac, cx);
            app.load_on_deck(DeckId::B, mp3, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.move_b_to_a(cx);
            app.load_on_deck(DeckId::B, wav, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        assert_eq!(decks.view(DeckId::A).track_index, Some(mp3));
        assert_eq!(
            decks.view(DeckId::B).track_index,
            Some(wav),
            "the track loaded on B after the move stays"
        );
    });
}

#[gpui::test]
fn a_decks_track_is_added_to_the_chosen_playlist(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    use crate::views::decks::deck_part;
    let (window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_decks(cx);
            app.load_on_deck(DeckId::B, mp3, cx);
        })
    });
    cx.run_until_parked();
    assert!(
        !is_shown(window, deck_part("deck-add-to-playlist", DeckId::B), cx),
        "no playlist to add to yet"
    );
    let (first, second) = cx.update(|cx| {
        view.update(cx, |app, _| {
            let playlists = app.session.playlists_mut();
            (playlists.create(), playlists.create())
        })
    });
    assert!(is_shown(
        window,
        deck_part("deck-add-to-playlist", DeckId::B),
        cx
    ));
    assert!(!is_shown(
        window,
        deck_part("deck-add-to-playlist", DeckId::A),
        cx
    ));

    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.add_deck_track_to_playlist(DeckId::B, second, cx)
        })
    });

    cx.update(|cx| {
        let app = view.read(cx);
        let path = app.decks.view(DeckId::B).path.clone().unwrap();
        let playlists = app.session.playlists();
        assert_eq!(playlists.get(second).unwrap().tracks, vec![path]);
        assert!(playlists.get(first).unwrap().tracks.is_empty());
    });
}

#[gpui::test]
fn a_loop_button_loops_the_deck_and_pressed_again_leaves_it(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    use crate::views::decks::{deck_part, loop_id};
    let output = RenderedDecks::default();
    let (window, view) = open_window_on(deck_tracks(), Box::new(output.clone()), cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_decks(cx);
            app.load_on_deck(DeckId::A, flac, cx);
        })
    });
    cx.run_until_parked();
    assert!(!is_shown(
        window,
        deck_part("deck-loop-region", DeckId::A),
        cx
    ));
    click(window, deck_part("deck-play", DeckId::A), cx);
    output.pull(0.05);
    click(window, loop_id(DeckId::A, 1), cx);
    output.pull(0.05);
    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        assert!(decks.is_looping(DeckId::A, 1));
        assert!(!decks.is_looping(DeckId::A, 2));
        let (start, end) = decks.loop_seconds(DeckId::A).unwrap();
        assert!(
            (start - 0.2).abs() < 1e-3,
            "from the next downbeat: {start}"
        );
        assert!((end - 2.2).abs() < 1e-3, "one bar at 120 BPM: {end}");
    });
    assert!(is_shown(
        window,
        deck_part("deck-loop-region", DeckId::A),
        cx
    ));
    click(window, loop_id(DeckId::A, 1), cx);
    output.pull(0.05);
    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        assert_eq!(decks.view(DeckId::A).loop_bars, None);
        assert!(!decks.is_looping(DeckId::A, 1));
    });
}

#[gpui::test]
fn a_deck_without_a_beat_grid_does_not_loop(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    use crate::views::decks::loop_id;
    let mut tracks = deck_tracks();
    for track in &mut tracks {
        track.outcome.as_mut().unwrap().beat_grid = None;
    }
    let output = RenderedDecks::default();
    let (window, view) = open_window_on(tracks, Box::new(output.clone()), cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_decks(cx);
            app.load_on_deck(DeckId::A, flac, cx);
        })
    });
    cx.run_until_parked();
    click(window, loop_id(DeckId::A, 4), cx);
    output.pull(0.05);
    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        assert_eq!(decks.view(DeckId::A).loop_bars, None);
        assert_eq!(decks.loop_seconds(DeckId::A), None);
    });
}

#[gpui::test]
fn the_energy_direction_reorders_the_suggestions(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    use euphoria_core::harmony::EnergyDirection;
    let (window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    click(window, "toggle-decks", cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    cx.update(|cx| view.update(cx, |app, cx| app.load_on_deck(DeckId::A, flac, cx)));
    let first =
        |cx: &mut TestAppContext| cx.update(|cx| view.read(cx).decks.suggestions[0].track_index);
    let up = EnergyDirection::ALL
        .iter()
        .position(|direction| *direction == EnergyDirection::Up)
        .unwrap();
    let down = EnergyDirection::ALL
        .iter()
        .position(|direction| *direction == EnergyDirection::Down)
        .unwrap();
    click(window, ("energy-direction", up), cx);
    assert_eq!(first(cx), mp3, "energy 7 after 6");
    click(window, ("energy-direction", down), cx);
    assert_eq!(first(cx), wav, "energy 5 after 6");
}

#[gpui::test]
fn the_transport_cues_and_mixer_drive_the_decks(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    use crate::views::decks::{cue_id, deck_part};
    let (window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    click(window, "toggle-decks", cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.load_on_deck(DeckId::A, flac, cx);
            app.load_on_deck(DeckId::B, mp3, cx);
        })
    });
    cx.run_until_parked();
    click(window, deck_part("deck-play", DeckId::A), cx);
    click(window, cue_id(DeckId::B, 1), cx);
    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        assert!(decks.view(DeckId::A).playing);
        assert!(decks.view(DeckId::B).playing, "a cue plays its deck");
    });
    click(window, deck_part("deck-play", DeckId::A), cx);
    click(window, deck_part("deck-keylock", DeckId::B), cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.set_crossfader(0.0, cx);
            app.set_deck_volume(DeckId::A, 0.5, cx);
        })
    });
    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        assert!(!decks.view(DeckId::A).playing);
        assert!(!decks.view(DeckId::B).keylock);
        assert_eq!(decks.gains(), [0.5, 0.0]);
        assert!(decks.load_readout().is_some());
    });
    assert!(
        !is_shown(window, "deck-load-readout", cx),
        "the readout stays hidden while no frames drop"
    );
    assert!(is_shown(window, "deck-load-hint", cx));
    click(window, "toggle-decks", cx);
    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        assert!(!decks.is_open);
        assert!(
            !decks.view(DeckId::B).playing,
            "closing the decks stops them"
        );
    });
}

#[gpui::test]
fn the_decks_without_an_audio_output_say_so(cx: &mut TestAppContext) {
    let (window, view) = open_window_on(deck_tracks(), Box::new(NoOutput), cx);
    click(window, "toggle-decks", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(!app.decks.is_open);
        assert!(!app.notice.is_empty());
    });
}

#[gpui::test]
fn suggestions_can_come_from_a_playlist(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    let (window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    click(window, "toggle-decks", cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    let wav_path = cx.update(|cx| {
        view.read(cx)
            .session
            .library()
            .track(wav)
            .unwrap()
            .path
            .clone()
    });
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let id = app.session.playlists_mut().create();
            app.session
                .playlists_mut()
                .add_tracks(id, std::slice::from_ref(&wav_path));
            app.load_on_deck(DeckId::A, flac, cx);
        })
    });
    let suggested = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            view.read(cx)
                .decks
                .suggestions
                .iter()
                .map(|suggestion| suggestion.track_index)
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(suggested(cx).len(), 2, "the whole library");
    click(window, ("suggestion-scope", 3_usize), cx);
    assert_eq!(suggested(cx), [wav], "only the playlist's track");
}

#[gpui::test]
fn a_deck_does_not_play_or_cue_the_previous_track_while_loading(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    let output = RenderedDecks::default();
    let (_, view) = open_window_on(deck_tracks(), Box::new(output.clone()), cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_decks(cx);
            app.load_on_deck(DeckId::A, flac, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|cx| view.update(cx, |app, cx| app.toggle_deck_play(DeckId::A, cx)));
    assert!(
        output.pull(2.0) > 0.01,
        "the first load must be open and play before the replacement"
    );
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.load_on_deck(DeckId::A, wav, cx);
            assert!(
                app.decks
                    .view(DeckId::A)
                    .path
                    .as_deref()
                    .is_some_and(|path| path.ends_with("sine-440hz-stereo.wav")),
                "the replacement's view is in place while it opens"
            );
            app.toggle_deck_play(DeckId::A, cx);
            assert!(
                !app.decks.view(DeckId::A).playing,
                "Play must not start the previous source while the replacement opens"
            );
            app.play_deck_cue(DeckId::A, 0, cx);
            assert!(
                !app.decks.view(DeckId::A).playing,
                "a cue must not start the previous source either"
            );
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_deck_play(DeckId::A, cx);
            assert!(
                app.decks.view(DeckId::A).playing,
                "the replaced source can play once it is open"
            );
        })
    });
}

#[gpui::test]
fn shown_deck_suggestions_follow_the_search(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    use crate::decks::SuggestionScope;
    let (_, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_decks(cx);
            app.load_on_deck(DeckId::A, flac, cx);
            app.set_suggestion_scope(SuggestionScope::Shown, cx);
            assert!(!app.decks.suggestions.is_empty());
            app.query = "nothing-matches-this-query".to_string();
            app.refresh_visible();
            assert!(app.visible.is_empty());
            assert!(
                app.decks.suggestions.is_empty(),
                "Tracks shown must exclude filtered-out candidates"
            );
        })
    });
}

#[gpui::test]
fn playlist_deck_suggestions_follow_edits_without_showing_the_playlist(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    use crate::decks::SuggestionScope;
    let (_, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_decks(cx);
            app.load_on_deck(DeckId::A, flac, cx);
            let id = app.session.playlists_mut().create();
            app.set_suggestion_scope(SuggestionScope::Playlist(id), cx);
            let path = app.session.library().track(mp3).unwrap().path.clone();
            app.add_to_playlist(id, &[path], cx);
            assert_eq!(
                app.decks
                    .suggestions
                    .iter()
                    .map(|s| s.track_index)
                    .collect::<Vec<_>>(),
                [mp3]
            );
            app.delete_playlist(id, cx);
            assert!(app.decks.suggestions.is_empty());
        })
    });
}

#[gpui::test]
fn the_player_cannot_start_a_third_track_while_decks_own_the_sound(cx: &mut TestAppContext) {
    let (_, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.select(flac, cx);
            app.toggle_decks(cx);
            app.toggle_playback(cx);
            assert!(
                !app.player.offers_pause(),
                "the normal player must stay paused while decks are open"
            );
            app.toggle_decks(cx);
            app.toggle_playback(cx);
            assert!(
                app.player.offers_pause(),
                "closing the decks releases the normal player"
            );
        })
    });
}

#[gpui::test]
fn correcting_a_loaded_decks_tempo_updates_sync_and_discards_its_old_grid(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    let (_, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_decks(cx);
            app.load_on_deck(DeckId::A, flac, cx);
            app.load_on_deck(DeckId::B, mp3, cx);
            app.select(flac, cx);
            app.accept_bpm(130.0, cx);
            let a = app.decks.view(DeckId::A);
            assert_eq!(a.bpm, Some(130.0));
            assert!(a.grid.is_none());
            assert!(a.cues.is_empty());
            assert!((app.decks.view(DeckId::B).rate - 1.04).abs() < 1e-9);
            app.reset_bpm(cx);
            assert!(app.decks.view(DeckId::A).grid.is_some());
            assert_eq!(app.decks.view(DeckId::A).cues.len(), 2);
            assert!((app.decks.view(DeckId::B).rate - 0.96).abs() < 1e-9);
        })
    });
}

/// Runs the actual deck mixer while retaining the normal output's silent voices.
#[derive(Clone, Default)]
struct RenderedDecks(std::rc::Rc<std::cell::RefCell<Option<crate::deck_audio::DeckMix>>>);

impl RenderedDecks {
    fn pull(&self, seconds: f64) -> f32 {
        let frames = (seconds * f64::from(crate::deck_audio::MIX_SAMPLE_RATE)) as usize;
        let mut peak = 0.0_f32;
        for block in (0..frames).step_by(256) {
            {
                let mut mix = self.0.borrow_mut();
                let mix = mix.as_mut().unwrap();
                for _ in 0..(frames - block).min(256) * 2 {
                    peak = peak.max(mix.next().unwrap().abs());
                }
            }
            // Decoding remains asynchronous, exactly as with the real output.
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
        peak
    }
}

impl AudioOutput for RenderedDecks {
    fn play(
        &mut self,
        source: crate::playback::TrackSource,
    ) -> Result<Box<dyn crate::playback::Voice>, PlaybackProblem> {
        SilentOutput.play(source)
    }
    fn play_reference(
        &mut self,
        sound: crate::reference_sound::ReferenceSound,
    ) -> Result<Box<dyn crate::playback::Voice>, PlaybackProblem> {
        SilentOutput.play_reference(sound)
    }
    fn play_metronome(
        &mut self,
        clicks: crate::metronome::Metronome,
    ) -> Result<Box<dyn crate::playback::Voice>, PlaybackProblem> {
        SilentOutput.play_metronome(clicks)
    }
    fn play_decks(
        &mut self,
        mix: crate::deck_audio::DeckMix,
    ) -> Result<Box<dyn crate::playback::Voice>, PlaybackProblem> {
        *self.0.borrow_mut() = Some(mix);
        struct Voice(RenderedDecks);
        impl crate::playback::Voice for Voice {
            fn pause(&self) {}
            fn resume(&self) {}
        }
        impl Drop for Voice {
            fn drop(&mut self) {
                *self.0.0.borrow_mut() = None;
            }
        }
        Ok(Box::new(Voice(self.clone())))
    }
}

#[gpui::test]
fn a_finished_deck_can_play_and_cue_again(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    let output = RenderedDecks::default();
    let (_, view) = open_window_on(deck_tracks(), Box::new(output.clone()), cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_decks(cx);
            app.load_on_deck(DeckId::A, flac, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|cx| view.update(cx, |app, cx| app.toggle_deck_play(DeckId::A, cx)));
    assert!(output.pull(2.0) > 0.01);
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(
            !view.read(cx).decks.view(DeckId::A).playing,
            "EOF must update the transport"
        )
    });
    cx.update(|cx| view.update(cx, |app, cx| app.toggle_deck_play(DeckId::A, cx)));
    cx.run_until_parked();
    assert!(
        output.pull(2.0) > 0.01,
        "Play reopens the exhausted decoder from the start"
    );
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    cx.update(|cx| view.update(cx, |app, cx| app.play_deck_cue(DeckId::A, 0, cx)));
    cx.run_until_parked();
    assert!(
        output.pull(1.0) > 0.01,
        "a cue reopens the exhausted decoder at its cue"
    );
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_deck_play(DeckId::A, cx);
            app.toggle_decks(cx);
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let decks = &view.read(cx).decks;
        assert!(!decks.is_open);
        assert!(
            !decks.view(DeckId::A).playing,
            "a pending reopen cannot restart a closed deck"
        );
        assert!(!decks.view(DeckId::A).loading);
    });
}

#[gpui::test]
fn replacing_a_deck_load_cancels_its_abandoned_waveform_work(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    let (_, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    let abandoned = cx.update(|cx| {
        view.read(cx)
            .session
            .library()
            .track(mp3)
            .unwrap()
            .path
            .clone()
    });
    let current = cx.update(|cx| {
        view.read(cx)
            .session
            .library()
            .track(wav)
            .unwrap()
            .path
            .clone()
    });
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_decks(cx);
            app.load_on_deck(DeckId::A, mp3, cx);
            app.load_on_deck(DeckId::A, wav, cx);
        })
    });
    for _ in 0..20 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
    }
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(matches!(
            app.waveform_of(&current),
            Some(Waveform::Ready(..))
        ));
        assert!(
            app.waveform_of(&abandoned).is_none(),
            "replaced loads must not keep decoding full tracks"
        );
    });
}
