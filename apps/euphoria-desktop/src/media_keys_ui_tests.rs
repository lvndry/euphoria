// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The remote commands and Now Playing through the real root view in a
//! headless window, with the system's calls replaced by a recording.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{AppContext, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::media_keys::{self, NowPlaying, NowPlayingSurface, RemoteCommand};
use crate::ui_tests::{index_of_track, open_fixture_library};

#[derive(Default)]
struct Told {
    published: Vec<NowPlaying>,
    cleared: usize,
}

struct Recording(Rc<RefCell<Told>>);

impl NowPlayingSurface for Recording {
    fn publish(&mut self, info: &NowPlaying) {
        self.0.borrow_mut().published.push(info.clone());
    }
    fn clear(&mut self) {
        self.0.borrow_mut().cleared += 1;
    }
}

fn connect(
    view: &Entity<EuphoriaApp>,
    cx: &mut TestAppContext,
) -> (async_channel::Sender<RemoteCommand>, Rc<RefCell<Told>>) {
    let (sender, receiver) = async_channel::unbounded();
    let told = Rc::new(RefCell::new(Told::default()));
    let surface = Box::new(Recording(Rc::clone(&told)));
    cx.update(|cx| media_keys::install(view, surface, receiver, cx));
    (sender, told)
}

fn send(
    sender: &async_channel::Sender<RemoteCommand>,
    command: RemoteCommand,
    cx: &mut TestAppContext,
) {
    sender.try_send(command).unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn commands_sent_from_another_thread_run_in_the_app(cx: &mut TestAppContext) {
    let (_, view) = open_fixture_library(cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    cx.update(|cx| view.update(cx, |app, cx| app.select(flac, cx)));
    let (sender, told) = connect(&view, cx);

    let from_system_thread = sender.clone();
    std::thread::spawn(move || {
        from_system_thread
            .try_send(RemoteCommand::TogglePlayPause)
            .unwrap();
    })
    .join()
    .unwrap();
    cx.run_until_parked();

    cx.update(|cx| {
        assert!(view.read(cx).player.offers_pause());
    });
    assert!(!told.borrow().published.is_empty());

    send(&sender, RemoteCommand::Pause, cx);
    cx.update(|cx| assert!(!view.read(cx).player.offers_pause()));
    send(&sender, RemoteCommand::Play, cx);
    cx.update(|cx| assert!(view.read(cx).player.offers_pause()));
}

#[gpui::test]
fn now_playing_names_the_track_and_clears_when_nothing_is_held(cx: &mut TestAppContext) {
    let (_, view) = open_fixture_library(cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    cx.update(|cx| view.update(cx, |app, cx| app.select(flac, cx)));
    cx.update(|cx| assert!(view.read(cx).now_playing().is_none()));

    let (sender, told) = connect(&view, cx);
    send(&sender, RemoteCommand::Play, cx);
    let info = told.borrow().published.last().cloned().unwrap();
    assert_eq!(info.title, "sine-440hz-mono.flac");
    assert!(info.artist.is_none());
    assert_eq!(told.borrow().cleared, 0);
}

#[gpui::test]
fn next_selects_and_plays_the_row_after(cx: &mut TestAppContext) {
    let (_, view) = open_fixture_library(cx);
    let first = cx.update(|cx| view.read(cx).visible[0]);
    let second = cx.update(|cx| view.read(cx).visible[1]);
    cx.update(|cx| view.update(cx, |app, cx| app.select(first, cx)));
    let (sender, _) = connect(&view, cx);
    send(&sender, RemoteCommand::NextTrack, cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.selected, Some(second));
        assert!(app.player.offers_pause());
    });
}
