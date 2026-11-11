// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measures the quality report of every file in a list and writes one line
//! per file, without its path, so a private library can be measured too.
//!
//! A list file has one file per line: a label, a tab, and the path of an
//! audio file relative to the list file. The label is what the line is
//! known by in the output, such as `lame-128` or `library`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use cpu_time::ThreadTime;
use euphoria_core::decode_mono;
use euphoria_core::quality::{QualityReport, decode_with_quality};
use rayon::prelude::*;

/// Usage of the quality mode.
pub const USAGE: &str = "usage: euphoria-bench quality LIST_FILE";

/// The parsed command line of the quality mode.
#[derive(Debug, PartialEq, Eq)]
pub struct Arguments {
    list_file: PathBuf,
}

/// Parses the arguments that follow `quality` on the command line.
pub fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut list_file = None;
    for argument in arguments {
        if argument.starts_with("--") {
            bail!("unknown option {argument:?}\n{USAGE}");
        } else if list_file.is_some() {
            bail!("more than one list file given\n{USAGE}");
        } else {
            list_file = Some(PathBuf::from(argument));
        }
    }
    Ok(Arguments {
        list_file: list_file.with_context(|| format!("no list file given\n{USAGE}"))?,
    })
}

/// Measures every file of the list and prints a tab separated table.
pub fn run(arguments: &Arguments) -> Result<()> {
    let text = std::fs::read_to_string(&arguments.list_file)
        .with_context(|| format!("cannot read {}", arguments.list_file.display()))?;
    let folder = arguments.list_file.parent().unwrap_or(Path::new("."));
    let entries = parse_list(&text, folder)?;
    let lines: Vec<String> = entries
        .par_iter()
        .map(|(label, path)| measure(label, path))
        .collect();
    print!("{HEADER}");
    for line in lines {
        print!("{line}");
    }
    Ok(())
}

const HEADER: &str = "label\tcodec\tlossless\tbitrate_kbps\tsample_rate\tbit_depth\tchannels\tcutoff_hz\tclipped_share\tloudness_lufs\ttrue_peak_dbtp\tverdict\tdecode_cpu_seconds\tdecode_and_quality_cpu_seconds\n";

fn parse_list(text: &str, folder: &Path) -> Result<Vec<(String, PathBuf)>> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|(index, line)| {
            let (label, path) = line.split_once('\t').with_context(|| {
                format!("line {}: expected a label, a tab and a path", index + 1)
            })?;
            Ok((label.to_string(), folder.join(path)))
        })
        .collect()
}

/// One line for one file. A file that fails gives a line with the label
/// and the kind of failure only.
fn measure(label: &str, path: &Path) -> String {
    let started = ThreadTime::now();
    let decoded = decode_mono(path);
    let decode_seconds = started.elapsed().as_secs_f64();
    drop(decoded);
    let started = ThreadTime::now();
    let measured = decode_with_quality(path);
    let quality_seconds = started.elapsed().as_secs_f64();
    match measured {
        Ok((_, report)) => format_line(label, &report, decode_seconds, quality_seconds),
        Err(_) => format!("{label}\tfailed\n"),
    }
}

fn format_line(
    label: &str,
    report: &QualityReport,
    decode_seconds: f64,
    quality_seconds: f64,
) -> String {
    let optional = |value: Option<String>| value.unwrap_or_default();
    let mut line = String::new();
    let _ = writeln!(
        line,
        "{label}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.6}\t{}\t{}\t{}\t{decode_seconds:.4}\t{quality_seconds:.4}",
        report.codec,
        report.lossless,
        optional(report.bitrate_kbps.map(|rate| rate.to_string())),
        report.sample_rate,
        optional(report.bit_depth.map(|depth| depth.to_string())),
        optional(report.channels.map(|count| count.to_string())),
        optional(report.cutoff_hz.map(|hz| format!("{hz:.0}"))),
        report.clipped_share,
        optional(
            report
                .integrated_loudness_lufs
                .map(|lufs| format!("{lufs:.2}"))
        ),
        optional(report.true_peak_dbtp.map(|peak| format!("{peak:.2}"))),
        report.verdict.label(),
    );
    line
}

#[cfg(test)]
mod tests {
    use euphoria_core::quality::QualityVerdict;

    use super::*;

    #[test]
    fn list_lines_are_a_label_and_a_path_relative_to_the_list() {
        let entries = parse_list(
            "# comment\nlame-128\tencoded/a.mp3\n\nlibrary\t/x/b.m4a\n",
            Path::new("/data"),
        )
        .unwrap();
        assert_eq!(
            entries,
            [
                ("lame-128".to_string(), PathBuf::from("/data/encoded/a.mp3")),
                ("library".to_string(), PathBuf::from("/x/b.m4a")),
            ]
        );
        assert!(parse_list("no tab here\n", Path::new("/data")).is_err());
    }

    #[test]
    fn a_line_holds_the_label_and_never_the_path() {
        let report = QualityReport {
            codec: "aac".to_string(),
            lossless: false,
            bitrate_kbps: Some(256),
            sample_rate: 44_100,
            bit_depth: None,
            channels: Some(2),
            cutoff_hz: None,
            clipped_share: 0.0,
            integrated_loudness_lufs: Some(-9.5),
            true_peak_dbtp: Some(0.2),
            verdict: QualityVerdict::Good,
        };
        let line = format_line("library", &report, 0.1, 0.12);
        assert_eq!(
            line,
            "library\taac\tfalse\t256\t44100\t\t2\t\t0.000000\t-9.50\t0.20\tGood\t0.1000\t0.1200\n"
        );
        assert_eq!(line.split('\t').count(), HEADER.split('\t').count());
        assert_eq!(
            measure("library", Path::new("/secret/name.m4a")),
            "library\tfailed\n"
        );
    }

    #[test]
    fn arguments_need_one_list_file() {
        assert!(parse_arguments(["list.tsv".to_string()]).is_ok());
        assert!(parse_arguments(Vec::<String>::new()).is_err());
        assert!(parse_arguments(["a".to_string(), "b".to_string()]).is_err());
        assert!(parse_arguments(["--fast".to_string()]).is_err());
    }
}
