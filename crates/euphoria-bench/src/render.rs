// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Renders a benchmark report as Markdown.

use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};

use crate::evaluate::{DetectorResult, Evaluation, Failure, Report};
use crate::scoring::{KeyTally, TempoTally};

/// A reason shared by more than this many tracks is listed once with a count.
const GROUPING_THRESHOLD: usize = 5;

/// Renders the whole report as Markdown.
pub fn render_report(report: &Report) -> String {
    MarkdownReport(report).to_string()
}

/// One line of a failure list.
#[derive(Debug, PartialEq, Eq)]
pub enum FailureLine<'report> {
    /// A single track and why it failed.
    Track(&'report Failure),
    /// A reason shared by many tracks.
    Grouped {
        /// The shared error chain.
        reason: &'report str,
        /// How many tracks failed with it.
        track_count: usize,
    },
}

/// Lists failures in their original order, folding every reason shared by
/// more than [`GROUPING_THRESHOLD`] tracks into one line where it first
/// appears.
pub fn group_failures(failures: &[Failure]) -> Vec<FailureLine<'_>> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for failure in failures {
        *counts.entry(failure.reason.as_str()).or_default() += 1;
    }
    let mut lines = Vec::new();
    for failure in failures {
        let reason = failure.reason.as_str();
        match counts.get(reason).copied() {
            Some(track_count) if track_count <= GROUPING_THRESHOLD => {
                lines.push(FailureLine::Track(failure));
            }
            Some(track_count) => {
                counts.remove(reason);
                lines.push(FailureLine::Grouped {
                    reason,
                    track_count,
                });
            }
            None => {}
        }
    }
    lines
}

struct MarkdownReport<'report>(&'report Report);

impl Display for MarkdownReport<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if let Some(key) = &self.0.key {
            write_key_section(formatter, key)?;
        }
        if let Some(tempo) = &self.0.tempo {
            write_tempo_section(formatter, tempo)?;
        }
        Ok(())
    }
}

fn write_key_section(
    formatter: &mut Formatter<'_>,
    evaluation: &Evaluation<KeyTally>,
) -> fmt::Result {
    writeln!(formatter, "## Key detection, GiantSteps Key\n")?;
    write_track_summary(formatter, evaluation)?;
    writeln!(
        formatter,
        "| Detector | Exact | Fifth | Relative | Parallel | Other | No key or error | MIREX score | CPU seconds per track |"
    )?;
    writeln!(
        formatter,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    )?;
    for detector in &evaluation.detectors {
        let tally = &detector.tally;
        writeln!(
            formatter,
            "| {} | {} | {} | {} | {} | {} | {} | {:.1}% | {} |",
            detector.name,
            tally.percent(tally.exact),
            tally.percent(tally.fifth),
            tally.percent(tally.relative),
            tally.percent(tally.parallel),
            tally.percent(tally.other),
            tally.percent(tally.missing),
            tally.mirex_score() * 100.0,
            CpuSeconds(detector),
        )?;
    }
    writeln!(formatter)?;
    write_failures(formatter, evaluation)
}

fn write_tempo_section(
    formatter: &mut Formatter<'_>,
    evaluation: &Evaluation<TempoTally>,
) -> fmt::Result {
    writeln!(formatter, "## Tempo detection, GiantSteps Tempo\n")?;
    write_track_summary(formatter, evaluation)?;
    writeln!(
        formatter,
        "| Detector | Accuracy 1 | Accuracy 2 | No tempo or error | CPU seconds per track |"
    )?;
    writeln!(formatter, "| --- | --- | --- | --- | --- |")?;
    for detector in &evaluation.detectors {
        let tally = &detector.tally;
        writeln!(
            formatter,
            "| {} | {} | {} | {} | {} |",
            detector.name,
            tally.percent(tally.accurate),
            tally.percent(tally.accurate_with_octaves),
            tally.percent(tally.missing),
            CpuSeconds(detector),
        )?;
    }
    writeln!(formatter)?;
    writeln!(
        formatter,
        "Accuracy 1 counts an estimate within 4% of the annotated tempo. Accuracy 2 also accepts 2, 1/2, 3 and 1/3 times the annotated tempo, as in the standard tempo evaluation.\n"
    )?;
    write_failures(formatter, evaluation)
}

fn write_track_summary<Scores>(
    formatter: &mut Formatter<'_>,
    evaluation: &Evaluation<Scores>,
) -> fmt::Result {
    let failed = evaluation.decode_failures.len();
    write!(
        formatter,
        "{} tracks with audio, {} decoded, {} failed to decode.",
        evaluation.track_count,
        evaluation.track_count - failed,
        failed
    )?;
    if evaluation.unannotated_count > 0 {
        write!(
            formatter,
            " {} more left out because their annotation has no reference value.",
            evaluation.unannotated_count
        )?;
    }
    writeln!(formatter, "\n")
}

fn write_failures<Scores>(
    formatter: &mut Formatter<'_>,
    evaluation: &Evaluation<Scores>,
) -> fmt::Result {
    write_failure_list(
        formatter,
        "Tracks that failed to decode:",
        &evaluation.decode_failures,
    )?;
    for detector in &evaluation.detectors {
        let heading = format!("Tracks where {} failed:", detector.name);
        write_failure_list(formatter, &heading, &detector.failures)?;
    }
    Ok(())
}

fn write_failure_list(
    formatter: &mut Formatter<'_>,
    heading: &str,
    failures: &[Failure],
) -> fmt::Result {
    if failures.is_empty() {
        return Ok(());
    }
    writeln!(formatter, "{heading}\n")?;
    for line in group_failures(failures) {
        match line {
            FailureLine::Track(failure) => {
                writeln!(formatter, "- {}: {}", failure.track, failure.reason)?;
            }
            FailureLine::Grouped {
                reason,
                track_count,
            } => writeln!(formatter, "- {track_count} tracks: {reason}")?,
        }
    }
    writeln!(formatter)
}

struct CpuSeconds<'result, Scores>(&'result DetectorResult<Scores>);

impl<Scores> Display for CpuSeconds<'_, Scores> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self.0.cpu_seconds_per_track() {
            Some(seconds) => write!(formatter, "{seconds:.2}"),
            None => formatter.write_str("not measured"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn failure(track: &str, reason: &str) -> Failure {
        Failure {
            track: track.to_string(),
            reason: reason.to_string(),
        }
    }

    fn detector<Scores>(
        name: &'static str,
        tally: Scores,
        measured_tracks: usize,
        failures: Vec<Failure>,
    ) -> DetectorResult<Scores> {
        DetectorResult {
            name,
            tally,
            total_cpu_time: Duration::from_secs(3),
            measured_tracks,
            failures,
        }
    }

    fn sample_report() -> Report {
        let mut key_tally = KeyTally::default();
        key_tally.record("A minor".parse().unwrap(), "A minor".parse().unwrap());
        key_tally.record_missing();
        let mut tempo_tally = TempoTally::default();
        tempo_tally.record(120.0, 121.0);
        tempo_tally.record(120.0, 60.0);

        Report {
            key: Some(Evaluation {
                track_count: 3,
                unannotated_count: 0,
                decode_failures: vec![failure("broken", "cannot decode broken.mp3: read error")],
                detectors: vec![detector(
                    "libkeyfinder",
                    key_tally,
                    2,
                    vec![failure("quiet", "libkeyfinder failed: no chroma")],
                )],
            }),
            tempo: Some(Evaluation {
                track_count: 2,
                unannotated_count: 3,
                decode_failures: Vec::new(),
                detectors: vec![detector("stratum-dsp", tempo_tally, 2, Vec::new())],
            }),
        }
    }

    #[test]
    fn renders_tables_notes_and_failures() {
        let expected = "\
## Key detection, GiantSteps Key

3 tracks with audio, 2 decoded, 1 failed to decode.

| Detector | Exact | Fifth | Relative | Parallel | Other | No key or error | MIREX score | CPU seconds per track |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| libkeyfinder | 50.0% | 0.0% | 0.0% | 0.0% | 0.0% | 50.0% | 50.0% | 1.50 |

Tracks that failed to decode:

- broken: cannot decode broken.mp3: read error

Tracks where libkeyfinder failed:

- quiet: libkeyfinder failed: no chroma

## Tempo detection, GiantSteps Tempo

2 tracks with audio, 2 decoded, 0 failed to decode. 3 more left out because their annotation has no reference value.

| Detector | Accuracy 1 | Accuracy 2 | No tempo or error | CPU seconds per track |
| --- | --- | --- | --- | --- |
| stratum-dsp | 50.0% | 100.0% | 0.0% | 1.50 |

Accuracy 1 counts an estimate within 4% of the annotated tempo. Accuracy 2 also accepts 2, 1/2, 3 and 1/3 times the annotated tempo, as in the standard tempo evaluation.

";
        assert_eq!(render_report(&sample_report()), expected);
    }

    #[test]
    fn renders_only_the_benchmarks_that_were_run() {
        let report = Report {
            key: None,
            ..sample_report()
        };

        let rendered = render_report(&report);

        assert!(rendered.starts_with("## Tempo detection, GiantSteps Tempo\n"));
        assert!(!rendered.contains("Key detection"));
    }

    #[test]
    fn reports_unmeasured_cpu_time_without_dividing_by_zero() {
        let result = detector("stratum-dsp", TempoTally::default(), 0, Vec::new());
        assert_eq!(CpuSeconds(&result).to_string(), "not measured");
    }

    #[test]
    fn groups_reasons_shared_by_more_than_five_tracks() {
        let mut failures = vec![failure("first", "rare")];
        for index in 0..6 {
            failures.push(failure(&format!("common {index}"), "common"));
        }
        failures.push(failure("last", "rare"));

        let lines = group_failures(&failures);

        assert_eq!(
            lines,
            [
                FailureLine::Track(&failures[0]),
                FailureLine::Grouped {
                    reason: "common",
                    track_count: 6,
                },
                FailureLine::Track(&failures[7]),
            ]
        );
    }

    #[test]
    fn lists_each_track_when_five_or_fewer_share_a_reason() {
        let failures: Vec<Failure> = (0..5)
            .map(|index| failure(&format!("track {index}"), "same"))
            .collect();

        let lines = group_failures(&failures);

        assert_eq!(lines.len(), 5);
        assert!(
            lines
                .iter()
                .all(|line| matches!(line, FailureLine::Track(_)))
        );
    }
}
