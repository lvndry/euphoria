// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cues users set by hand, in a table of their own.
//!
//! Kept like corrections: under the content hash of the file, with no
//! analyzer version, so a new version of the analyzer keeps every set, and
//! carried to the new content of a file whose tags Euphoria writes. A track
//! with a set here has exactly those cues, even none; a track without one
//! has the cues analysis placed.

use std::path::Path;

use rusqlite::types::Type;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::corrections::unreadable;
use super::identity::ContentHash;
use super::{Cache, cache_error, seconds_since_epoch};
use crate::analysis::TrackAnalysis;
use crate::correction::Correction;
use crate::error::EuphoriaError;
use crate::track_cues::{Cue, HotCue, TrackCues, check_cues, check_cues_within};

/// One cue as the table stores it, in a JSON array.
#[derive(Serialize, Deserialize)]
struct StoredCue {
    seconds: f64,
    name: String,
    hot_cue: Option<char>,
}

impl Cache {
    /// The cues the user set for the file at `path`, if any.
    ///
    /// A file at the place an analysis recorded is matched without being
    /// read. Another file is hashed, but only while the cache holds any
    /// edited cues at all.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::Scan`] when the file must be hashed and cannot be
    /// read, and [`EuphoriaError::Cache`] when the database cannot be read
    /// or holds a set it cannot understand.
    pub fn edited_cues_for(&self, path: &Path) -> Result<Option<Vec<Cue>>, EuphoriaError> {
        if !self.has_edited_cues()? {
            return Ok(None);
        }
        let content_hash = self.content_at(path)?;
        self.edited_cues_of(&content_hash)
    }

    /// The cues the user set for the file at `path`, if any, after reading
    /// the whole file to be sure of its content, for export.
    ///
    /// # Errors
    ///
    /// As [`Cache::edited_cues_for`].
    pub fn verified_edited_cues_for(&self, path: &Path) -> Result<Option<Vec<Cue>>, EuphoriaError> {
        if !self.has_edited_cues()? {
            return Ok(None);
        }
        let content_hash = ContentHash::of_file(path).map_err(unreadable(path))?;
        self.edited_cues_of(&content_hash)
    }

    /// The correction and the cues the user set for the file at `path`,
    /// after reading the whole file once to be sure of its content, for tag
    /// writing and export. The file is not read when the cache holds no
    /// edit of either kind.
    ///
    /// # Errors
    ///
    /// As [`Cache::verified_correction_for`].
    pub fn verified_user_edits_for(
        &self,
        path: &Path,
    ) -> Result<(Option<Correction>, Option<Vec<Cue>>), EuphoriaError> {
        let has_corrections = self.has_corrections()?;
        let has_edited_cues = self.has_edited_cues()?;
        if !has_corrections && !has_edited_cues {
            return Ok((None, None));
        }
        let content_hash = ContentHash::of_file(path).map_err(unreadable(path))?;
        let correction = if has_corrections {
            self.correction_of(&content_hash)?
        } else {
            None
        };
        let cues = if has_edited_cues {
            self.edited_cues_of(&content_hash)?
        } else {
            None
        };
        Ok((correction, cues))
    }

    /// Keeps `cues` as the cues of the file at `path`, replacing any earlier
    /// set; an empty set keeps the track without cues. `None` forgets the
    /// user's set, so the track has the cues analysis placed again. The file
    /// is read once to hash it, unless an analysis recorded where it is.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::InvalidCues`] when [`check_cues`] refuses `cues`,
    /// or a cue lies past the end of the track as the cache recorded its
    /// length, [`EuphoriaError::Scan`] when the file cannot be read, and
    /// [`EuphoriaError::Cache`] when the database cannot be written.
    pub fn set_edited_cues(
        &mut self,
        path: &Path,
        cues: Option<&[Cue]>,
    ) -> Result<(), EuphoriaError> {
        if let Some(cues) = cues {
            check_cues(cues)?;
        }
        let content_hash = self.content_at(path)?;
        if let Some(cues) = cues
            && let Some(duration_seconds) = self.recorded_duration(&content_hash)?
        {
            check_cues_within(cues, duration_seconds)?;
        }
        let result = match cues {
            None => self.connection.execute(
                "DELETE FROM edited_cues WHERE content_hash = ?1",
                params![content_hash.as_bytes()],
            ),
            Some(cues) => self.connection.execute(
                "INSERT OR REPLACE INTO edited_cues (content_hash, cues, edited_at)
                 VALUES (?1, ?2, ?3)",
                params![
                    content_hash.as_bytes(),
                    stored_text(cues).map_err(cache_error(&self.path))?,
                    seconds_since_epoch()
                ],
            ),
        };
        result.map(|_| ()).map_err(cache_error(&self.path))
    }

    /// The cues of the file at `path`: the user's set when it has one, and
    /// otherwise those analysis placed in `analysis`, which should carry any
    /// correction already. See [`TrackCues::of`].
    ///
    /// # Errors
    ///
    /// As [`Cache::edited_cues_for`].
    pub fn track_cues_for(
        &self,
        path: &Path,
        analysis: &TrackAnalysis,
    ) -> Result<TrackCues, EuphoriaError> {
        Ok(TrackCues::of(analysis, self.edited_cues_for(path)?))
    }

    /// The length an analysis of any version recorded for `content_hash`.
    fn recorded_duration(&self, content_hash: &ContentHash) -> Result<Option<f64>, EuphoriaError> {
        self.connection
            .query_row(
                "SELECT duration_seconds FROM analyses WHERE content_hash = ?1",
                params![content_hash.as_bytes()],
                |row| row.get(0),
            )
            .optional()
            .map_err(cache_error(&self.path))
    }

    fn has_edited_cues(&self) -> Result<bool, EuphoriaError> {
        self.connection
            .query_row("SELECT EXISTS (SELECT 1 FROM edited_cues)", [], |row| {
                row.get(0)
            })
            .map_err(cache_error(&self.path))
    }

    fn edited_cues_of(
        &self,
        content_hash: &ContentHash,
    ) -> Result<Option<Vec<Cue>>, EuphoriaError> {
        self.connection
            .query_row(
                "SELECT cues FROM edited_cues WHERE content_hash = ?1",
                params![content_hash.as_bytes()],
                |row| {
                    let text: String = row.get(0)?;
                    parse_stored(&text).ok_or_else(|| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            Type::Text,
                            format!("{text:?} is not a set of cues").into(),
                        )
                    })
                },
            )
            .optional()
            .map_err(cache_error(&self.path))
    }
}

fn stored_text(cues: &[Cue]) -> serde_json::Result<String> {
    let stored: Vec<StoredCue> = TrackCues::edited(cues.to_vec())
        .cues
        .into_iter()
        .map(|cue| StoredCue {
            seconds: cue.seconds,
            name: cue.name,
            hot_cue: cue.hot_cue.map(HotCue::letter),
        })
        .collect();
    serde_json::to_string(&stored)
}

/// Reconciles the stored cues of the content `analysis` describes, keeping
/// them within the track with the same rule [`TrackCues::of`] applies on
/// read, so no consumer can serve or export a cue past the end of a track
/// the cache has measured. Runs in the transaction that records the
/// analysis. A duration that is not a positive number of seconds is no
/// measurement, and the set is left as stored.
///
/// Only the set stored under `content_hash` is touched: another track's
/// analysis says nothing about this one's length.
///
/// # Errors
///
/// As [`Cache::edited_cues_of`].
pub(crate) fn reconcile_stored_cues_to_analysis(
    connection: &rusqlite::Connection,
    content_hash: &ContentHash,
    analysis: &TrackAnalysis,
) -> rusqlite::Result<()> {
    let end = f64::from(analysis.duration_seconds);
    if !(end.is_finite() && end > 0.0) {
        return Ok(());
    }
    let stored: Option<String> = connection
        .query_row(
            "SELECT cues FROM edited_cues WHERE content_hash = ?1",
            params![content_hash.as_bytes()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(stored) = stored else {
        return Ok(());
    };
    let Some(cues) = parse_stored(&stored) else {
        // The set is reported to the user as unread when it is read back,
        // so it is left as stored here.
        return Ok(());
    };
    let Ok(current) = stored_text(&TrackCues::within(cues, end).cues) else {
        return Ok(());
    };
    if current != stored {
        connection.execute(
            "UPDATE edited_cues SET cues = ?2 WHERE content_hash = ?1",
            params![content_hash.as_bytes(), current],
        )?;
    }
    Ok(())
}

fn parse_stored(text: &str) -> Option<Vec<Cue>> {
    let stored: Vec<StoredCue> = serde_json::from_str(text).ok()?;
    let cues = stored
        .into_iter()
        .map(|stored| {
            let hot_cue = match stored.hot_cue {
                Some(letter) => Some(HotCue::from_letter(letter)?),
                None => None,
            };
            Some(Cue {
                seconds: stored.seconds,
                name: stored.name,
                hot_cue,
            })
        })
        .collect::<Option<Vec<Cue>>>()?;
    check_cues(&cues).ok()?;
    Some(cues)
}

#[cfg(test)]
mod tests {
    use super::super::CachedAnalyzer;
    use super::super::test_support::TemporaryFolder;
    use super::*;
    use crate::detect::{CueSection, SectionCue};
    use crate::track_cues::CueOrigin;

    fn analysis() -> TrackAnalysis {
        TrackAnalysis {
            duration_seconds: 300.0,
            key: None,
            bpm: Some(124.0),
            energy: Some(7),
            tuning_cents: None,
            key_timeline: Vec::new(),
            quality: None,
            key_runner_up: None,
            key_score_margin: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            beat_grid: None,
            cues: vec![SectionCue {
                seconds: 0.5,
                section: CueSection::Start,
            }],
        }
    }

    fn open(folder: &TemporaryFolder) -> Cache {
        Cache::open(&folder.path().join("cache.sqlite3")).unwrap()
    }

    fn edited() -> Vec<Cue> {
        vec![
            Cue {
                seconds: 16.25,
                name: "Vocal \"in\"\tand\nout".to_string(),
                hot_cue: HotCue::from_letter('C'),
            },
            Cue {
                seconds: 200.0,
                name: String::new(),
                hot_cue: None,
            },
        ]
    }

    fn track_in(folder: &TemporaryFolder) -> std::path::PathBuf {
        let track = folder.path().join("track.mp3");
        std::fs::write(&track, b"audio").unwrap();
        track
    }

    #[test]
    fn edited_cues_are_kept_exactly_and_found_again_after_a_move() {
        let folder = TemporaryFolder::new();
        let track = track_in(&folder);
        let mut cache = open(&folder);
        cache.record_retagged_file(&track, &analysis()).unwrap();
        assert_eq!(cache.edited_cues_for(&track).unwrap(), None);

        cache.set_edited_cues(&track, Some(&edited())).unwrap();
        assert_eq!(
            open(&folder).edited_cues_for(&track).unwrap(),
            Some(edited())
        );
        let moved = folder.path().join("moved.mp3");
        std::fs::rename(&track, &moved).unwrap();
        assert_eq!(
            open(&folder).verified_edited_cues_for(&moved).unwrap(),
            Some(edited())
        );
    }

    #[test]
    fn cues_stored_before_the_length_is_recorded_are_kept_within_it() {
        let folder = TemporaryFolder::new();
        let track = track_in(&folder);
        let mut cache = open(&folder);

        // No analysis yet, so the length is unknown and the 999 s cue is
        // kept.
        let cues = vec![
            Cue {
                seconds: 16.25,
                name: "Vocal".to_string(),
                hot_cue: HotCue::from_letter('C'),
            },
            Cue {
                seconds: 999.0,
                name: "Past the end".to_string(),
                hot_cue: None,
            },
        ];
        cache.set_edited_cues(&track, Some(&cues)).unwrap();
        assert_eq!(cache.edited_cues_for(&track).unwrap().unwrap().len(), 2);

        // The analysis records the length: the cue past the end is dropped
        // from the stored set, and what is read back is the same set the
        // display would show.
        cache.record_retagged_file(&track, &analysis()).unwrap();
        let stored = cache.edited_cues_for(&track).unwrap().unwrap();
        assert_eq!(
            stored,
            vec![Cue {
                seconds: 16.25,
                name: "Vocal".to_string(),
                hot_cue: HotCue::from_letter('C'),
            }]
        );
        assert_eq!(
            cache.track_cues_for(&track, &analysis()).unwrap().cues,
            stored
        );
    }

    #[test]
    fn recording_one_tracks_length_does_not_truncate_another_tracks_cues() {
        let folder = TemporaryFolder::new();
        let short = folder.path().join("short.mp3");
        std::fs::write(&short, b"short audio").unwrap();
        let long = folder.path().join("long.mp3");
        std::fs::write(&long, b"long audio").unwrap();
        let mut cache = open(&folder);

        let mut short_analysis = analysis();
        short_analysis.duration_seconds = 100.0;
        let mut long_analysis = analysis();
        long_analysis.duration_seconds = 300.0;
        cache.record_retagged_file(&short, &short_analysis).unwrap();
        cache.record_retagged_file(&long, &long_analysis).unwrap();

        let short_cues = [Cue {
            seconds: 50.0,
            name: "Short end".to_string(),
            hot_cue: None,
        }];
        let long_cues = [Cue {
            seconds: 200.0,
            name: "Long end".to_string(),
            hot_cue: None,
        }];
        cache.set_edited_cues(&short, Some(&short_cues)).unwrap();
        cache.set_edited_cues(&long, Some(&long_cues)).unwrap();

        // Recording the shorter track's analysis again must not clamp the
        // longer track's cues to the shorter track's length.
        cache.record_retagged_file(&short, &short_analysis).unwrap();
        assert_eq!(
            cache.edited_cues_for(&long).unwrap(),
            Some(long_cues.to_vec())
        );
    }

    #[test]
    fn the_users_set_wins_and_resetting_brings_back_the_automatic_cues() {
        let folder = TemporaryFolder::new();
        let track = track_in(&folder);
        let mut cache = open(&folder);
        let automatic = cache.track_cues_for(&track, &analysis()).unwrap();
        assert_eq!(automatic.origin, CueOrigin::Automatic);
        assert_eq!(automatic.cues[0].name, "Start");

        cache.set_edited_cues(&track, Some(&[])).unwrap();
        let none = cache.track_cues_for(&track, &analysis()).unwrap();
        assert_eq!(none.origin, CueOrigin::Edited);
        assert!(none.cues.is_empty(), "deleting every cue is a set too");

        cache.set_edited_cues(&track, None).unwrap();
        assert_eq!(
            cache.track_cues_for(&track, &analysis()).unwrap(),
            automatic
        );
    }

    #[test]
    fn a_set_with_two_cues_on_one_pad_is_refused_and_nothing_is_kept() {
        let folder = TemporaryFolder::new();
        let track = track_in(&folder);
        let mut cache = open(&folder);
        let mut shared_pad = edited();
        shared_pad[1].hot_cue = HotCue::from_letter('C');
        assert!(matches!(
            cache.set_edited_cues(&track, Some(&shared_pad)),
            Err(EuphoriaError::InvalidCues { .. })
        ));
        assert_eq!(cache.edited_cues_for(&track).unwrap(), None);
    }

    #[test]
    fn a_cue_past_the_recorded_end_of_the_track_is_refused_and_nothing_is_kept() {
        let folder = TemporaryFolder::new();
        let track = track_in(&folder);
        let mut cache = open(&folder);
        cache.record_retagged_file(&track, &analysis()).unwrap();
        let past_the_end = [Cue {
            seconds: 999.0,
            name: "Lost".to_string(),
            hot_cue: None,
        }];
        assert!(matches!(
            cache.set_edited_cues(&track, Some(&past_the_end)),
            Err(EuphoriaError::InvalidCues { .. })
        ));
        assert_eq!(cache.edited_cues_for(&track).unwrap(), None);
        cache.set_edited_cues(&track, Some(&edited())).unwrap();
    }

    #[test]
    fn a_new_analyzer_version_and_clearing_the_cache_keep_every_set() {
        let folder = TemporaryFolder::new();
        let track = track_in(&folder);
        let mut cache = open(&folder);
        cache.record_retagged_file(&track, &analysis()).unwrap();
        cache.set_edited_cues(&track, Some(&edited())).unwrap();
        cache
            .connection
            .execute("DELETE FROM analyses", [])
            .unwrap();
        assert_eq!(cache.edited_cues_for(&track).unwrap(), Some(edited()));
        drop(cache);
        assert!(super::super::clear_cache(&folder.path().join("cache.sqlite3")).unwrap());
        assert_eq!(
            open(&folder).edited_cues_for(&track).unwrap(),
            Some(edited())
        );
    }

    #[test]
    fn edited_cues_follow_their_file_through_a_tag_write_and_an_undo() {
        let folder = TemporaryFolder::new();
        let track = track_in(&folder);
        let original = std::fs::read(&track).unwrap();
        let mut cache = open(&folder);
        cache.record_retagged_file(&track, &analysis()).unwrap();
        cache.set_edited_cues(&track, Some(&edited())).unwrap();

        std::fs::write(&track, b"audio with a tag").unwrap();
        cache.record_retagged_file(&track, &analysis()).unwrap();
        assert_eq!(
            open(&folder).verified_edited_cues_for(&track).unwrap(),
            Some(edited())
        );

        std::fs::write(&track, &original).unwrap();
        assert!(cache.recognise_file(&track).unwrap());
        assert_eq!(
            open(&folder).verified_edited_cues_for(&track).unwrap(),
            Some(edited())
        );
    }

    #[test]
    fn a_damaged_set_is_an_error_and_not_a_panic() {
        let folder = TemporaryFolder::new();
        let track = track_in(&folder);
        let mut cache = open(&folder);
        cache.set_edited_cues(&track, Some(&edited())).unwrap();
        for damaged in [
            "not json",
            r#"[{"seconds": -1, "name": "", "hot_cue": null}]"#,
            r#"[{"seconds": 1, "name": "", "hot_cue": "Z"}]"#,
        ] {
            cache
                .connection
                .execute("UPDATE edited_cues SET cues = ?1", params![damaged])
                .unwrap();
            assert!(
                matches!(
                    cache.edited_cues_for(&track),
                    Err(EuphoriaError::Cache { .. })
                ),
                "{damaged}"
            );
        }
    }

    #[test]
    fn the_analyzer_reads_and_writes_edited_cues_too() {
        let folder = TemporaryFolder::new();
        let track = track_in(&folder);
        let analyzer = CachedAnalyzer::new(open(&folder)).unwrap();
        analyzer.set_edited_cues(&track, Some(&edited())).unwrap();
        assert_eq!(analyzer.edited_cues_for(&track).unwrap(), Some(edited()));
        assert_eq!(
            analyzer.track_cues_for(&track, &analysis()).unwrap().cues,
            edited()
        );
    }

    #[test]
    fn a_library_without_edited_cues_never_reads_its_files_for_them() {
        let folder = TemporaryFolder::new();
        let missing = folder.path().join("not here.mp3");
        assert_eq!(open(&folder).edited_cues_for(&missing).unwrap(), None);
    }
}
