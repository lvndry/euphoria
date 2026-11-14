// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing separated stems out of the cache: the four stems as WAV files,
//! or the acapella or the instrumental mixed from them, optionally in
//! another key.

use std::io;
use std::path::{Path, PathBuf};

use super::wav::WavWriter;
use super::{STEMS_CHANNELS, STEMS_SAMPLE_RATE, Stem, StemFiles};
use crate::audio_stream::AudioStream;
use crate::error::EuphoriaError;
use crate::export::rekordbox::ExistingFile;
use crate::key::Key;
use crate::tags::{TagValues, TagWriteOptions, apply_plan, plan_write};

/// A mix of the four stems that is written as one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mixdown {
    /// The vocals alone.
    Acapella,
    /// Drums, bass and the rest, without the vocals.
    Instrumental,
}

impl Mixdown {
    /// The stems that are added up.
    pub fn stems(self) -> &'static [Stem] {
        match self {
            Mixdown::Acapella => &[Stem::Vocals],
            Mixdown::Instrumental => &[Stem::Drums, Stem::Bass, Stem::Other],
        }
    }

    /// The name shown to people, such as `Acapella`.
    pub fn label(self) -> &'static str {
        match self {
            Mixdown::Acapella => "Acapella",
            Mixdown::Instrumental => "Instrumental",
        }
    }
}

/// The key change an export is made in: every stem is shifted by
/// `semitones` with the pitch shifter of shifted copies, and the files are
/// tagged with the key of the original moved by the same amount.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StemShift {
    /// How far to move the key, within the shifts a track allows.
    pub semitones: i8,
    /// The tags of the original: its key is the one that is shifted.
    pub values: TagValues,
}

/// What [`export_mix`] wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct MixExport {
    /// The file written.
    pub path: PathBuf,
    /// Samples per channel.
    pub frames: u64,
    /// The highest sample of the sum, as a fraction of full scale, before
    /// any scaling down.
    pub peak: f32,
    /// What the sum was multiplied by to keep it from clipping, when it
    /// would have. At most 1.
    pub gain: Option<f32>,
    /// The key the file is tagged with, when the export was shifted and the
    /// original key was known.
    pub key: Option<Key>,
}

/// What [`export_stems`] wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct StemsExport {
    /// The key the files are tagged with, when the export was shifted and
    /// the original key was known.
    pub key: Option<Key>,
}

fn existing_file(replace: bool) -> ExistingFile {
    if replace {
        ExistingFile::Replace
    } else {
        ExistingFile::Keep
    }
}

fn file_error(path: &Path) -> impl FnOnce(io::Error) -> EuphoriaError + '_ {
    |source| EuphoriaError::ExportFile {
        path: path.to_path_buf(),
        source,
    }
}

/// Stages every stem before replacing any output. A failed publication
/// removes the new files and restores the previous directory entries.
/// Without `replace`, a file already at a destination is kept and the
/// publication fails.
pub fn publish_stems(
    files: &StemFiles,
    destinations: &[(Stem, PathBuf)],
    replace: bool,
) -> Result<(), EuphoriaError> {
    publish_stems_using(files, destinations, replace, install_stem)
}

pub(crate) fn install_stem(
    temporary: tempfile::NamedTempFile,
    destination: &Path,
    replace: bool,
) -> io::Result<()> {
    if replace {
        temporary.persist(destination)
    } else {
        temporary.persist_noclobber(destination)
    }
    .map(|_| ())
    .map_err(|error| error.error)
}

pub(crate) fn publish_stems_using(
    files: &StemFiles,
    destinations: &[(Stem, PathBuf)],
    replace: bool,
    mut install: impl FnMut(tempfile::NamedTempFile, &Path, bool) -> io::Result<()>,
) -> Result<(), EuphoriaError> {
    let mut staged = Vec::new();
    for (stem, destination) in destinations {
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(file_error(destination))?;
        let mut source =
            std::fs::File::open(files.path(*stem)).map_err(|source| EuphoriaError::StemFiles {
                path: files.path(*stem),
                source,
            })?;
        io::copy(&mut source, temporary.as_file_mut()).map_err(file_error(destination))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(file_error(destination))?;
        match std::fs::symlink_metadata(destination) {
            Ok(metadata) if !replace || metadata.is_dir() => {
                return Err(EuphoriaError::ExportDestinationExists {
                    path: destination.clone(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(file_error(destination)(error)),
        }
        staged.push(temporary);
    }
    let parent = destinations
        .first()
        .and_then(|(_, path)| path.parent())
        .unwrap_or_else(|| Path::new("."));
    let backups = tempfile::Builder::new()
        .prefix(".euphoria-stems-")
        .tempdir_in(parent)
        .map_err(file_error(parent))?;
    let mut saved = Vec::new();
    let mut published = Vec::new();
    let result: Result<(), EuphoriaError> = (|| {
        if replace {
            for (index, (_, destination)) in destinations.iter().enumerate() {
                match std::fs::symlink_metadata(destination) {
                    Ok(metadata) if metadata.is_dir() => {
                        return Err(EuphoriaError::ExportDestinationExists {
                            path: destination.clone(),
                        });
                    }
                    Ok(_) => {
                        let backup = backups.path().join(index.to_string());
                        std::fs::rename(destination, &backup).map_err(file_error(destination))?;
                        saved.push((destination, backup));
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(file_error(destination)(error)),
                }
            }
        }
        for ((_, destination), temporary) in destinations.iter().zip(staged) {
            install(temporary, destination, replace).map_err(file_error(destination))?;
            published.push(destination);
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut rollback_errors = Vec::new();
        for destination in published.into_iter().rev() {
            if let Err(error) = std::fs::remove_file(destination) {
                rollback_errors.push(error);
            }
        }
        for (destination, backup) in saved.into_iter().rev() {
            if let Err(error) = std::fs::rename(backup, destination) {
                rollback_errors.push(error);
            }
        }
        if !rollback_errors.is_empty() {
            let retained = backups.keep();
            return Err(EuphoriaError::ExportFile {
                path: retained.clone(),
                source: io::Error::other(format!(
                    "{}; could not completely restore the previous stems: {rollback_errors:?}; backups kept at {}",
                    error.chain(),
                    retained.display()
                )),
            });
        }
        return Err(error);
    }
    Ok(())
}

/// Writes the four stems of `files` to `destinations`, 16 bit WAV files.
/// Unshifted, they are the cached stems exactly, written by
/// [`publish_stems`]. With `shift`, every stem is shifted first and tagged
/// with the new key, and the files are as long as the stems.
pub fn export_stems(
    files: &StemFiles,
    destinations: &[(Stem, PathBuf)],
    replace: bool,
    shift: Option<&StemShift>,
) -> Result<StemsExport, EuphoriaError> {
    let Some(shift) = shift else {
        publish_stems(files, destinations, replace)?;
        return Ok(StemsExport { key: None });
    };
    let parent = destinations
        .first()
        .and_then(|(_, path)| path.parent())
        .unwrap_or_else(|| Path::new("."));
    let staging = tempfile::Builder::new()
        .prefix(".euphoria-stems-shifted-")
        .tempdir_in(parent)
        .map_err(file_error(parent))?;
    let shifted_files = StemFiles::in_directory(staging.path().to_path_buf());
    let mut key = None;
    for (stem, _) in destinations {
        let samples = shifted_samples(&files.path(*stem), shift.semitones)?;
        let path = shifted_files.path(*stem);
        write_samples(&path, &samples)?;
        key = tag_shifted(&path, shift)?;
    }
    publish_stems(&shifted_files, destinations, replace)?;
    Ok(StemsExport { key })
}

/// Adds up the stems of `mixdown` in 32 bit floating point and writes the
/// sum to `destination` as a 16 bit WAV file. A sum that would go past full
/// scale is scaled down as a whole, and [`MixExport::gain`] says by how
/// much. With `shift`, each stem is shifted before they are added, and the
/// file is tagged with the new key.
pub fn export_mix(
    files: &StemFiles,
    mixdown: Mixdown,
    destination: &Path,
    replace: bool,
    shift: Option<&StemShift>,
) -> Result<MixExport, EuphoriaError> {
    if !replace && std::fs::symlink_metadata(destination).is_ok() {
        return Err(EuphoriaError::ExportDestinationExists {
            path: destination.to_path_buf(),
        });
    }
    let mut sum: Vec<f32> = Vec::new();
    for stem in mixdown.stems() {
        let stem_path = files.path(*stem);
        let samples = match shift {
            Some(shift) => shifted_samples(&stem_path, shift.semitones)?,
            None => read_samples(&stem_path)?,
        };
        if sum.is_empty() {
            sum = samples;
        } else if sum.len() == samples.len() {
            for (total, sample) in sum.iter_mut().zip(&samples) {
                *total += sample;
            }
        } else {
            return Err(EuphoriaError::StemFiles {
                path: stem_path,
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the stems are not the same length",
                ),
            });
        }
    }
    let peak = sum
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
    let gain = (peak > 1.0).then(|| 1.0 / peak);
    if let Some(gain) = gain {
        for sample in &mut sum {
            *sample *= gain;
        }
    }
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let temporary = tempfile::Builder::new()
        .prefix(".euphoria-mix-")
        .suffix(".wav")
        .tempfile_in(parent)
        .map_err(file_error(destination))?;
    write_samples(temporary.path(), &sum)?;
    let key = match shift {
        Some(shift) => tag_shifted(temporary.path(), shift)?,
        None => None,
    };
    crate::export::rekordbox::install(temporary.path(), destination, existing_file(replace))?;
    Ok(MixExport {
        path: destination.to_path_buf(),
        frames: (sum.len() / STEMS_CHANNELS) as u64,
        peak,
        gain,
        key,
    })
}

fn read_samples(path: &Path) -> Result<Vec<f32>, EuphoriaError> {
    let mut stream = AudioStream::open(path)?;
    let mut samples = Vec::new();
    let mut chunk = Vec::new();
    while stream.read_chunk(&mut chunk)? {
        samples.extend_from_slice(&chunk);
    }
    Ok(samples)
}

fn shifted_samples(path: &Path, semitones: i8) -> Result<Vec<f32>, EuphoriaError> {
    let failure = |reason: String| EuphoriaError::Shift {
        path: path.to_path_buf(),
        reason,
    };
    let samples = read_samples(path)?;
    let mut shifter = crate::shift::PitchShifter::new(STEMS_CHANNELS, STEMS_SAMPLE_RATE, semitones)
        .map_err(failure)?;
    let mut shifted = shifter.process(&samples).map_err(failure)?;
    shifted.extend(shifter.finish().map_err(failure)?);
    Ok(shifted)
}

fn write_samples(path: &Path, samples: &[f32]) -> Result<(), EuphoriaError> {
    let write = || -> io::Result<()> {
        let mut writer = WavWriter::create(
            path,
            samples.len() / STEMS_CHANNELS,
            STEMS_CHANNELS as u16,
            STEMS_SAMPLE_RATE,
        )?;
        writer.append(samples)?;
        writer.finish()
    };
    write().map_err(file_error(path))
}

fn tag_shifted(path: &Path, shift: &StemShift) -> Result<Option<Key>, EuphoriaError> {
    let key = shift.values.key.map(|key| key.transposed(shift.semitones));
    let tagged = TagValues {
        key,
        ..shift.values
    };
    plan_write(path, tagged, &TagWriteOptions::default()).and_then(|plan| apply_plan(&plan))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fixture_stems(directory: &Path, value: impl Fn(Stem, usize) -> f32) -> StemFiles {
        std::fs::create_dir_all(directory).unwrap();
        let files = StemFiles::in_directory(directory.to_path_buf());
        for stem in Stem::ALL {
            let samples: Vec<f32> = (0..2_000).map(|index| value(stem, index)).collect();
            super::super::wav::write_wav(&files.path(stem), &samples, 2, STEMS_SAMPLE_RATE)
                .unwrap();
        }
        files
    }

    fn fixture_files(folder: &Path) -> StemFiles {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sine-440hz-stereo.wav");
        let directory = folder.join("stems");
        std::fs::create_dir_all(&directory).unwrap();
        let files = StemFiles::in_directory(directory);
        for stem in Stem::ALL {
            std::fs::copy(&fixture, files.path(stem)).unwrap();
        }
        files
    }

    #[test]
    fn an_output_created_after_the_preflight_check_is_not_replaced() {
        let folder = tempfile::tempdir().unwrap();
        let files = fixture_files(folder.path());
        let destination = folder.path().join("vocals.wav");
        std::fs::write(&destination, b"created by another writer").unwrap();
        let result = publish_stems(&files, &[(Stem::Vocals, destination.clone())], false);
        assert!(
            result.is_err(),
            "a late output must not be silently overwritten"
        );
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"created by another writer"
        );
    }

    #[test]
    fn a_late_publication_error_rolls_back_new_and_replaced_outputs() {
        let folder = tempfile::tempdir().unwrap();
        let files = fixture_files(folder.path());
        for replace in [false, true] {
            let output = folder.path().join(format!("output-{replace}"));
            std::fs::create_dir(&output).unwrap();
            let destinations: Vec<_> = Stem::ALL
                .iter()
                .map(|stem| (*stem, output.join(format!("{}.wav", stem.name()))))
                .collect();
            if replace {
                for (_, destination) in &destinations {
                    std::fs::write(destination, b"previous stem").unwrap();
                }
            }
            let mut installed = 0;
            let result = publish_stems_using(
                &files,
                &destinations,
                replace,
                |temporary, destination, replace| {
                    installed += 1;
                    if installed == 3 {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "simulated publication failure",
                        ));
                    }
                    install_stem(temporary, destination, replace)
                },
            );
            assert!(result.is_err());
            assert_eq!(installed, 3, "two outputs were installed before failure");
            for (_, destination) in destinations {
                if replace {
                    assert_eq!(std::fs::read(destination).unwrap(), b"previous stem");
                } else {
                    assert!(!destination.exists());
                }
            }
            assert_eq!(
                std::fs::read_dir(output).unwrap().count(),
                if replace { 4 } else { 0 }
            );
        }
    }

    #[test]
    fn a_mix_that_would_clip_is_scaled_down_as_a_whole() {
        let folder = tempfile::tempdir().unwrap();
        let files = write_fixture_stems(&folder.path().join("stems"), |stem, index| {
            if stem == Stem::Vocals {
                0.0
            } else if index == 10 {
                0.8
            } else {
                0.1
            }
        });
        let destination = folder.path().join("instrumental.wav");
        let export = export_mix(&files, Mixdown::Instrumental, &destination, false, None).unwrap();
        assert!((export.peak - 2.4).abs() < 0.001, "{}", export.peak);
        let gain = export.gain.unwrap();
        assert!((gain - 1.0 / 2.4).abs() < 0.001);
        let written = read_samples(&destination).unwrap();
        assert!((written[10] - 1.0).abs() < 0.001);
        assert!((written[0] - 0.3 * gain).abs() < 0.001);
    }

    #[test]
    fn a_mix_that_fits_is_not_scaled_and_an_existing_file_is_kept_unless_replacing() {
        let folder = tempfile::tempdir().unwrap();
        let files = write_fixture_stems(&folder.path().join("stems"), |_, _| 0.1);
        let destination = folder.path().join("acapella.wav");
        let export = export_mix(&files, Mixdown::Acapella, &destination, false, None).unwrap();
        assert_eq!(export.gain, None);
        assert!(export_mix(&files, Mixdown::Acapella, &destination, false, None).is_err());
        export_mix(&files, Mixdown::Acapella, &destination, true, None).unwrap();
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 2);
    }
}
