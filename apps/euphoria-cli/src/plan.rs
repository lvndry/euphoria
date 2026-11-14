// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `plan` command: how to mix one track into the next. It stretches B
//! to A's tempo, reads both tracks bar by bar, and ranks the windows where A
//! can go out and B come in.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::Args;
use euphoria_core::harmony::transition::{
    AutomaticCues, CandidateWindow, Clash, PlanOptions, PlanTrack, SectionSource, TransitionPlan,
    plan_transition,
};
use euphoria_core::plan_signals::{
    SignalsCache, TrackSignals, measure_energy, signals_with_energy,
};
use euphoria_core::{ContentHash, FileResult, TrackAnalysis};
use serde::Serialize;

use crate::batch::{self, AnalysisOptions};
use crate::style::Theme;
use crate::terminal::Printable;
use crate::with_causes;

/// Windows shown in the table.
const SHOWN_WINDOWS: usize = 3;
/// Windows in the JSON.
const JSON_WINDOWS: usize = 10;

#[derive(Debug, Args)]
pub struct PlanArguments {
    /// The track that is playing.
    #[arg(value_name = "A")]
    first: PathBuf,
    /// The track that plays next.
    #[arg(value_name = "B")]
    second: PathBuf,
    /// Print JSON with every lane and the best windows.
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

struct Loaded {
    path: PathBuf,
    analysis: TrackAnalysis,
    signals: TrackSignals,
}

#[derive(Serialize)]
struct PlanJson<'plan> {
    a: String,
    b: String,
    elapsed_milliseconds: f64,
    plan: &'plan TransitionPlan,
}

pub fn run(arguments: &PlanArguments) -> Result<ExitCode> {
    for file in [&arguments.first, &arguments.second] {
        if !file.is_file() {
            bail!("{} is not a file", Printable(file.display()));
        }
    }
    let files = vec![arguments.first.clone(), arguments.second.clone()];
    let display = if arguments.json {
        batch::Display::CountOnly
    } else {
        batch::Display::Stages
    };
    let batch = batch::analyze(&files, &arguments.analysis, display)?;
    let edits = batch::user_edits(
        &batch.results,
        &arguments.analysis,
        batch::CorrectionCheck::Quick,
    )?;
    let results = batch::corrected(batch.results, &edits);
    let mut analyses = Vec::new();
    for FileResult { path, outcome } in results {
        match outcome {
            Ok(analysis) => analyses.push((path, analysis)),
            Err(error) => {
                return Err(with_causes(error)
                    .context(format!("cannot analyze {}", Printable(path.display()))));
            }
        }
    }
    let started = Instant::now();
    let cache = SignalsCache::open_default().map_err(with_causes)?;
    let mut loaded = Vec::new();
    for (path, analysis) in analyses {
        let signals = signals_of(&cache, &path, &analysis)?;
        loaded.push(Loaded {
            path,
            analysis,
            signals,
        });
    }
    let sections: Vec<_> = loaded
        .iter()
        .map(|track| AutomaticCues.sections(&track.analysis))
        .collect();
    let track_of = |index: usize| PlanTrack {
        analysis: &loaded[index].analysis,
        sections: &sections[index],
        energy: &loaded[index].signals.energy,
        stems: loaded[index].signals.stems.as_ref(),
    };
    let options = PlanOptions {
        window_count: JSON_WINDOWS,
    };
    let plan = plan_transition(&track_of(0), &track_of(1), &options);
    let elapsed = started.elapsed();
    let plan = match plan {
        Ok(plan) => plan,
        Err(error) => {
            let theme = Theme::stderr();
            writeln!(std::io::stderr().lock(), "{} {error}", theme.error_label())?;
            return Ok(ExitCode::FAILURE);
        }
    };
    let mut standard_output = std::io::stdout().lock();
    if arguments.json {
        let document = PlanJson {
            a: loaded[0].path.display().to_string(),
            b: loaded[1].path.display().to_string(),
            elapsed_milliseconds: elapsed.as_secs_f64() * 1000.0,
            plan: &plan,
        };
        serde_json::to_writer_pretty(&mut standard_output, &document)?;
        writeln!(standard_output)?;
    } else {
        write_plan(&mut standard_output, &Theme::stdout(), &loaded, &plan)?;
    }
    standard_output.flush()?;
    Ok(ExitCode::SUCCESS)
}

fn signals_of(cache: &SignalsCache, path: &Path, analysis: &TrackAnalysis) -> Result<TrackSignals> {
    let hash = ContentHash::of_file(path)
        .with_context(|| format!("cannot read {}", Printable(path.display())))?;
    let mut signals = match cache.load(&hash) {
        Some(signals) => signals,
        None => {
            let energy = measure_energy(path, analysis.bpm).map_err(with_causes)?;
            let signals = signals_with_energy(energy);
            cache.store(&hash, &signals).map_err(with_causes)?;
            signals
        }
    };
    #[cfg(feature = "stems")]
    if signals.stems.is_none()
        && let Ok(stem_cache) = euphoria_core::stems::StemCache::open_default()
        && let Some(files) = stem_cache.cached(&hash)
        && let Ok(levels) = euphoria_core::stems::measure_stem_levels(&files)
    {
        signals.stems = Some(levels);
        cache.store(&hash, &signals).map_err(with_causes)?;
    }
    Ok(signals)
}

fn strip_text(theme: &Theme, window: &CandidateWindow) -> String {
    window
        .clash_strip
        .iter()
        .map(|clash| match clash {
            Clash::Vocals => theme.bad("█"),
            Clash::Bass => theme.caution("▄"),
            Clash::Clear => theme.good("·"),
            Clash::Unknown => theme.dim("?"),
        })
        .collect()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn write_plan(
    writer: &mut impl Write,
    theme: &Theme,
    tracks: &[Loaded],
    plan: &TransitionPlan,
) -> std::io::Result<()> {
    let alignment = &plan.alignment;
    writeln!(
        writer,
        "{} {}  {}  {}",
        theme.bold("Plan"),
        Printable(file_name(&tracks[0].path)),
        theme.dim("into"),
        Printable(file_name(&tracks[1].path)),
    )?;
    writeln!(
        writer,
        "{}  B {:.1} to {:.1} BPM, played at {:.3}x ({:+.1}%)",
        theme.dim("Tempo"),
        alignment.bpm_b,
        alignment.bpm_a,
        alignment.rate,
        alignment.tempo_change_percent
    )?;
    if let (Some(first), Some(second)) = (tracks[0].analysis.key, tracks[1].analysis.key) {
        let relation = first
            .relation_to(&second)
            .map_or("Key clash", |relation| relation.label());
        writeln!(
            writer,
            "{}    {} to {}  {}",
            theme.dim("Key"),
            theme.key(first, first.camelot()),
            theme.key(second, second.camelot()),
            relation
        )?;
    }
    for (name, lanes) in [("A", &plan.lanes_a), ("B", &plan.lanes_b)] {
        if !lanes.stems_known {
            writeln!(
                writer,
                "{} No cached stems for {name}: vocals, bass and drums are unknown, so clashes \
                 are not checked. Run euphoria stems on it first.",
                theme.warning_label(),
            )?;
        }
    }
    if plan.windows.is_empty() {
        writeln!(
            writer,
            "\nNo window of 16, 32 or 64 bars fits these two tracks."
        )?;
        return Ok(());
    }
    writeln!(
        writer,
        "\n{}  {} vocals  {} bass  {} clear  {} not known\n",
        theme.dim("Clash strip, one cell per bar:"),
        theme.bad("█"),
        theme.caution("▄"),
        theme.good("·"),
        theme.dim("?"),
    )?;
    for window in plan.windows.iter().take(SHOWN_WINDOWS) {
        let section = |label: Option<euphoria_core::harmony::transition::SectionLabel>| {
            label.map_or("-", |label| label.name())
        };
        writeln!(
            writer,
            "{} {}  score {:.0}, {} bars: A {} from bar {}, B {} from bar {}",
            theme.bold(format!("#{}", window.rank)),
            if window.clean {
                theme.good("clean")
            } else {
                String::new()
            },
            window.score,
            window.bars,
            section(window.a_section),
            window.a_start_bar + 1,
            section(window.b_section),
            window.b_start_bar + 1,
        )?;
        writeln!(writer, "   {}", strip_text(theme, window))?;
        writeln!(writer, "   {}", window.reason)?;
        for step in &window.steps {
            writeln!(
                writer,
                "   {} bar {:>2}  {}",
                theme.dim("-"),
                step.window_bar,
                step.text
            )?;
        }
        writeln!(writer)?;
    }
    if let Some(cues) = &plan.cues {
        let describe = |cues: &[euphoria_core::harmony::transition::SuggestedCue]| {
            cues.iter()
                .map(|cue| format!("{} {:.1}s", cue.name, cue.seconds))
                .collect::<Vec<_>>()
                .join(", ")
        };
        writeln!(
            writer,
            "{}  A: {}",
            theme.dim("Cues for #1"),
            describe(&cues.a)
        )?;
        writeln!(writer, "             B: {}", describe(&cues.b))?;
    }
    Ok(())
}
