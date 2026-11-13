// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Separated stems kept on disk, one folder per track content and model,
//! within a size limit: when the stems take more room than the limit, the
//! folders used longest ago are removed first.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::wav::WavWriter;
use super::{
    STEMS_CHANNELS, STEMS_MODEL, STEMS_SAMPLE_RATE, SeparationObserver, SeparationStage, Stem,
    StemSink, Stems,
};
use crate::cache::ContentHash;
use crate::error::EuphoriaError;

/// The most room the stem cache takes before the stems used longest ago
/// are removed: 5 GB, about 25 tracks of five minutes.
pub const DEFAULT_STEM_CACHE_LIMIT_BYTES: u64 = 5_000_000_000;
/// Environment variable setting the limit of the stem cache in gigabytes,
/// such as `20` or `0.5`.
pub const STEM_CACHE_LIMIT_VARIABLE: &str = "EUPHORIA_STEMS_CACHE_GB";

const SEPARATION_LOCK_FILE_NAME: &str = ".separating.lock";
const LAST_USED_FILE_NAME: &str = "last-used";
const INCOMING_PREFIX: &str = ".incoming-";

/// The folder of stems kept on disk.
#[derive(Debug, Clone)]
pub struct StemCache {
    directory: PathBuf,
    limit_bytes: u64,
}

/// The four WAV files of a track's stems in the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StemFiles {
    directory: PathBuf,
}

impl StemFiles {
    /// The stem files in `directory`, named as the cache names them.
    pub fn in_directory(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// The 16 bit stereo WAV file of `stem`, at 44.1 kHz.
    pub fn path(&self, stem: Stem) -> PathBuf {
        self.directory.join(format!("{}.wav", stem.name()))
    }

    /// The folder holding the four files.
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

impl StemCache {
    /// The cache in the `stems` folder of Euphoria's data folder, with the
    /// limit from `EUPHORIA_STEMS_CACHE_GB` when it is set to a number, and
    /// [`DEFAULT_STEM_CACHE_LIMIT_BYTES`] otherwise.
    pub fn open_default() -> Result<Self, EuphoriaError> {
        let directory = crate::cache::location::default_data_directory()?.join("stems");
        let limit_bytes = std::env::var(STEM_CACHE_LIMIT_VARIABLE)
            .ok()
            .and_then(|value| limit_from_gigabytes(&value))
            .unwrap_or(DEFAULT_STEM_CACHE_LIMIT_BYTES);
        Ok(Self::new(directory, limit_bytes))
    }

    /// A cache in `directory` that keeps at most `limit_bytes` of stems.
    pub fn new(directory: PathBuf, limit_bytes: u64) -> Self {
        Self {
            directory,
            limit_bytes,
        }
    }

    /// The folder the stems are kept in.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The most room the stems take before old ones are removed.
    pub fn limit_bytes(&self) -> u64 {
        self.limit_bytes
    }

    fn entry_directory(&self, hash: &ContentHash) -> PathBuf {
        let hex: String = hash
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        self.directory.join(format!("{STEMS_MODEL}-{hex}"))
    }

    /// The stems of the track with content `hash`, when they are cached.
    /// Marks them as just used.
    pub fn lookup(&self, hash: &ContentHash) -> Option<StemFiles> {
        let files = self.cached(hash)?;
        let _ = mark_used(&files.directory);
        Some(files)
    }

    /// The stems of the track with content `hash`, when they are cached,
    /// without marking them as used: for checking which tracks have stems.
    pub fn cached(&self, hash: &ContentHash) -> Option<StemFiles> {
        let files = StemFiles {
            directory: self.entry_directory(hash),
        };
        Stem::ALL
            .iter()
            .all(|stem| files.path(*stem).is_file())
            .then_some(files)
    }

    /// Writes `stems` as the stems of the track with content `hash`, then
    /// removes the stems used longest ago until the cache fits its limit.
    /// The new stems are always kept, even when they alone exceed it.
    pub fn store(&self, hash: &ContentHash, stems: &Stems) -> Result<StemFiles, EuphoriaError> {
        self.store_streamed(hash, stems.frames, |sink| sink.write(&stems.interleaved))
    }

    /// Stores like [`StemCache::store`] stems `frames` long that `separate`
    /// hands to the sink it is given a block at a time, written to disk as
    /// they come. Nothing is kept when `separate` fails or hands over
    /// another length.
    pub(crate) fn store_streamed(
        &self,
        hash: &ContentHash,
        frames: usize,
        separate: impl FnOnce(&mut dyn StemSink) -> Result<(), EuphoriaError>,
    ) -> Result<StemFiles, EuphoriaError> {
        let stem_error = |path: &Path| {
            let path = path.to_path_buf();
            move |source| EuphoriaError::StemFiles { path, source }
        };
        fs::create_dir_all(&self.directory).map_err(stem_error(&self.directory))?;
        let incoming = tempfile::Builder::new()
            .prefix(INCOMING_PREFIX)
            .tempdir_in(&self.directory)
            .map_err(stem_error(&self.directory))?;
        let mut sink =
            WavStemSink::create(incoming.path(), frames).map_err(stem_error(incoming.path()))?;
        separate(&mut sink)?;
        sink.finish().map_err(stem_error(incoming.path()))?;
        mark_used(incoming.path()).map_err(stem_error(incoming.path()))?;
        let destination = self.entry_directory(hash);
        install_entry(incoming.path(), &destination).map_err(stem_error(&destination))?;
        self.evict(&destination)
            .map_err(stem_error(&self.directory))?;
        Ok(StemFiles {
            directory: destination,
        })
    }

    /// Waits until no other process separates into this cache, then holds
    /// the turn until the returned file is dropped. Separations run one at
    /// a time across every window and command, so that two never need the
    /// memory of the model twice.
    pub(crate) fn separation_turn(
        &self,
        observer: &dyn SeparationObserver,
    ) -> Result<File, EuphoriaError> {
        let lock_path = self.directory.join(SEPARATION_LOCK_FILE_NAME);
        let lock_error = |source| EuphoriaError::StemFiles {
            path: lock_path.clone(),
            source,
        };
        fs::create_dir_all(&self.directory).map_err(lock_error)?;
        let lock = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(lock_error)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                observer.stage(SeparationStage::Waiting);
                lock.lock().map_err(lock_error)?;
            }
            Err(fs::TryLockError::Error(source)) => return Err(lock_error(source)),
        }
        Ok(lock)
    }

    /// Removes the entries used longest ago until the total size is within
    /// the limit, never removing `keep`.
    fn evict(&self, keep: &Path) -> io::Result<()> {
        let mut entries = Vec::new();
        let mut total_bytes: u64 = 0;
        let prefix = format!("{STEMS_MODEL}-");
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let managed = name
                .to_str()
                .and_then(|name| name.strip_prefix(&prefix))
                .is_some_and(|hash| {
                    hash.len() == 64
                        && hash
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                });
            if !entry.file_type()?.is_dir() || !managed {
                continue;
            }
            let bytes = folder_bytes(&path)?;
            total_bytes += bytes;
            let last_used = fs::metadata(path.join(LAST_USED_FILE_NAME))
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            entries.push((last_used, bytes, path));
        }
        entries.sort();
        for (_, bytes, path) in entries {
            if total_bytes <= self.limit_bytes {
                break;
            }
            if path == keep {
                continue;
            }
            fs::remove_dir_all(&path)?;
            total_bytes -= bytes;
        }
        Ok(())
    }
}

fn install_entry(incoming: &Path, destination: &Path) -> io::Result<()> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let backup = tempfile::Builder::new()
        .prefix(".replaced-")
        .tempdir_in(parent)?;
    let saved = backup.path().join("entry");
    let had_previous = match fs::symlink_metadata(destination) {
        Ok(_) => {
            fs::rename(destination, &saved)?;
            true
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    if let Err(error) = fs::rename(incoming, destination) {
        if had_previous && let Err(restore) = fs::rename(&saved, destination) {
            let retained = backup.keep();
            return Err(io::Error::new(
                error.kind(),
                format!(
                    "{error}; could not restore the previous cache entry: {restore}; it is kept at {}",
                    retained.display()
                ),
            ));
        }
        return Err(error);
    }
    Ok(())
}

/// The four WAV files of a cache entry being written.
struct WavStemSink {
    directory: PathBuf,
    writers: Vec<WavWriter>,
}

impl WavStemSink {
    fn create(directory: &Path, frames: usize) -> io::Result<Self> {
        let writers = Stem::ALL
            .iter()
            .map(|stem| {
                WavWriter::create(
                    &directory.join(format!("{}.wav", stem.name())),
                    frames,
                    STEMS_CHANNELS as u16,
                    STEMS_SAMPLE_RATE,
                )
            })
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self {
            directory: directory.to_path_buf(),
            writers,
        })
    }

    fn finish(self) -> io::Result<()> {
        self.writers.into_iter().try_for_each(WavWriter::finish)
    }
}

impl StemSink for WavStemSink {
    fn write(&mut self, blocks: &[Vec<f32>; 4]) -> Result<(), EuphoriaError> {
        for (writer, block) in self.writers.iter_mut().zip(blocks) {
            writer
                .append(block)
                .map_err(|source| EuphoriaError::StemFiles {
                    path: self.directory.clone(),
                    source,
                })?;
        }
        Ok(())
    }
}

fn mark_used(directory: &Path) -> io::Result<()> {
    let file = File::options()
        .create(true)
        .append(true)
        .open(directory.join(LAST_USED_FILE_NAME))?;
    file.set_modified(SystemTime::now())
}

fn folder_bytes(directory: &Path) -> io::Result<u64> {
    let mut total = 0;
    for entry in fs::read_dir(directory)? {
        let metadata = entry?.metadata()?;
        if metadata.is_file() {
            total += metadata.len();
        }
    }
    Ok(total)
}

fn limit_from_gigabytes(value: &str) -> Option<u64> {
    let gigabytes: f64 = value.trim().parse().ok()?;
    (gigabytes.is_finite() && gigabytes >= 0.0).then_some((gigabytes * 1e9) as u64)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    struct TemporaryFolder(PathBuf);

    impl TemporaryFolder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("euphoria-stem-cache-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TemporaryFolder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn stems(frames: usize) -> Stems {
        Stems {
            frames,
            interleaved: Stem::ALL.map(|stem| vec![stem.index() as f32 / 10.0; frames * 2]),
        }
    }

    fn write_entry(directory: &Path, stems: &Stems) -> io::Result<()> {
        let mut sink = WavStemSink::create(directory, stems.frames)?;
        sink.write(&stems.interleaved)
            .map_err(|error| io::Error::other(error.to_string()))?;
        sink.finish()?;
        mark_used(directory)
    }

    fn hash(byte: u8) -> ContentHash {
        ContentHash::from_bytes([byte; 32])
    }

    fn set_last_used(files: &StemFiles, seconds_ago: u64) {
        File::options()
            .append(true)
            .open(files.directory().join(LAST_USED_FILE_NAME))
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(seconds_ago))
            .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn failed_installation_preserves_the_previous_entry() {
        use std::os::unix::fs::PermissionsExt;
        let folder = TemporaryFolder::new("failed-install");
        fs::create_dir_all(&folder.0).unwrap();
        let destination = folder.0.join("existing");
        let incoming = folder.0.join("incoming");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&incoming).unwrap();
        write_entry(&destination, &stems(10)).unwrap();
        write_entry(&incoming, &stems(20)).unwrap();
        let original = fs::read(destination.join("vocals.wav")).unwrap();
        fs::set_permissions(&folder.0, fs::Permissions::from_mode(0o555)).unwrap();
        let result = install_entry(&incoming, &destination);
        fs::set_permissions(&folder.0, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read(destination.join("vocals.wav")).unwrap(), original);
    }

    #[test]
    fn a_failed_rename_restores_the_previous_cache_entry() {
        let folder = TemporaryFolder::new("restore-install");
        let cache = StemCache::new(folder.0.clone(), u64::MAX);
        let previous = cache.store(&hash(8), &stems(10)).unwrap();
        let original = fs::read(previous.path(Stem::Vocals)).unwrap();
        assert!(install_entry(&folder.0.join("missing"), previous.directory()).is_err());
        assert_eq!(fs::read(previous.path(Stem::Vocals)).unwrap(), original);
        assert_eq!(cache.lookup(&hash(8)), Some(previous));
    }

    #[cfg(unix)]
    #[test]
    fn preexisting_incoming_symlinks_cannot_redirect_cache_writes() {
        let folder = TemporaryFolder::new("incoming-symlink");
        let outside = folder.0.join("outside");
        let directory = folder.0.join("cache");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir(&directory).unwrap();
        fs::write(outside.join("vocals.wav"), b"original bytes").unwrap();
        for number in 0..128 {
            std::os::unix::fs::symlink(
                &outside,
                directory.join(format!("{INCOMING_PREFIX}{}-{number}", std::process::id())),
            )
            .unwrap();
        }
        let cache = StemCache::new(directory, u64::MAX);
        let stored = cache.store(&hash(7), &stems(10)).unwrap();
        assert!(stored.path(Stem::Vocals).is_file());
        assert_eq!(
            fs::read(outside.join("vocals.wav")).unwrap(),
            b"original bytes"
        );
    }

    #[test]
    fn stored_stems_are_found_by_content() {
        let folder = TemporaryFolder::new("found");
        let cache = StemCache::new(folder.0.clone(), DEFAULT_STEM_CACHE_LIMIT_BYTES);
        assert_eq!(cache.lookup(&hash(1)), None);
        let stored = cache.store(&hash(1), &stems(100)).unwrap();
        assert_eq!(cache.lookup(&hash(1)), Some(stored.clone()));
        assert_eq!(cache.lookup(&hash(2)), None);
        for stem in Stem::ALL {
            assert_eq!(fs::metadata(stored.path(stem)).unwrap().len(), 44 + 400);
        }
        assert!(
            stored
                .directory()
                .ends_with(format!("{STEMS_MODEL}-{}", "01".repeat(32)))
        );
    }

    #[test]
    fn an_incomplete_entry_is_not_served() {
        let folder = TemporaryFolder::new("incomplete");
        let cache = StemCache::new(folder.0.clone(), DEFAULT_STEM_CACHE_LIMIT_BYTES);
        let stored = cache.store(&hash(3), &stems(10)).unwrap();
        fs::remove_file(stored.path(Stem::Vocals)).unwrap();
        assert_eq!(cache.lookup(&hash(3)), None);
    }

    #[test]
    fn the_stems_used_longest_ago_go_first_when_over_the_limit() {
        let folder = TemporaryFolder::new("evict");
        let entry_bytes = 4 * (44 + 4000);
        let cache = StemCache::new(folder.0.clone(), 2 * entry_bytes);
        let oldest = cache.store(&hash(1), &stems(1000)).unwrap();
        let middle = cache.store(&hash(2), &stems(1000)).unwrap();
        set_last_used(&oldest, 300);
        set_last_used(&middle, 200);
        assert!(cache.lookup(&hash(1)).is_some());
        let newest = cache.store(&hash(3), &stems(1000)).unwrap();
        assert!(cache.lookup(&hash(1)).is_some());
        assert_eq!(cache.lookup(&hash(2)), None);
        assert!(newest.directory().exists());
    }

    #[test]
    fn new_stems_are_kept_even_beyond_the_limit() {
        let folder = TemporaryFolder::new("beyond");
        let cache = StemCache::new(folder.0.clone(), 10);
        cache.store(&hash(1), &stems(100)).unwrap();
        let kept = cache.store(&hash(2), &stems(100)).unwrap();
        assert_eq!(cache.lookup(&hash(1)), None);
        assert_eq!(cache.lookup(&hash(2)), Some(kept));
    }

    #[test]
    fn eviction_never_deletes_an_unrelated_directory() {
        let folder = TemporaryFolder::new("unrelated");
        let personal = folder.0.join("personal");
        fs::create_dir_all(&personal).unwrap();
        fs::write(personal.join("keep.txt"), b"unrelated data").unwrap();
        let cache = StemCache::new(folder.0.clone(), 1);
        cache.store(&hash(1), &stems(10)).unwrap();
        assert_eq!(
            fs::read(personal.join("keep.txt")).unwrap(),
            b"unrelated data"
        );
    }

    #[test]
    fn a_second_separation_waits_for_the_first() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Heard(AtomicBool);
        impl SeparationObserver for Heard {
            fn stage(&self, stage: SeparationStage) {
                if stage == SeparationStage::Waiting {
                    self.0.store(true, Ordering::SeqCst);
                }
            }
        }
        let folder = TemporaryFolder::new("turns");
        let cache = StemCache::new(folder.0.clone(), DEFAULT_STEM_CACHE_LIMIT_BYTES);
        let first = cache
            .separation_turn(&Heard(AtomicBool::new(false)))
            .unwrap();
        let second_cache = cache.clone();
        let waiting = std::thread::spawn(move || {
            let heard = Heard(AtomicBool::new(false));
            let _turn = second_cache.separation_turn(&heard).unwrap();
            heard.0.load(Ordering::SeqCst)
        });
        std::thread::sleep(Duration::from_millis(100));
        assert!(!waiting.is_finished());
        drop(first);
        assert!(
            waiting.join().unwrap(),
            "the second separation never said it waited"
        );
    }

    #[test]
    fn the_limit_is_read_in_gigabytes() {
        assert_eq!(limit_from_gigabytes("20"), Some(20_000_000_000));
        assert_eq!(limit_from_gigabytes(" 0.5 "), Some(500_000_000));
        assert_eq!(limit_from_gigabytes("-1"), None);
        assert_eq!(limit_from_gigabytes("lots"), None);
    }
}
