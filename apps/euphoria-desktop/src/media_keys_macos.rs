// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The macOS side of the media keys: MPRemoteCommandCenter for the commands
//! and MPNowPlayingInfoCenter for what is shown in Control Center and on
//! the menu bar.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSDictionary, NSNumber, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemPropertyArtist,
    MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle, MPNowPlayingInfoCenter,
    MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate,
    MPNowPlayingPlaybackState, MPRemoteCommand, MPRemoteCommandCenter, MPRemoteCommandEvent,
    MPRemoteCommandHandlerStatus,
};

use crate::media_keys::{NowPlaying, NowPlayingSurface, RemoteCommand};

/// The system's Now Playing, with the remote commands registered for as long
/// as it lives.
pub(crate) struct SystemNowPlaying {
    center: Retained<MPNowPlayingInfoCenter>,
    registered: Vec<(Retained<MPRemoteCommand>, Retained<AnyObject>)>,
}

/// Registers the commands and returns the surface with the commands the
/// system sends. The system calls the handlers on a thread of its own; they
/// only queue the command.
pub(crate) fn connect() -> (SystemNowPlaying, async_channel::Receiver<RemoteCommand>) {
    let (sender, receiver) = async_channel::unbounded();
    // SAFETY: the shared center and its commands are plain Objective-C
    // objects, and each handler block matches the signature the framework
    // expects and only touches the cloned sender.
    unsafe {
        let center = MPRemoteCommandCenter::sharedCommandCenter();
        let mut registered = Vec::new();
        let simple = [
            (center.playCommand(), RemoteCommand::Play),
            (center.pauseCommand(), RemoteCommand::Pause),
            (
                center.togglePlayPauseCommand(),
                RemoteCommand::TogglePlayPause,
            ),
            (center.nextTrackCommand(), RemoteCommand::NextTrack),
            (center.previousTrackCommand(), RemoteCommand::PreviousTrack),
        ];
        for (command, mapped) in simple {
            let sender = sender.clone();
            let handler = RcBlock::new(move |_event: NonNull<MPRemoteCommandEvent>| {
                let _ = sender.try_send(mapped);
                MPRemoteCommandHandlerStatus::Success
            });
            command.setEnabled(true);
            let target = command.addTargetWithHandler(&handler);
            registered.push((command, target));
        }
        let seek = center.changePlaybackPositionCommand();
        let seek_sender = sender.clone();
        let handler = RcBlock::new(move |event: NonNull<MPRemoteCommandEvent>| {
            let event = event.cast::<MPChangePlaybackPositionCommandEvent>();
            let seconds = event.as_ref().positionTime();
            let _ = seek_sender.try_send(RemoteCommand::SeekTo(seconds));
            MPRemoteCommandHandlerStatus::Success
        });
        seek.setEnabled(true);
        let target = seek.addTargetWithHandler(&handler);
        registered.push((Retained::into_super(seek), target));
        (
            SystemNowPlaying {
                center: MPNowPlayingInfoCenter::defaultCenter(),
                registered,
            },
            receiver,
        )
    }
}

impl Drop for SystemNowPlaying {
    fn drop(&mut self) {
        for (command, target) in self.registered.drain(..) {
            // SAFETY: the target is the object this command returned when
            // the handler was added.
            unsafe { command.removeTarget(Some(&target)) };
        }
    }
}

fn number(value: f64) -> Retained<AnyObject> {
    // SAFETY: an NSNumber is an Objective-C object.
    unsafe { Retained::cast_unchecked(NSNumber::new_f64(value)) }
}

fn text(value: &str) -> Retained<AnyObject> {
    // SAFETY: an NSString is an Objective-C object.
    unsafe { Retained::cast_unchecked(NSString::from_str(value)) }
}

impl NowPlayingSurface for SystemNowPlaying {
    fn publish(&mut self, info: &NowPlaying) {
        // SAFETY: the property keys are the framework's own constants.
        unsafe {
            let mut keys: Vec<&NSString> = vec![
                MPMediaItemPropertyTitle,
                MPNowPlayingInfoPropertyElapsedPlaybackTime,
                MPNowPlayingInfoPropertyPlaybackRate,
            ];
            let mut values = vec![
                text(&info.title),
                number(info.elapsed_seconds),
                number(info.rate),
            ];
            if let Some(artist) = &info.artist {
                keys.push(MPMediaItemPropertyArtist);
                values.push(text(artist));
            }
            if let Some(duration) = info.duration_seconds {
                keys.push(MPMediaItemPropertyPlaybackDuration);
                values.push(number(duration));
            }
            let dictionary = NSDictionary::from_retained_objects(&keys, &values);
            self.center.setNowPlayingInfo(Some(&dictionary));
            self.center.setPlaybackState(if info.playing {
                MPNowPlayingPlaybackState::Playing
            } else {
                MPNowPlayingPlaybackState::Paused
            });
        }
    }

    fn clear(&mut self) {
        // SAFETY: clearing is a plain setter call.
        unsafe {
            self.center.setNowPlayingInfo(None);
            self.center
                .setPlaybackState(MPNowPlayingPlaybackState::Stopped);
        }
    }
}
