// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cover art of the tracks being looked at, ready to draw. The picture
//! is read from the track's tags or its folder, cut to a square and reduced
//! to the two sizes the window draws, all off the UI thread, and kept in
//! memory by file, so that going back to a track draws at once. A track whose
//! file changed is read again. Nothing is read for a track nobody looks at.

use std::collections::VecDeque;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use euphoria_core::artwork::read_artwork;
use gpui::RenderImage;
use image::imageops::FilterType;
use image::{DynamicImage, Frame, GenericImageView, ImageReader, Limits};

/// Pixels on a side of the large thumbnail: sharp at 2x up to 96 points.
pub(crate) const LARGE_SIDE: u32 = 192;

/// Pixels on a side of the small thumbnail: sharp at 2x up to 40 points.
pub(crate) const SMALL_SIDE: u32 = 80;

/// How many covers are kept. A pair of thumbnails is about 175 KB, so the
/// limit holds the cache near 11 MB.
pub(crate) const ARTWORK_CAPACITY: usize = 64;

/// How long a looked up file is trusted to be unchanged before its
/// modification time is read again.
const RECHECK_INTERVAL: Duration = Duration::from_secs(2);

/// The most memory the decoder may use for one picture.
const DECODER_MEMORY_LIMIT_BYTES: u64 = 256 * 1024 * 1024;

/// Which of the two sizes to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThumbnailSize {
    /// For the inspector and the focus view, up to 96 points.
    Large,
    /// For the player bar and the decks, up to 40 points.
    Small,
}

/// The thumbnails of one cover.
#[derive(Clone)]
pub(crate) struct Cover {
    large: Arc<RenderImage>,
    small: Arc<RenderImage>,
}

impl Cover {
    /// The thumbnail of `size`.
    pub(crate) fn image(&self, size: ThumbnailSize) -> Arc<RenderImage> {
        match size {
            ThumbnailSize::Large => self.large.clone(),
            ThumbnailSize::Small => self.small.clone(),
        }
    }
}

impl std::fmt::Debug for Cover {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Cover")
    }
}

/// What the store knows about the cover of a file.
#[derive(Debug, Clone)]
pub(crate) enum Lookup {
    /// The cover, ready to draw.
    Ready(Cover),
    /// The file has no cover, or it could not be read: draw the placeholder.
    Absent,
    /// The cover is being read.
    Loading,
    /// Nothing is known yet: run the job off the UI thread and give its
    /// result to [`ArtworkStore::finish`]. The store shows `Loading` until then.
    Needed(Job),
}

#[derive(Clone)]
enum State {
    Loading,
    Absent,
    Ready(Cover),
}

struct Entry {
    path: PathBuf,
    modified: Option<SystemTime>,
    checked_at: Instant,
    state: State,
}

struct Inner {
    capacity: usize,
    recheck_interval: Duration,
    /// Least recently looked at first.
    entries: VecDeque<Entry>,
}

/// The covers kept in memory. Cloning shares them, so that a job finished in
/// the background and the window see the same ones.
#[derive(Clone)]
pub(crate) struct ArtworkStore {
    inner: Arc<Mutex<Inner>>,
    decodes: Arc<AtomicUsize>,
}

/// The work of reading and reducing one cover. It touches the disk and
/// decodes an image, so it belongs on a background thread.
#[derive(Clone)]
pub(crate) struct Job {
    path: PathBuf,
    modified: Option<SystemTime>,
    decodes: Arc<AtomicUsize>,
}

/// What a [`Job`] found.
pub(crate) struct Loaded {
    path: PathBuf,
    modified: Option<SystemTime>,
    cover: Option<Cover>,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Job")
            .field("path", &self.path)
            .finish()
    }
}

impl Job {
    /// Reads and reduces the cover. Call it off the UI thread.
    pub(crate) fn run(self) -> Loaded {
        self.decodes.fetch_add(1, Ordering::SeqCst);
        let cover = read_artwork(&self.path).and_then(|artwork| thumbnails(&artwork.bytes));
        Loaded {
            path: self.path,
            modified: self.modified,
            cover,
        }
    }
}

fn modified_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|data| data.modified())
        .ok()
}

impl ArtworkStore {
    /// An empty store keeping at most `capacity` covers.
    pub(crate) fn new(capacity: usize) -> Self {
        Self::with_recheck_interval(capacity, RECHECK_INTERVAL)
    }

    fn with_recheck_interval(capacity: usize, recheck_interval: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                capacity,
                recheck_interval,
                entries: VecDeque::with_capacity(capacity),
            })),
            decodes: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn locked(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// What is known of the cover of the file at `path`. It never reads a
    /// picture or decodes one: at most it looks at the file's modification
    /// time, and only when it has not been looked at for a couple of seconds.
    pub(crate) fn lookup(&self, path: &Path) -> Lookup {
        let mut inner = self.locked();
        let now = Instant::now();
        let position = inner.entries.iter().position(|entry| entry.path == path);
        if let Some(position) = position {
            let mut entry = inner
                .entries
                .remove(position)
                .expect("the position was just found");
            if now.duration_since(entry.checked_at) >= inner.recheck_interval {
                entry.checked_at = now;
                if modified_time(path) != entry.modified {
                    let modified = modified_time(path);
                    inner.entries.push_back(Entry {
                        path: path.to_path_buf(),
                        modified,
                        checked_at: now,
                        state: State::Loading,
                    });
                    return Lookup::Needed(self.job(path, modified));
                }
            }
            let lookup = match &entry.state {
                State::Loading => Lookup::Loading,
                State::Absent => Lookup::Absent,
                State::Ready(cover) => Lookup::Ready(cover.clone()),
            };
            inner.entries.push_back(entry);
            return lookup;
        }
        let modified = modified_time(path);
        inner.entries.push_back(Entry {
            path: path.to_path_buf(),
            modified,
            checked_at: now,
            state: State::Loading,
        });
        while inner.entries.len() > inner.capacity {
            inner.entries.pop_front();
        }
        Lookup::Needed(self.job(path, modified))
    }

    fn job(&self, path: &Path, modified: Option<SystemTime>) -> Job {
        Job {
            path: path.to_path_buf(),
            modified,
            decodes: self.decodes.clone(),
        }
    }

    /// Keeps what a job found, unless the file changed again or was
    /// forgotten while the job ran.
    pub(crate) fn finish(&self, loaded: Loaded) {
        let mut inner = self.locked();
        let entry = inner
            .entries
            .iter_mut()
            .find(|entry| entry.path == loaded.path);
        if let Some(entry) = entry
            && matches!(entry.state, State::Loading)
            && entry.modified == loaded.modified
        {
            entry.state = match loaded.cover {
                Some(cover) => State::Ready(cover),
                None => State::Absent,
            };
        }
    }

    /// How many covers were read and decoded so far.
    #[cfg(test)]
    pub(crate) fn decode_count(&self) -> usize {
        self.decodes.load(Ordering::SeqCst)
    }

    /// How many covers are kept.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.locked().entries.len()
    }
}

/// The two thumbnails of an encoded picture: its centre, as a square, at
/// each size. `None` when it is not a picture that can be decoded.
pub(crate) fn thumbnails(bytes: &[u8]) -> Option<Cover> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(DECODER_MEMORY_LIMIT_BYTES);
    reader.limits(limits);
    let picture = reader.decode().ok()?;
    let (width, height) = picture.dimensions();
    let side = width.min(height);
    if side == 0 {
        return None;
    }
    let centre = picture.crop_imm((width - side) / 2, (height - side) / 2, side, side);
    let large = centre.resize_exact(LARGE_SIDE, LARGE_SIDE, FilterType::Triangle);
    let small = large.resize_exact(SMALL_SIDE, SMALL_SIDE, FilterType::Triangle);
    Some(Cover {
        large: render_image(large),
        small: render_image(small),
    })
}

fn render_image(picture: DynamicImage) -> Arc<RenderImage> {
    let mut pixels = picture.into_rgba8();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![Frame::new(pixels)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgb, RgbImage};
    use std::fs;

    pub(crate) fn encoded_picture(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
        let picture = RgbImage::from_fn(width, height, |x, y| {
            Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
        });
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(picture)
            .write_to(&mut bytes, format)
            .expect("the picture encodes");
        bytes.into_inner()
    }

    fn scratch(test: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("euphoria-artwork-{test}"));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("the folder is made");
        folder
    }

    fn track_with_cover(folder: &Path) -> PathBuf {
        let track = folder.join("track.mp3");
        fs::write(&track, b"audio").expect("the track is written");
        fs::write(
            folder.join("cover.jpg"),
            encoded_picture(300, 200, ImageFormat::Jpeg),
        )
        .expect("the cover is written");
        track
    }

    fn immediate_store() -> ArtworkStore {
        ArtworkStore::with_recheck_interval(8, Duration::ZERO)
    }

    fn run(lookup: Lookup, store: &ArtworkStore) {
        let Lookup::Needed(job) = lookup else {
            panic!("expected a job, got {lookup:?}");
        };
        store.finish(job.run());
    }

    #[test]
    fn reduces_a_picture_to_a_square_at_each_size() {
        for format in [ImageFormat::Jpeg, ImageFormat::Png] {
            let cover = thumbnails(&encoded_picture(640, 400, format)).expect("it decodes");
            let large = cover.image(ThumbnailSize::Large);
            let small = cover.image(ThumbnailSize::Small);
            assert_eq!(large.size(0).width.0 as u32, LARGE_SIDE);
            assert_eq!(large.size(0).height.0 as u32, LARGE_SIDE);
            assert_eq!(small.size(0).width.0 as u32, SMALL_SIDE);
        }
    }

    #[test]
    fn something_that_is_not_a_picture_has_no_thumbnails() {
        assert!(thumbnails(b"not an image").is_none());
        assert!(thumbnails(b"").is_none());
    }

    #[test]
    fn a_lookup_never_decodes_and_the_job_decodes_off_the_calling_thread() {
        let folder = scratch("off-thread");
        let track = track_with_cover(&folder);
        let store = immediate_store();

        let lookup = store.lookup(&track);
        assert_eq!(store.decode_count(), 0, "looking up decodes nothing");
        let Lookup::Needed(job) = lookup else {
            panic!("a first lookup asks for a job");
        };
        assert!(matches!(store.lookup(&track), Lookup::Loading));
        assert_eq!(store.decode_count(), 0);

        let caller = std::thread::current().id();
        let worker = std::thread::spawn(move || {
            let loaded = job.run();
            (std::thread::current().id(), loaded)
        });
        let (worker_thread, loaded) = worker.join().expect("the worker finishes");
        assert_ne!(worker_thread, caller);
        assert_eq!(store.decode_count(), 1);
        store.finish(loaded);
        assert!(matches!(store.lookup(&track), Lookup::Ready(_)));
        assert_eq!(store.decode_count(), 1, "a hit does not decode again");
    }

    #[test]
    fn a_hit_by_path_and_modification_time_reuses_the_cover() {
        let folder = scratch("hit");
        let track = track_with_cover(&folder);
        let store = immediate_store();
        run(store.lookup(&track), &store);
        for _ in 0..50 {
            assert!(matches!(store.lookup(&track), Lookup::Ready(_)));
        }
        assert_eq!(store.decode_count(), 1);
    }

    #[test]
    fn a_changed_file_is_read_again() {
        let folder = scratch("changed");
        let track = track_with_cover(&folder);
        let store = immediate_store();
        run(store.lookup(&track), &store);
        assert!(matches!(store.lookup(&track), Lookup::Ready(_)));

        let later = SystemTime::now() + Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(&track)
            .expect("the track opens")
            .set_modified(later)
            .expect("the time is set");
        run(store.lookup(&track), &store);
        assert_eq!(store.decode_count(), 2);
        assert!(matches!(store.lookup(&track), Lookup::Ready(_)));
    }

    #[test]
    fn a_track_without_a_cover_is_absent_and_not_read_again() {
        let folder = scratch("absent");
        let track = folder.join("bare.mp3");
        fs::write(&track, b"audio").expect("the track is written");
        let store = immediate_store();
        run(store.lookup(&track), &store);
        for _ in 0..10 {
            assert!(matches!(store.lookup(&track), Lookup::Absent));
        }
        assert_eq!(store.decode_count(), 1);
    }

    #[test]
    fn a_result_for_a_file_that_changed_meanwhile_is_dropped() {
        let folder = scratch("stale");
        let track = track_with_cover(&folder);
        let store = immediate_store();
        let Lookup::Needed(stale_job) = store.lookup(&track) else {
            panic!("a first lookup asks for a job");
        };
        let later = SystemTime::now() + Duration::from_secs(120);
        fs::File::options()
            .write(true)
            .open(&track)
            .expect("the track opens")
            .set_modified(later)
            .expect("the time is set");
        assert!(matches!(store.lookup(&track), Lookup::Needed(_)));
        store.finish(stale_job.run());
        assert!(matches!(store.lookup(&track), Lookup::Loading));
    }

    #[test]
    fn keeps_at_most_its_capacity_and_forgets_the_least_recently_looked_at() {
        let store = ArtworkStore::with_recheck_interval(3, Duration::ZERO);
        let paths: Vec<PathBuf> = (0..5)
            .map(|number| PathBuf::from(format!("/nowhere/track-{number}.mp3")))
            .collect();
        for path in &paths {
            let _ = store.lookup(path);
            assert!(store.len() <= 3);
        }
        assert!(matches!(store.lookup(&paths[0]), Lookup::Needed(_)));
        assert!(matches!(store.lookup(&paths[4]), Lookup::Loading));
    }

    #[test]
    fn scrolling_a_two_thousand_track_library_stays_cheap() {
        let store = ArtworkStore::new(ARTWORK_CAPACITY);
        let paths: Vec<PathBuf> = (0..2_000)
            .map(|number| PathBuf::from(format!("/nowhere/track-{number}.mp3")))
            .collect();
        let started = Instant::now();
        for _ in 0..5 {
            for path in &paths {
                let _ = store.lookup(path);
            }
        }
        let elapsed = started.elapsed();
        assert_eq!(store.decode_count(), 0, "lookups decode nothing");
        assert!(store.len() <= ARTWORK_CAPACITY);
        assert!(
            elapsed < Duration::from_millis(500),
            "10,000 lookups took {elapsed:?}"
        );
    }

    #[test]
    fn decoding_a_large_cover_takes_well_under_a_second() {
        let bytes = encoded_picture(3_000, 3_000, ImageFormat::Jpeg);
        let started = Instant::now();
        assert!(thumbnails(&bytes).is_some());
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "a 3000 px cover took {elapsed:?}"
        );
    }
}
