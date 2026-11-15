// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The macOS side of the media keys: MPRemoteCommandCenter for the commands
//! and MPNowPlayingInfoCenter for what is shown in Control Center and on
//! the menu bar.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSImage;
use objc2_core_foundation::CGSize;
use objc2_foundation::{NSData, NSDictionary, NSNumber, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemArtwork, MPMediaItemPropertyAlbumTitle,
    MPMediaItemPropertyArtist, MPMediaItemPropertyArtwork, MPMediaItemPropertyPlaybackDuration,
    MPMediaItemPropertyTitle, MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyDefaultPlaybackRate,
    MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate,
    MPNowPlayingPlaybackState, MPRemoteCommand, MPRemoteCommandCenter, MPRemoteCommandEvent,
    MPRemoteCommandHandlerStatus,
};

use crate::media_keys::{
    InfoKey, InfoValue, NowPlaying, NowPlayingArtwork, NowPlayingSurface, RemoteCommand,
    info_entries,
};

/// The system's Now Playing, with the remote commands registered for as long
/// as it lives.
pub(crate) struct SystemNowPlaying {
    center: Retained<MPNowPlayingInfoCenter>,
    registered: Vec<(Retained<MPRemoteCommand>, Retained<AnyObject>)>,
    /// The cover last given, kept so that telling a new position or rate
    /// does not decode it again.
    artwork: Option<(NowPlayingArtwork, Retained<MPMediaItemArtwork>)>,
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
                artwork: None,
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

fn info_key(key: InfoKey) -> &'static NSString {
    // SAFETY: the property keys are the framework's own constants.
    unsafe {
        match key {
            InfoKey::Title => MPMediaItemPropertyTitle,
            InfoKey::Artist => MPMediaItemPropertyArtist,
            InfoKey::AlbumTitle => MPMediaItemPropertyAlbumTitle,
            InfoKey::PlaybackDuration => MPMediaItemPropertyPlaybackDuration,
            InfoKey::ElapsedPlaybackTime => MPNowPlayingInfoPropertyElapsedPlaybackTime,
            InfoKey::PlaybackRate => MPNowPlayingInfoPropertyPlaybackRate,
            InfoKey::DefaultPlaybackRate => MPNowPlayingInfoPropertyDefaultPlaybackRate,
            InfoKey::Artwork => MPMediaItemPropertyArtwork,
        }
    }
}

/// The framework's artwork for a cover. It hands the same image for every
/// size asked, and the system scales it.
fn media_artwork(artwork: &NowPlayingArtwork) -> Option<Retained<MPMediaItemArtwork>> {
    let data = NSData::with_bytes(artwork.png());
    let image = NSImage::initWithData(NSImage::alloc(), &data)?;
    let side = f64::from(artwork.side_pixels());
    let handler = RcBlock::new(move |_size: CGSize| -> NonNull<NSImage> { NonNull::from(&*image) });
    // SAFETY: the block returns the image it holds, which lives as long as
    // the block, and so as long as the artwork that keeps the block.
    Some(unsafe {
        MPMediaItemArtwork::initWithBoundsSize_requestHandler(
            MPMediaItemArtwork::alloc(),
            CGSize::new(side, side),
            &handler,
        )
    })
}

impl SystemNowPlaying {
    fn artwork_object(&mut self, artwork: &NowPlayingArtwork) -> Option<Retained<AnyObject>> {
        let cached = self
            .artwork
            .as_ref()
            .filter(|(kept, _)| kept == artwork)
            .map(|(_, object)| object.clone());
        let object = match cached {
            Some(object) => object,
            None => {
                let object = media_artwork(artwork)?;
                self.artwork = Some((artwork.clone(), object.clone()));
                object
            }
        };
        Some(Retained::into_super(Retained::into_super(object)))
    }
}

impl NowPlayingSurface for SystemNowPlaying {
    fn publish(&mut self, info: &NowPlaying) {
        let mut keys: Vec<&NSString> = Vec::new();
        let mut values: Vec<Retained<AnyObject>> = Vec::new();
        for (key, value) in info_entries(info) {
            let object = match value {
                InfoValue::Text(value) => text(&value),
                InfoValue::Number(value) => number(value),
                InfoValue::Artwork(artwork) => match self.artwork_object(&artwork) {
                    Some(object) => object,
                    None => continue,
                },
            };
            keys.push(info_key(key));
            values.push(object);
        }
        if info.artwork.is_none() {
            self.artwork = None;
        }
        let dictionary = NSDictionary::from_retained_objects(&keys, &values);
        // SAFETY: setting the info and the state are plain setter calls.
        unsafe {
            self.center.setNowPlayingInfo(Some(&dictionary));
            self.center.setPlaybackState(if info.playing {
                MPNowPlayingPlaybackState::Playing
            } else {
                MPNowPlayingPlaybackState::Paused
            });
        }
    }

    fn clear(&mut self) {
        self.artwork = None;
        // SAFETY: clearing is a plain setter call.
        unsafe {
            self.center.setNowPlayingInfo(None);
            self.center
                .setPlaybackState(MPNowPlayingPlaybackState::Stopped);
        }
    }
}
