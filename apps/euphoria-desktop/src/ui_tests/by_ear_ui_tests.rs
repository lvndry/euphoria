// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 14 tests: the tones, the piano, the chords and hearing the track in another key.

#[gpui::test]
fn the_tones_play_over_the_track_one_at_a_time_and_stop(cx: &mut TestAppContext) {
    let (window, _, output) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    click(window, ("tone", 0_usize), cx);
    let sounding = output.sounding();
    assert_eq!(sounding.len(), 1);
    assert!(sounding[0].is_drone);
    assert!((sounding[0].frequencies[1] - 220.0).abs() < 0.01, "A minor");

    click(window, ("tone", 1_usize), cx);
    let sounding = output.sounding();
    assert_eq!(sounding.len(), 1, "one tone at a time");
    assert!(
        (sounding[0].frequencies[1] - 164.81).abs() < 0.01,
        "the runner up, E minor"
    );

    click(window, ("tone", 1_usize), cx);
    assert!(output.sounding().is_empty(), "a second click stops it");

    click(window, ("tone", 0_usize), cx);
    assert_eq!(output.sounding().len(), 1);
    click(window, track(2), cx);
    assert!(
        output.sounding().is_empty(),
        "another track has its own key"
    );
}

#[gpui::test]
fn taking_another_key_stops_the_drone_of_the_old_one(cx: &mut TestAppContext) {
    let (window, _, output) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    click(window, ("tone", 0_usize), cx);
    assert_eq!(output.sounding().len(), 1);
    click(window, "runner-up-key", cx);
    assert!(output.sounding().is_empty(), "the drone was of the old key");

    click(window, ("tone", 0_usize), cx);
    let sounding = output.sounding();
    assert_eq!(sounding.len(), 1, "the tone button plays the new key");
    assert!(
        (sounding[0].frequencies[1] - 164.81).abs() < 0.01,
        "E minor"
    );
    click(window, "key-reset", cx);
    assert!(output.sounding().is_empty());
}

#[gpui::test]
fn the_piano_plays_its_notes_and_the_volume_has_steps(cx: &mut TestAppContext) {
    let (window, view, output) = open_tracks_to_check_by_ear(cx);
    click(window, ("piano-key", 9_usize), cx);
    click(window, ("piano-key", 1_usize), cx);
    let heard = output.heard();
    assert_eq!(heard.len(), 2);
    assert!(heard.iter().all(|sound| !sound.is_drone));
    assert!((heard[0].frequencies[0] - 440.0).abs() < 0.01);
    assert!((heard[1].frequencies[0] - 277.18).abs() < 0.01);

    click(window, ("reference-level", 1_usize), cx);
    cx.update(|cx| assert_eq!(view.read(cx).by_ear.level_step(), 1));
    click(window, ("reference-level", 5_usize), cx);
    cx.update(|cx| assert_eq!(view.read(cx).by_ear.level_step(), 5));
}

fn chord_notes(output: &ListeningOutput) -> Vec<f32> {
    output
        .heard()
        .iter()
        .map(|sound| sound.frequencies[0])
        .collect()
}

fn lit_keys(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<u8> {
    cx.update(|cx| view.read(cx).by_ear.lit_keys())
}

#[gpui::test]
fn the_chord_button_plays_the_triad_of_the_key_together_and_lights_its_keys(
    cx: &mut TestAppContext,
) {
    let (window, view, output) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    click(window, ("chord", 0_usize), cx);
    let heard = output.heard();
    assert_eq!(heard.len(), 3, "root, third and fifth");
    assert!(heard.iter().all(|sound| !sound.is_drone));
    let expected = [440.0, 523.25, 659.26];
    for (sound, expected) in heard.iter().zip(expected) {
        assert!((sound.frequencies[0] - expected).abs() < 0.01, "A C E");
    }
    assert_eq!(output.sounding().len(), 3, "together");
    assert_eq!(lit_keys(&view, cx), [0, 4, 9]);

    cx.executor()
        .advance_clock(std::time::Duration::from_millis(1900));
    cx.run_until_parked();
    assert_eq!(lit_keys(&view, cx), [0, 4, 9], "still ringing");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    assert!(
        lit_keys(&view, cx).is_empty(),
        "the keys go dark when it ends"
    );
}

#[gpui::test]
fn the_chord_sounds_over_a_tone_without_stopping_it(cx: &mut TestAppContext) {
    let (window, _, output) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    click(window, ("tone", 0_usize), cx);
    click(window, ("chord", 0_usize), cx);
    assert_eq!(
        output.sounding().len(),
        4,
        "the drone stays under the chord"
    );
}

#[gpui::test]
fn a_major_key_gets_a_major_third(cx: &mut TestAppContext) {
    let (window, view, output) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.accept_key(euphoria_core::Key::new(0, euphoria_core::Mode::Major), cx)
        })
    });
    cx.run_until_parked();
    click(window, ("chord", 0_usize), cx);
    let notes = chord_notes(&output);
    for (note, expected) in notes.iter().zip([261.63, 329.63, 392.0]) {
        assert!((note - expected).abs() < 0.01, "8B: C E G");
    }
    assert_eq!(lit_keys(&view, cx), [0, 4, 7]);
}

#[gpui::test]
fn the_runner_up_gets_its_own_chord_button_only_when_there_is_one(cx: &mut TestAppContext) {
    let (window, _, output) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    assert!(is_shown(window, ("chord", 0_usize), cx));
    assert!(is_shown(window, ("chord", 1_usize), cx), "key unsure");
    click(window, ("chord", 1_usize), cx);
    let notes = chord_notes(&output);
    for (note, expected) in notes.iter().zip([329.63, 392.0, 493.88]) {
        assert!(
            (note - expected).abs() < 0.01,
            "the runner up, E minor: E G B"
        );
    }

    click(window, "runner-up-key", cx);
    assert!(is_shown(window, ("chord", 0_usize), cx));
    assert!(
        !is_shown(window, ("chord", 1_usize), cx),
        "a corrected key has no second candidate"
    );
    click(window, track(2), cx);
    assert!(is_shown(window, ("chord", 0_usize), cx));
    assert!(!is_shown(window, ("chord", 1_usize), cx), "a sure key");
}

#[gpui::test]
fn a_shift_click_plays_the_chord_as_an_arpeggio_120_ms_apart(cx: &mut TestAppContext) {
    let (window, view, output) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click_with_modifiers(("chord", 0_usize), Modifiers::shift(), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(output.heard().len(), 1, "the root first");
    assert_eq!(lit_keys(&view, cx), [9]);

    cx.executor()
        .advance_clock(std::time::Duration::from_millis(119));
    cx.run_until_parked();
    assert_eq!(output.heard().len(), 1, "not before 120 ms");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(output.heard().len(), 2, "the third at 120 ms");
    assert_eq!(lit_keys(&view, cx), [0, 9]);

    cx.executor()
        .advance_clock(std::time::Duration::from_millis(119));
    cx.run_until_parked();
    assert_eq!(output.heard().len(), 2);
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(output.heard().len(), 3, "the fifth at 240 ms");
    assert_eq!(lit_keys(&view, cx), [0, 4, 9]);
    let notes = chord_notes(&output);
    assert!(notes[0] < notes[1] && notes[1] < notes[2], "rising");

    cx.executor()
        .advance_clock(std::time::Duration::from_millis(2100));
    cx.run_until_parked();
    assert!(lit_keys(&view, cx).is_empty());
}

fn hear_in_another_key(view: &Entity<EuphoriaApp>, semitones: i8, cx: &mut TestAppContext) {
    cx.update(|cx| view.update(cx, |app, cx| app.set_key_shift(semitones, cx)));
    cx.run_until_parked();
}

fn set_shown_key(view: &Entity<EuphoriaApp>, key: euphoria_core::Key, cx: &mut TestAppContext) {
    cx.update(|cx| view.update(cx, |app, cx| app.accept_key(key, cx)));
    cx.run_until_parked();
}

fn pitch_classes(output: &ListeningOutput) -> Vec<u8> {
    output
        .heard()
        .iter()
        .map(|sound| {
            let midi = 69.0 + 12.0 * (sound.frequencies[0] / 440.0).log2();
            (midi.round() as i32).rem_euclid(12) as u8
        })
        .collect()
}

fn drone_key(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<euphoria_core::Key> {
    cx.update(|cx| view.read(cx).by_ear.drone_key())
}

#[gpui::test]
fn the_tone_the_piano_and_the_chord_follow_the_key_it_is_heard_in(cx: &mut TestAppContext) {
    use euphoria_core::{Key, Mode};
    let cases = [
        (
            Key::new(9, Mode::Minor),
            1,
            Key::new(10, Mode::Minor),
            [10, 1, 5],
            "3A",
        ),
        (
            Key::new(9, Mode::Minor),
            -2,
            Key::new(7, Mode::Minor),
            [7, 10, 2],
            "6A",
        ),
        (
            Key::new(9, Mode::Minor),
            6,
            Key::new(3, Mode::Minor),
            [3, 6, 10],
            "2A",
        ),
        (
            Key::new(0, Mode::Major),
            1,
            Key::new(1, Mode::Major),
            [1, 5, 8],
            "3B",
        ),
        (
            Key::new(0, Mode::Major),
            -2,
            Key::new(10, Mode::Major),
            [10, 2, 5],
            "6B",
        ),
        (
            Key::new(0, Mode::Major),
            6,
            Key::new(6, Mode::Major),
            [6, 10, 1],
            "2B",
        ),
    ];
    for (shown, semitones, heard_key, triad, camelot) in cases {
        let (window, view, output) = open_tracks_to_check_by_ear(cx);
        click(window, track(1), cx);
        set_shown_key(&view, shown, cx);
        hear_in_another_key(&view, semitones, cx);
        assert_eq!(heard_key.camelot(), camelot);

        cx.update(|cx| {
            let app = view.read(cx);
            let (key, _) =
                crate::views::by_ear::heard_keys(app.selected_track().unwrap(), app.key_shift)
                    .unwrap();
            assert_eq!(key, heard_key, "{shown} moved {semitones:+}");
            let mut marked = crate::key_by_ear::scale_of(key).to_vec();
            marked.sort_unstable();
            let mut expected: Vec<u8> = [0, 2, 3, 5, 7, 8, 10]
                .iter()
                .map(|step| (heard_key.tonic() + step) % 12)
                .collect();
            if heard_key.mode() == Mode::Major {
                expected = [0, 2, 4, 5, 7, 9, 11]
                    .iter()
                    .map(|step| (heard_key.tonic() + step) % 12)
                    .collect();
            }
            expected.sort_unstable();
            assert_eq!(marked, expected, "the piano marks the notes of {camelot}");
        });

        click(window, ("tone", 0_usize), cx);
        assert_eq!(drone_key(&view, cx), Some(heard_key));
        let sounding = output.sounding();
        let root_hz = 220.0 * 2f32.powf((f32::from(heard_key.tonic()) - 9.0) / 12.0);
        let fifth_hz = root_hz * 2f32.powf(7.0 / 12.0);
        let octaves_apart = |first: f32, second: f32| {
            let semitones = 12.0 * (first / second).log2();
            (semitones - 12.0 * (semitones / 12.0).round()).abs()
        };
        assert!(
            octaves_apart(sounding[0].frequencies[1], fifth_hz) < 0.1
                || octaves_apart(sounding[0].frequencies[1], root_hz) < 0.1,
            "the tone is built on {camelot}"
        );
        click(window, ("tone", 0_usize), cx);

        click(window, ("chord", 0_usize), cx);
        let heard = pitch_classes(&output);
        assert_eq!(heard[heard.len() - 3..], triad, "the triad of {camelot}");
        let mut lit = triad.to_vec();
        lit.sort_unstable();
        assert_eq!(lit_keys(&view, cx), lit);
    }
}

#[gpui::test]
fn the_tone_stays_as_detected_at_zero_semitones_and_is_named_by_the_heard_key(
    cx: &mut TestAppContext,
) {
    use euphoria_core::{Key, Mode};
    let (window, view, output) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    click(window, ("tone", 0_usize), cx);
    let sounding = output.sounding();
    assert!(
        (sounding[0].frequencies[1] - 220.0).abs() < 0.01,
        "0 semitones is A minor"
    );
    assert_eq!(drone_key(&view, cx), Some(Key::new(9, Mode::Minor)));
    click(window, ("tone", 0_usize), cx);

    set_shown_key(&view, Key::new(2, Mode::Minor), cx);
    hear_in_another_key(&view, 2, cx);
    click(window, ("tone", 0_usize), cx);
    assert_eq!(
        drone_key(&view, cx),
        Some(Key::new(4, Mode::Minor)),
        "7A heard 2 semitones up is E minor, 9A"
    );
}

#[gpui::test]
fn a_playing_tone_retunes_when_the_shift_changes(cx: &mut TestAppContext) {
    use euphoria_core::{Key, Mode};
    let (window, view, output) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    click(window, ("tone", 0_usize), cx);
    hear_in_another_key(&view, 2, cx);
    assert_eq!(drone_key(&view, cx), Some(Key::new(11, Mode::Minor)));
    let sounding = output.sounding();
    assert_eq!(sounding.len(), 1, "one tone, retuned");
    assert!((sounding[0].frequencies[1] - 220.0 * 2f32.powf(2.0 / 12.0)).abs() < 0.05);
    hear_in_another_key(&view, -1, cx);
    assert_eq!(drone_key(&view, cx), Some(Key::new(8, Mode::Minor)));
    assert_eq!(output.sounding().len(), 1);
    hear_in_another_key(&view, 0, cx);
    assert_eq!(drone_key(&view, cx), Some(Key::new(9, Mode::Minor)));
    click(window, ("tone", 0_usize), cx);
    assert!(
        output.sounding().is_empty(),
        "the button shows it on, so it stops it"
    );
}

#[gpui::test]
fn the_runner_up_transposes_with_the_key(cx: &mut TestAppContext) {
    let (window, view, output) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    assert!(is_shown(window, ("chord", 1_usize), cx), "key unsure");
    click(window, ("chord", 1_usize), cx);
    assert_eq!(pitch_classes(&output), [4, 7, 11], "E minor at 0 semitones");
    hear_in_another_key(&view, 2, cx);
    click(window, ("chord", 1_usize), cx);
    assert_eq!(
        pitch_classes(&output)[3..],
        [6, 9, 1],
        "F# minor, the runner up +2"
    );
    click(window, ("tone", 1_usize), cx);
    assert_eq!(
        drone_key(&view, cx),
        Some(euphoria_core::Key::new(6, euphoria_core::Mode::Minor))
    );
}

#[gpui::test]
fn a_tone_without_an_audio_output_says_so(cx: &mut TestAppContext) {
    let (window, view) = open_window_with(
        tracks_to_review(),
        Box::new(NoOutput),
        Settings::default(),
        None,
        USUAL_WINDOW,
        cx,
    );
    open_sections(&view, &[crate::inspector_sections::SectionKind::ByEar], cx);
    click(window, ("tone", 0_usize), cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.by_ear.drone_key(), None);
        assert_eq!(app.notice.as_slice(), [PlaybackProblem::NoOutput.message()]);
    });
}

#[gpui::test]
fn a_corrected_key_hides_the_detected_key_timeline_until_it_is_reset(cx: &mut TestAppContext) {
    let (window, view) =
        open_track_changing_key(ThemeName::Slate, Panels::default(), USUAL_WINDOW, cx);
    assert!(is_shown(window, "key-timeline", cx));
    let detected = cx.update(|cx| {
        view.read(cx)
            .selected_track()
            .unwrap()
            .detected_key()
            .unwrap()
    });
    let other = euphoria_core::Key::new((detected.tonic() + 1) % 12, detected.mode());
    cx.update(|cx| view.update(cx, |app, cx| app.accept_key(other, cx)));
    assert!(!is_shown(window, "key-timeline", cx));
    cx.update(|cx| view.update(cx, |app, cx| app.accept_key(detected, cx)));
    assert!(
        !is_shown(window, "key-timeline", cx),
        "a correction to the detected key is still the user's"
    );
    cx.update(|cx| view.update(cx, |app, cx| app.reset_key(cx)));
    assert!(is_shown(window, "key-timeline", cx));
}
