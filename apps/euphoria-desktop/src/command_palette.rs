// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The command palette: one box that finds tracks, by name and by what a DJ
//! asks of them, and runs the app's commands.
//!
//! This module holds what the palette decides: how a query is read, which
//! tracks and commands answer it, and what it remembers. The window around
//! it is in `palette_flow` and `views::command_palette`.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;

use euphoria_core::Key;
use euphoria_core::harmony::Relation;
use gpui::component::input::InputState;
use gpui::{Action, Entity, ScrollHandle};

use crate::actions::{
    AddCueAtPlayhead, AddFolder, EditTags, ExportLibrary, FocusSearch, OpenCommandPalette,
    OpenFocusView, OpenSetBuilder, OpenSettings, PlanTransition, ResetColumnWidths, RevealTrack,
    SelectAllTracks, SeparateStems, ToggleDecks, ToggleFavorite, ToggleMetronome, TogglePlayback,
    ToggleSidebar, UndoCueEdit, UndoTagWrite, UseDeckTheme, UseGraphiteTheme, UsePaperTheme,
    UseSlateTheme, WriteTags,
};
use crate::library::Track;
use crate::metronome::parse_bpm;
use crate::track_filters::{MAXIMUM_ENERGY, MINIMUM_ENERGY, TrackFilters};

/// How many recent tracks the palette remembers.
pub(crate) const RECENT_TRACK_COUNT: usize = 8;

/// How many commands show while nothing is typed.
pub(crate) const COMMON_COMMAND_COUNT: usize = 6;

/// The most tracks one answer lists.
pub(crate) const TRACK_ROW_LIMIT: usize = 8;

/// The most commands one answer lists.
pub(crate) const COMMAND_ROW_LIMIT: usize = 8;

/// Seconds the palette takes to appear.
pub(crate) const OPEN_SECONDS: f32 = 0.15;

/// How far, in pixels, the panel rises while it appears.
pub(crate) const RISE_PIXELS: f32 = 8.0;

/// How far from a typed tempo a track may be and still match it, when the
/// tempo is typed as a whole number.
const WHOLE_TEMPO_TOLERANCE: f32 = 1.0;

/// The same, when the tempo is typed with decimals.
const DECIMAL_TEMPO_TOLERANCE: f32 = 0.5;

/// How many of a track's search fields name it: the title, the artist and
/// the file name. The folder and the genre come after.
const NAME_FIELDS: usize = 3;

/// How much less a match in the folder or the genre counts than one in a
/// name.
const SIDE_FIELD_PENALTY: i32 = 400;

/// A command the palette can run: the same actions the menus carry.
pub(crate) struct CommandEntry {
    /// A stable name, used to count how often the command is used.
    pub(crate) id: &'static str,
    /// What the row says.
    pub(crate) name: &'static str,
    /// Other words that find the command.
    pub(crate) keywords: &'static [&'static str],
    /// Makes the action to run.
    pub(crate) action: fn() -> Box<dyn Action>,
}

macro_rules! command {
    ($id:literal, $name:literal, [$($keyword:literal),* $(,)?], $action:expr) => {
        CommandEntry {
            id: $id,
            name: $name,
            keywords: &[$($keyword),*],
            action: || Box::new($action),
        }
    };
}

/// Every command the palette runs. A test keeps this list equal to the
/// actions in the menus.
pub(crate) const COMMANDS: &[CommandEntry] = &[
    command!(
        "write-tags",
        "Write tags",
        [
            "tag", "tags", "write", "save", "key", "bpm", "tempo", "id3", "files", "embed",
            "program"
        ],
        WriteTags
    ),
    command!(
        "edit-tags",
        "Edit tags",
        [
            "tag", "tags", "edit", "metadata", "title", "artist", "genre", "rename", "change"
        ],
        EditTags
    ),
    command!(
        "undo-tag-write",
        "Undo last tag write",
        [
            "undo", "tag", "tags", "revert", "restore", "rollback", "write", "back"
        ],
        UndoTagWrite
    ),
    command!(
        "add-folder",
        "Add folder",
        [
            "import",
            "open",
            "scan",
            "music",
            "library",
            "directory",
            "add"
        ],
        AddFolder
    ),
    command!(
        "export-library",
        "Export library",
        [
            "rekordbox",
            "serato",
            "traktor",
            "engine",
            "virtualdj",
            "dj",
            "program",
            "save",
            "send"
        ],
        ExportLibrary
    ),
    command!(
        "toggle-decks",
        "Open decks",
        [
            "deck", "decks", "mix", "mixer", "two", "close", "hide", "show"
        ],
        ToggleDecks
    ),
    command!(
        "open-set-builder",
        "Open set builder",
        [
            "set", "builder", "playlist", "build", "order", "sequence", "plan", "mix"
        ],
        OpenSetBuilder
    ),
    command!(
        "plan-transition",
        "Plan a transition from the playing track",
        [
            "transition",
            "mix",
            "blend",
            "next",
            "plan",
            "playing",
            "match",
            "cue"
        ],
        PlanTransition
    ),
    command!(
        "toggle-metronome",
        "Toggle metronome",
        [
            "metronome",
            "click",
            "tempo",
            "bpm",
            "beat",
            "tap",
            "start",
            "stop"
        ],
        ToggleMetronome
    ),
    command!(
        "separate-stems",
        "Separate stems",
        [
            "stems", "stem", "vocals", "drums", "bass", "acapella", "split", "demucs", "separate"
        ],
        SeparateStems
    ),
    command!(
        "toggle-playback",
        "Play or pause",
        ["play", "pause", "stop", "start", "resume", "space"],
        TogglePlayback
    ),
    command!(
        "add-cue",
        "Add cue at playhead",
        ["cue", "cues", "marker", "hot", "add", "point", "playhead"],
        AddCueAtPlayhead
    ),
    command!(
        "undo-cue-edit",
        "Undo cue edit",
        ["cue", "cues", "undo", "revert", "back"],
        UndoCueEdit
    ),
    command!(
        "settings",
        "Settings",
        [
            "preferences",
            "options",
            "config",
            "configure",
            "notation",
            "setup"
        ],
        OpenSettings
    ),
    command!(
        "command-palette",
        "Command palette",
        ["find", "search", "commands", "anything", "go", "quick"],
        OpenCommandPalette
    ),
    command!(
        "toggle-sidebar",
        "Toggle sidebar",
        ["sidebar", "folders", "hide", "show", "fold", "panel"],
        ToggleSidebar
    ),
    command!(
        "focus-view",
        "Focus view",
        [
            "focus", "big", "full", "waveform", "zoom", "track", "expand", "detail"
        ],
        OpenFocusView
    ),
    command!(
        "search",
        "Search",
        ["find", "filter", "field", "look"],
        FocusSearch
    ),
    command!(
        "select-all",
        "Select all tracks",
        ["select", "all", "everything", "every"],
        SelectAllTracks
    ),
    command!(
        "toggle-favorite",
        "Star or unstar selection",
        ["favorite", "favourite", "star", "like", "love", "mark"],
        ToggleFavorite
    ),
    command!(
        "reveal-track",
        "Reveal in file manager",
        [
            "finder", "explorer", "show", "file", "folder", "locate", "reveal"
        ],
        RevealTrack
    ),
    command!(
        "reset-column-widths",
        "Reset column widths",
        [
            "columns", "column", "table", "width", "widths", "layout", "reset"
        ],
        ResetColumnWidths
    ),
    command!(
        "theme-slate",
        "Theme: Slate",
        ["theme", "dark", "colour", "color", "appearance", "slate"],
        UseSlateTheme
    ),
    command!(
        "theme-paper",
        "Theme: Paper",
        [
            "theme",
            "light",
            "colour",
            "color",
            "appearance",
            "paper",
            "white"
        ],
        UsePaperTheme
    ),
    command!(
        "theme-deck",
        "Theme: Deck",
        [
            "theme",
            "dark",
            "colour",
            "color",
            "appearance",
            "deck",
            "layout"
        ],
        UseDeckTheme
    ),
    command!(
        "theme-graphite",
        "Theme: Graphite",
        [
            "theme",
            "dark",
            "colour",
            "color",
            "appearance",
            "graphite",
            "warm"
        ],
        UseGraphiteTheme
    ),
];

/// The commands that show first while nothing has been used yet.
const STARTING_COMMANDS: [&str; COMMON_COMMAND_COUNT] = [
    "write-tags",
    "edit-tags",
    "add-folder",
    "toggle-decks",
    "open-set-builder",
    "settings",
];

/// Whether `name` is the id of a command.
#[cfg(test)]
pub(crate) fn command_named(name: &str) -> Option<usize> {
    COMMANDS.iter().position(|command| command.id == name)
}

/// What a typed query asks for.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ParsedQuery {
    /// A key the tracks must be in, or compatible with.
    pub(crate) key: Option<Key>,
    /// The tempo and energy ranges the tracks must be in.
    pub(crate) filters: TrackFilters,
    /// The middle of a typed tempo, to put the nearest tempo first.
    pub(crate) tempo_centre: Option<f32>,
    /// The other words, in lower case, which find names.
    pub(crate) words: Vec<String>,
}

impl ParsedQuery {
    /// Reads `text`: a Camelot or musical key ("8A", "A minor", "Am"), a
    /// tempo or a range of tempos ("124", "120-126", "124,5"), an energy
    /// ("e6", "e6+", "energy 6-8") and words, in any order.
    pub(crate) fn parse(text: &str) -> Self {
        let tokens: Vec<String> = text
            .replace(['\u{2013}', '\u{2014}'], "-")
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let mut query = Self::default();
        let mut index = 0;
        while index < tokens.len() {
            let token = tokens[index].as_str();
            let next = tokens.get(index + 1).map(String::as_str);
            if query.filters.energy_min.is_none()
                && token == "energy"
                && let Some((lowest, highest)) = next.and_then(energy_range)
            {
                query.set_energy(lowest, highest);
                index += 2;
                continue;
            }
            if query.filters.energy_min.is_none()
                && let Some((lowest, highest)) = token
                    .strip_prefix('e')
                    .filter(|rest| rest.starts_with(|first: char| first.is_ascii_digit()))
                    .and_then(energy_range)
            {
                query.set_energy(lowest, highest);
                index += 1;
                continue;
            }
            if query.key.is_none() {
                if let Some(key) = key_from_token(token) {
                    query.key = Some(key);
                    index += 1;
                    continue;
                }
                if let Some(key) = next.and_then(|mode| key_from_two_tokens(token, mode)) {
                    query.key = Some(key);
                    index += 2;
                    continue;
                }
            }
            if query.filters.bpm_min.is_none()
                && let Some((slowest, fastest, centre)) = tempo_range(token)
            {
                query.filters.bpm_min = Some(slowest);
                query.filters.bpm_max = Some(fastest);
                query.tempo_centre = Some(centre);
                index += 1;
                continue;
            }
            query.words.push(token.to_string());
            index += 1;
        }
        query
    }

    fn set_energy(&mut self, lowest: u8, highest: u8) {
        self.filters.energy_min = Some(lowest);
        self.filters.energy_max = Some(highest);
    }

    /// True when the query narrows tracks by key, tempo or energy.
    pub(crate) fn has_dj_terms(&self) -> bool {
        self.key.is_some() || self.filters.is_active()
    }

    /// True when nothing is typed.
    pub(crate) fn is_empty(&self) -> bool {
        !self.has_dj_terms() && self.words.is_empty()
    }
}

fn energy_range(text: &str) -> Option<(u8, u8)> {
    let parse = |digits: &str| -> Option<u8> {
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let value: u8 = digits.parse().ok()?;
        (MINIMUM_ENERGY..=MAXIMUM_ENERGY)
            .contains(&value)
            .then_some(value)
    };
    if let Some(from) = text.strip_suffix('+') {
        return parse(from).map(|lowest| (lowest, MAXIMUM_ENERGY));
    }
    match text.split_once('-') {
        Some((from, to)) => {
            let (lowest, highest) = (parse(from)?, parse(to)?);
            Some((lowest.min(highest), lowest.max(highest)))
        }
        None => parse(text).map(|value| (value, value)),
    }
}

fn tempo_range(token: &str) -> Option<(f32, f32, f32)> {
    let typed = token.strip_suffix("bpm").unwrap_or(token).replace(',', ".");
    if typed.is_empty()
        || !typed
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.' || byte == b'-')
    {
        return None;
    }
    if let Some((from, to)) = typed.split_once('-') {
        if [from, to].iter().any(|end| {
            let digits = end.split('.').next().unwrap_or_default().len();
            !(2..=3).contains(&digits) || end.starts_with('0')
        }) {
            return None;
        }
        let (first, second) = (parse_bpm(from)?, parse_bpm(to)?);
        let (slowest, fastest) = (first.min(second), first.max(second));
        return Some((slowest, fastest, (slowest + fastest) / 2.0));
    }
    let whole_digits = typed.split('.').next().unwrap_or_default().len();
    if !(2..=3).contains(&whole_digits) || typed.starts_with('0') {
        return None;
    }
    let tempo = parse_bpm(&typed)?;
    let tolerance = if typed.contains('.') {
        DECIMAL_TEMPO_TOLERANCE
    } else {
        WHOLE_TEMPO_TOLERANCE
    };
    Some((tempo - tolerance, tempo + tolerance, tempo))
}

const MODE_WORDS: [&str; 4] = ["min", "minor", "maj", "major"];

fn is_note_name(token: &str) -> bool {
    let mut characters = token.chars();
    matches!(characters.next(), Some('a'..='g')) && matches!(characters.as_str(), "" | "#" | "b")
}

fn key_from_token(token: &str) -> Option<Key> {
    if token.starts_with(|first: char| first.is_ascii_digit()) {
        return (token.len() <= 3).then(|| Key::parse_file_key(token))?;
    }
    let has_mode = token.ends_with('m')
        || MODE_WORDS
            .iter()
            .any(|word| token.len() > word.len() && token.ends_with(word));
    (token.len() >= 2 && token.starts_with(|first: char| ('a'..='g').contains(&first)) && has_mode)
        .then(|| Key::parse_file_key(token))?
}

fn key_from_two_tokens(note: &str, mode: &str) -> Option<Key> {
    (is_note_name(note) && MODE_WORDS.contains(&mode))
        .then(|| Key::parse_file_key(&format!("{note} {mode}")))?
}

/// The pill that says how a track's key relates to the key typed, for the
/// moves the palette lists beside an exact match.
pub(crate) fn relation_pill(relation: Relation) -> Option<&'static str> {
    match relation {
        Relation::Adjacent => Some("one step"),
        Relation::Relative => Some("relative"),
        _ => None,
    }
}

fn compatibility_bonus(relation: Relation) -> Option<i32> {
    match relation {
        Relation::Same => Some(3000),
        Relation::Adjacent => Some(2000),
        Relation::Relative => Some(1000),
        _ => None,
    }
}

/// How well `needle` matches `haystack`, higher being better, or `None`.
/// A run of the needle inside the text scores most, more at the start of
/// the text or of a word. Letters in order with gaps between score less,
/// and only for needles of three letters or more.
pub(crate) fn fuzzy_score(haystack: &str, needle: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    if let Some(position) = haystack.find(needle) {
        let starts_word = position == 0
            || haystack[..position]
                .chars()
                .next_back()
                .is_some_and(|before| !before.is_alphanumeric());
        let position_penalty = i32::try_from(position.min(100)).unwrap_or(100);
        return Some(1000 - position_penalty + if starts_word { 200 } else { 0 });
    }
    if needle.chars().count() < 3 {
        return None;
    }
    let mut wanted = needle.chars().peekable();
    let mut gaps = 0i32;
    let mut previous_matched = true;
    for letter in haystack.chars() {
        match wanted.peek() {
            Some(next) if *next == letter => {
                wanted.next();
                previous_matched = true;
            }
            Some(_) => {
                if !previous_matched {
                    gaps += 1;
                }
                previous_matched = false;
            }
            None => break,
        }
    }
    wanted.peek().is_none().then_some((500 - gaps * 8).max(100))
}

/// A track that answers the query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TrackHit {
    /// Where the track is in the library.
    pub(crate) index: usize,
    /// How its key relates to the key typed, when one was typed.
    pub(crate) relation: Option<Relation>,
    score: i32,
}

/// The tracks that answer a query, best first.
#[derive(Debug, Default)]
pub(crate) struct TrackAnswer {
    /// The best tracks, at most [`TRACK_ROW_LIMIT`].
    pub(crate) hits: Vec<TrackHit>,
    /// How many tracks answer in all.
    pub(crate) total: usize,
}

/// Finds the tracks that answer `query` among `tracks`. Every word must be
/// in the title, the artist or the file name. A typed key keeps the tracks
/// in that key and in the keys one step from it or relative to it, the
/// exact key first.
pub(crate) fn find_tracks(tracks: &[Track], query: &ParsedQuery) -> TrackAnswer {
    if query.is_empty() {
        return TrackAnswer::default();
    }
    let mut hits: Vec<TrackHit> = Vec::new();
    for (index, track) in tracks.iter().enumerate() {
        let mut score = 0;
        let mut relation = None;
        if let Some(wanted) = query.key {
            let Some(track_key) = track.key() else {
                continue;
            };
            let Some(found) = wanted.relation_to(&track_key) else {
                continue;
            };
            let Some(bonus) = compatibility_bonus(found) else {
                continue;
            };
            score += bonus;
            relation = Some(found);
        }
        if !query.filters.matches(track) {
            continue;
        }
        if let (Some(centre), Some(bpm)) = (query.tempo_centre, track.bpm()) {
            score -= ((bpm - centre).abs() * 10.0) as i32;
        }
        let fields = track.search_fields();
        let mut words_fit = true;
        for word in &query.words {
            let best = fields
                .iter()
                .enumerate()
                .filter(|(_, field)| !field.is_empty())
                .filter_map(|(position, field)| {
                    let penalty = if position < NAME_FIELDS {
                        0
                    } else {
                        SIDE_FIELD_PENALTY
                    };
                    fuzzy_score(field, word).map(|score| score - penalty)
                })
                .max();
            match best {
                Some(word_score) => score += word_score,
                None => {
                    words_fit = false;
                    break;
                }
            }
        }
        if words_fit {
            hits.push(TrackHit {
                index,
                relation,
                score,
            });
        }
    }
    let total = hits.len();
    if hits.len() > TRACK_ROW_LIMIT {
        hits.select_nth_unstable_by(TRACK_ROW_LIMIT, better_hit_first);
        hits.truncate(TRACK_ROW_LIMIT);
    }
    hits.sort_unstable_by(better_hit_first);
    TrackAnswer { hits, total }
}

fn better_hit_first(first: &TrackHit, second: &TrackHit) -> std::cmp::Ordering {
    second
        .score
        .cmp(&first.score)
        .then(first.index.cmp(&second.index))
}

/// The commands that answer `words`, best first, as positions in
/// [`COMMANDS`]. A word may match the name or one of the keywords.
pub(crate) fn find_commands(words: &[String]) -> Vec<usize> {
    if words.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(i32, usize)> = COMMANDS
        .iter()
        .enumerate()
        .filter_map(|(position, command)| {
            let name = command.name.to_lowercase();
            let mut total = 0;
            for word in words {
                let name_score = fuzzy_score(&name, word);
                let keyword_score = command
                    .keywords
                    .iter()
                    .filter_map(|keyword| keyword_score(keyword, word))
                    .max();
                total += name_score.max(keyword_score)?;
            }
            Some((total, position))
        })
        .collect();
    scored.sort_unstable_by(|first, second| second.0.cmp(&first.0).then(first.1.cmp(&second.1)));
    scored
        .into_iter()
        .take(COMMAND_ROW_LIMIT)
        .map(|(_, position)| position)
        .collect()
}

fn keyword_score(keyword: &str, word: &str) -> Option<i32> {
    if keyword == word {
        Some(900)
    } else if keyword.starts_with(word) {
        Some(700)
    } else {
        None
    }
}

/// The last tracks selected or played, newest first, kept in memory only.
#[derive(Debug, Default)]
pub(crate) struct RecentTracks {
    paths: VecDeque<PathBuf>,
}

impl RecentTracks {
    /// Puts `path` first, dropping the oldest beyond [`RECENT_TRACK_COUNT`].
    pub(crate) fn record(&mut self, path: &Path) {
        self.paths.retain(|known| known != path);
        self.paths.push_front(path.to_path_buf());
        self.paths.truncate(RECENT_TRACK_COUNT);
    }

    /// Takes `path` off the list.
    pub(crate) fn forget(&mut self, path: &Path) {
        self.paths.retain(|known| known != path);
    }

    /// The paths, newest first.
    pub(crate) fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.paths.iter()
    }
}

/// How often each command has been run in this session.
#[derive(Debug, Default)]
pub(crate) struct CommandUsage {
    counts: HashMap<&'static str, u32>,
}

impl CommandUsage {
    /// Counts one run of the command at `position` in [`COMMANDS`].
    pub(crate) fn record(&mut self, position: usize) {
        if let Some(command) = COMMANDS.get(position) {
            *self.counts.entry(command.id).or_default() += 1;
        }
    }

    /// The most used commands, then the starting ones to fill the list.
    pub(crate) fn most_used(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..COMMANDS.len()).collect();
        order.sort_by_key(|position| {
            let id = COMMANDS[*position].id;
            let uses = self.counts.get(id).copied().unwrap_or(0);
            let starting_rank = STARTING_COMMANDS
                .iter()
                .position(|starting| *starting == id)
                .unwrap_or(usize::MAX);
            (std::cmp::Reverse(uses), starting_rank, *position)
        });
        order.truncate(COMMON_COMMAND_COUNT);
        order
    }
}

/// One row of the palette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PaletteRow {
    /// A track of the library.
    Track {
        /// Where it is in the library.
        index: usize,
        /// How its key relates to the key typed.
        relation: Option<Relation>,
    },
    /// A command, as a position in [`COMMANDS`].
    Command(usize),
}

/// The rows the palette shows for `text`.
pub(crate) struct PaletteAnswer {
    /// The rows: tracks first, then commands.
    pub(crate) rows: Vec<PaletteRow>,
    /// How many tracks answer in all, which may be more than are listed.
    pub(crate) track_total: usize,
    /// How many of the rows are tracks.
    pub(crate) track_rows: usize,
    /// True while nothing is typed: the rows are the recent tracks and the
    /// most used commands.
    pub(crate) is_resting: bool,
}

/// Works out the rows for `text`. With nothing typed they are the recent
/// tracks and the most used commands.
pub(crate) fn answer(
    text: &str,
    tracks: &[Track],
    index_of: impl Fn(&Path) -> Option<usize>,
    recent: &RecentTracks,
    usage: &CommandUsage,
) -> PaletteAnswer {
    let query = ParsedQuery::parse(text);
    if query.is_empty() {
        let mut rows: Vec<PaletteRow> = recent
            .paths()
            .filter_map(|path| index_of(path))
            .map(|index| PaletteRow::Track {
                index,
                relation: None,
            })
            .collect();
        let track_rows = rows.len();
        rows.extend(usage.most_used().into_iter().map(PaletteRow::Command));
        return PaletteAnswer {
            rows,
            track_total: track_rows,
            track_rows,
            is_resting: true,
        };
    }
    let found = find_tracks(tracks, &query);
    let mut rows: Vec<PaletteRow> = found
        .hits
        .iter()
        .map(|hit| PaletteRow::Track {
            index: hit.index,
            relation: hit.relation,
        })
        .collect();
    let track_rows = rows.len();
    rows.extend(
        find_commands(&query.words)
            .into_iter()
            .map(PaletteRow::Command),
    );
    PaletteAnswer {
        rows,
        track_total: found.total,
        track_rows,
        is_resting: false,
    }
}

/// How far the panel has come in, from 0 to 1, `now` after it began at
/// `opened_at`. With motion reduced it is there at once.
pub(crate) fn open_progress(opened_at: Instant, now: Instant, reduces_motion: bool) -> f32 {
    if reduces_motion {
        return 1.0;
    }
    let elapsed = now.saturating_duration_since(opened_at).as_secs_f32();
    let progress = (elapsed / OPEN_SECONDS).clamp(0.0, 1.0);
    crate::motion::ease_out(progress)
}

pub(crate) use crate::motion::system_reduces_motion as motion_is_reduced;

/// The palette on screen.
pub(crate) struct OpenPalette {
    /// What the palette lists.
    pub(crate) answer: PaletteAnswer,
    /// The row Enter runs.
    pub(crate) selected: usize,
    /// When the panel began to appear.
    pub(crate) opened_at: Instant,
    /// Whether the panel appears at once.
    pub(crate) reduces_motion: bool,
}

impl OpenPalette {
    /// Moves the selection `steps` rows, wrapping round the ends.
    pub(crate) fn move_selection(&mut self, steps: isize) {
        let count = self.answer.rows.len();
        if count == 0 {
            return;
        }
        let count = count as isize;
        self.selected = (self.selected as isize + steps).rem_euclid(count) as usize;
    }

    /// The row Enter runs.
    pub(crate) fn selected_row(&self) -> Option<PaletteRow> {
        self.answer.rows.get(self.selected).copied()
    }
}

/// The palette: its field and what it remembers.
pub(crate) struct CommandPalette {
    /// The field the query is typed in.
    pub(crate) input: Entity<InputState>,
    /// The palette on screen, or `None` while it is closed.
    pub(crate) open: Option<OpenPalette>,
    /// The tracks last selected or played.
    pub(crate) recent: RecentTracks,
    /// How often commands have been run.
    pub(crate) usage: CommandUsage,
    /// Scrolls the list of rows.
    pub(crate) scroll: ScrollHandle,
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use euphoria_core::harmony::Relation;

    use super::*;
    use crate::demo;
    use crate::library::Library;

    fn key(text: &str) -> Key {
        text.parse().unwrap()
    }

    fn library_of(count: usize) -> Library {
        let mut library = Library::default();
        library.add_folder(PathBuf::from(demo::DEMO_FOLDER));
        let tracks = demo::tracks(count);
        let paths = tracks.iter().map(|track| track.path.clone()).collect();
        library.add_files(paths);
        for track in tracks {
            library.record_outcome(
                &track.path,
                track.outcome.map_err(crate::library::TrackFailure::Failed),
            );
        }
        library
    }

    #[test]
    fn a_camelot_code_in_either_case_is_a_key() {
        for typed in ["8A", "8a", "08a"] {
            let query = ParsedQuery::parse(typed);
            assert_eq!(query.key, Some(key("8A")), "{typed}");
            assert!(query.words.is_empty());
        }
        assert_eq!(ParsedQuery::parse("12b").key, Some(key("12B")));
    }

    #[test]
    fn a_musical_key_is_a_key() {
        for typed in ["A minor", "a minor", "Am", "am", "Amin"] {
            assert_eq!(ParsedQuery::parse(typed).key, Some(key("8A")), "{typed}");
        }
        assert_eq!(ParsedQuery::parse("C major").key, Some(key("8B")));
        assert_eq!(ParsedQuery::parse("F#m").key, Some(key("11A")));
        assert_eq!(ParsedQuery::parse("Bbm").key, Some(key("3A")));
    }

    #[test]
    fn plain_words_that_look_a_little_like_keys_stay_words() {
        for typed in ["a", "dream", "bass", "drive", "egg"] {
            let query = ParsedQuery::parse(typed);
            assert_eq!(query.key, None, "{typed}");
            assert_eq!(query.words, vec![typed.to_string()]);
        }
    }

    #[test]
    fn a_number_is_a_tempo_within_a_beat() {
        let query = ParsedQuery::parse("124");
        assert_eq!(query.filters.bpm_min, Some(123.0));
        assert_eq!(query.filters.bpm_max, Some(125.0));
        assert!(query.words.is_empty() && query.key.is_none());
    }

    #[test]
    fn a_range_of_tempos_reads_hyphens_dashes_and_comma_decimals() {
        for typed in ["120-126", "120\u{2013}126", "120\u{2014}126", "126-120"] {
            let query = ParsedQuery::parse(typed);
            assert_eq!(query.filters.bpm_min, Some(120.0), "{typed}");
            assert_eq!(query.filters.bpm_max, Some(126.0), "{typed}");
        }
        let decimals = ParsedQuery::parse("120,5-125,5");
        assert_eq!(decimals.filters.bpm_min, Some(120.5));
        assert_eq!(decimals.filters.bpm_max, Some(125.5));
        let single = ParsedQuery::parse("124,5");
        assert_eq!(single.filters.bpm_min, Some(124.0));
        assert_eq!(single.filters.bpm_max, Some(125.0));
        assert_eq!(ParsedQuery::parse("124bpm").filters.bpm_min, Some(123.0));
    }

    #[test]
    fn numbers_outside_the_tempo_range_are_words() {
        for typed in ["7", "00042", "1999"] {
            let query = ParsedQuery::parse(typed);
            assert!(!query.filters.is_active(), "{typed}");
            assert_eq!(query.words, vec![typed.to_string()]);
        }
    }

    #[test]
    fn energy_reads_as_one_rating_a_floor_or_a_range() {
        let exact = ParsedQuery::parse("e6");
        assert_eq!(
            (exact.filters.energy_min, exact.filters.energy_max),
            (Some(6), Some(6))
        );
        let floor = ParsedQuery::parse("energy 6+");
        assert_eq!(
            (floor.filters.energy_min, floor.filters.energy_max),
            (Some(6), Some(10))
        );
        let range = ParsedQuery::parse("E7-9");
        assert_eq!(
            (range.filters.energy_min, range.filters.energy_max),
            (Some(7), Some(9))
        );
        assert!(floor.words.is_empty());
        assert!(ParsedQuery::parse("e11").filters.energy_min.is_none());
        assert_eq!(ParsedQuery::parse("energy").words, vec!["energy"]);
    }

    #[test]
    fn key_tempo_energy_and_words_combine_in_any_order() {
        for typed in ["8A 124 deep", "deep 124 8a", "124 deep 8A e6+"] {
            let query = ParsedQuery::parse(typed);
            assert_eq!(query.key, Some(key("8A")), "{typed}");
            assert_eq!(query.filters.bpm_min, Some(123.0), "{typed}");
            assert_eq!(query.words, vec!["deep"], "{typed}");
        }
        let all = ParsedQuery::parse("A minor 120-126 energy 6+ deep house");
        assert_eq!(all.key, Some(key("8A")));
        assert_eq!(all.filters.bpm_min, Some(120.0));
        assert_eq!(all.filters.energy_min, Some(6));
        assert_eq!(all.words, vec!["deep", "house"]);
    }

    #[test]
    fn a_key_filter_lists_the_key_and_its_one_step_and_relative_neighbours() {
        let library = library_of(400);
        let query = ParsedQuery::parse("8A");
        let found = find_tracks(library.tracks(), &query);
        assert!(found.total > found.hits.len(), "more keys than rows");
        let mut relations: Vec<Relation> = Vec::new();
        for hit in &found.hits {
            let relation = hit.relation.unwrap();
            let track_key = library.tracks()[hit.index].key().unwrap();
            assert_eq!(key("8A").relation_to(&track_key), Some(relation));
            relations.push(relation);
        }
        assert_eq!(relations[0], Relation::Same, "the exact key comes first");
        assert!(relations.iter().all(|relation| matches!(
            relation,
            Relation::Same | Relation::Adjacent | Relation::Relative
        )));
        assert!(relations.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn every_compatible_key_has_a_pill_saying_how() {
        let library = library_of(600);
        let query = ParsedQuery::parse("8A");
        let found = find_tracks(library.tracks(), &query);
        let everyone: Vec<_> = (0..library.tracks().len())
            .filter_map(|index| {
                let track_key = library.tracks()[index].key()?;
                key("8A")
                    .relation_to(&track_key)
                    .filter(|relation| {
                        matches!(
                            relation,
                            Relation::Same | Relation::Adjacent | Relation::Relative
                        )
                    })
                    .map(|relation| (index, relation))
            })
            .collect();
        assert_eq!(found.total, everyone.len());
        assert_eq!(relation_pill(Relation::Adjacent), Some("one step"));
        assert_eq!(relation_pill(Relation::Relative), Some("relative"));
        assert_eq!(relation_pill(Relation::Same), None);
        assert_eq!(relation_pill(Relation::EnergyBoost), None);
        let seven_a = find_tracks(library.tracks(), &ParsedQuery::parse("7A 8B"));
        assert!(seven_a.hits.is_empty(), "the second key is a word");
    }

    #[test]
    fn a_tempo_range_keeps_only_tracks_in_it_using_the_corrected_tempo() {
        let mut library = library_of(300);
        let corrected_path = library.tracks()[0].path.clone();
        library.set_correction(
            &corrected_path,
            euphoria_core::Correction {
                bpm: Some(150.0),
                ..euphoria_core::Correction::default()
            },
        );
        let query = ParsedQuery::parse("149-151");
        let found = find_tracks(library.tracks(), &query);
        assert!(
            found
                .hits
                .iter()
                .any(|hit| library.tracks()[hit.index].path == corrected_path)
        );
        for hit in &found.hits {
            let bpm = library.tracks()[hit.index].bpm().unwrap();
            assert!((149.0..=151.0).contains(&bpm));
        }
        let demo_range = find_tracks(library.tracks(), &ParsedQuery::parse("120-126"));
        assert!(demo_range.total > 0);
        for hit in &demo_range.hits {
            let bpm = library.tracks()[hit.index].bpm().unwrap();
            assert!((120.0..=126.0).contains(&bpm));
        }
    }

    #[test]
    fn energy_keeps_only_tracks_with_that_rating() {
        let library = library_of(300);
        let found = find_tracks(library.tracks(), &ParsedQuery::parse("e6"));
        assert!(found.total > 0);
        for hit in &found.hits {
            assert_eq!(library.tracks()[hit.index].energy(), Some(6));
        }
        let floor = find_tracks(library.tracks(), &ParsedQuery::parse("energy 9+"));
        for hit in &floor.hits {
            assert!(library.tracks()[hit.index].energy().unwrap() >= 9);
        }
    }

    #[test]
    fn a_combined_query_needs_every_part() {
        let library = library_of(2000);
        let query = ParsedQuery::parse("8A 120-126 deep");
        let found = find_tracks(library.tracks(), &query);
        assert!(found.total > 0, "the demo library has such tracks");
        for hit in &found.hits {
            let track = &library.tracks()[hit.index];
            assert!(track.folder.to_lowercase().contains("deep"));
            assert!((120.0..=126.0).contains(&track.bpm().unwrap()));
            assert!(hit.relation.is_some());
        }
    }

    #[test]
    fn words_match_the_title_the_artist_and_the_file_name_with_typos_in_order() {
        assert!(fuzzy_score("glass signal 00001.flac", "glass").unwrap() > 1000);
        assert!(
            fuzzy_score("glass signal", "signal").unwrap()
                > fuzzy_score("a glass signal", "ass").unwrap()
        );
        assert!(fuzzy_score("glass signal", "glsig").is_some());
        assert_eq!(fuzzy_score("glass signal", "gz"), None);
        assert_eq!(fuzzy_score("glass signal", "lgs"), None);
        let library = library_of(100);
        let found = find_tracks(library.tracks(), &ParsedQuery::parse("cobalt"));
        assert!(found.total > 0);
        for hit in &found.hits {
            assert!(library.tracks()[hit.index].title().contains("Cobalt"));
        }
    }

    #[test]
    fn a_word_finds_commands_by_name_and_by_synonym() {
        let names = |typed: &str| -> Vec<&'static str> {
            find_commands(&ParsedQuery::parse(typed).words)
                .into_iter()
                .map(|position| COMMANDS[position].id)
                .collect()
        };
        assert_eq!(names("write")[0], "write-tags");
        assert!(names("tag").contains(&"write-tags"));
        assert!(names("tag").contains(&"edit-tags"));
        assert!(names("tag").contains(&"undo-tag-write"));
        assert!(names("key").contains(&"write-tags"));
        assert_eq!(names("undo tag")[0], "undo-tag-write");
        assert_eq!(names("stems")[0], "separate-stems");
        assert!(names("vocals").contains(&"separate-stems"));
        assert!(names("rekordbox").contains(&"export-library"));
        assert!(names("preferences").contains(&"settings"));
        assert!(names("dark").contains(&"theme-slate"));
        assert!(names("light").contains(&"theme-paper"));
        assert!(names("transition").contains(&"plan-transition"));
        assert!(names("zzzzqq").is_empty());
        assert!(names("").is_empty());
    }

    #[test]
    fn every_command_makes_an_action_and_ids_are_unique() {
        let mut ids: Vec<&str> = COMMANDS.iter().map(|command| command.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), COMMANDS.len());
        for command in COMMANDS {
            let action = (command.action)();
            assert!(!action.name().is_empty(), "{}", command.id);
            assert!(!command.keywords.is_empty(), "{}", command.id);
        }
        for starting in STARTING_COMMANDS {
            assert!(command_named(starting).is_some(), "{starting}");
        }
    }

    #[test]
    fn the_recent_list_keeps_the_last_eight_newest_first_without_repeats() {
        let mut recent = RecentTracks::default();
        for number in 0..12 {
            recent.record(Path::new(&format!("/music/track {number}.flac")));
        }
        recent.record(Path::new("/music/track 9.flac"));
        let paths: Vec<String> = recent
            .paths()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(paths.len(), RECENT_TRACK_COUNT);
        assert_eq!(paths[0], "/music/track 9.flac");
        assert_eq!(paths[1], "/music/track 11.flac");
        assert_eq!(
            paths
                .iter()
                .filter(|path| path.ends_with(" 9.flac"))
                .count(),
            1
        );
        assert!(!paths.iter().any(|path| path.ends_with(" 3.flac")));
        recent.forget(Path::new("/music/track 9.flac"));
        assert_eq!(recent.paths().count(), RECENT_TRACK_COUNT - 1);
    }

    #[test]
    fn nothing_typed_lists_recent_tracks_then_the_six_most_used_commands() {
        let library = library_of(50);
        let mut recent = RecentTracks::default();
        recent.record(&library.tracks()[3].path);
        recent.record(&library.tracks()[7].path);
        recent.record(Path::new("/gone/away.flac"));
        let mut usage = CommandUsage::default();
        let reveal = COMMANDS
            .iter()
            .position(|command| command.id == "reveal-track")
            .unwrap();
        usage.record(reveal);
        usage.record(reveal);
        let shown = answer(
            "  ",
            library.tracks(),
            |path| library.index_of(path),
            &recent,
            &usage,
        );
        assert!(shown.is_resting);
        assert_eq!(shown.track_rows, 2);
        assert_eq!(
            shown.rows[0],
            PaletteRow::Track {
                index: 7,
                relation: None
            }
        );
        assert_eq!(shown.rows.len(), 2 + COMMON_COMMAND_COUNT);
        assert_eq!(shown.rows[2], PaletteRow::Command(reveal));
        assert_eq!(
            shown.rows[3],
            PaletteRow::Command(command_named("write-tags").unwrap())
        );
    }

    #[test]
    fn a_typed_query_lists_tracks_before_commands() {
        let library = library_of(200);
        let shown = answer(
            "tag",
            library.tracks(),
            |path| library.index_of(path),
            &RecentTracks::default(),
            &CommandUsage::default(),
        );
        assert!(!shown.is_resting);
        assert!(
            shown
                .rows
                .iter()
                .skip(shown.track_rows)
                .all(|row| matches!(row, PaletteRow::Command(_)))
        );
        assert!(shown.rows.len() > shown.track_rows);
        let only_dj = answer(
            "8A 124",
            library.tracks(),
            |path| library.index_of(path),
            &RecentTracks::default(),
            &CommandUsage::default(),
        );
        assert_eq!(only_dj.rows.len(), only_dj.track_rows);
    }

    #[test]
    fn the_selection_wraps_round_both_ends() {
        let mut open = OpenPalette {
            answer: PaletteAnswer {
                rows: vec![
                    PaletteRow::Command(0),
                    PaletteRow::Command(1),
                    PaletteRow::Command(2),
                ],
                track_total: 0,
                track_rows: 0,
                is_resting: true,
            },
            selected: 0,
            opened_at: Instant::now(),
            reduces_motion: true,
        };
        open.move_selection(-1);
        assert_eq!(open.selected, 2);
        open.move_selection(1);
        assert_eq!(open.selected, 0);
        open.move_selection(1);
        assert_eq!(open.selected_row(), Some(PaletteRow::Command(1)));
    }

    #[test]
    fn the_panel_eases_in_over_150_ms_and_is_there_at_once_with_motion_reduced() {
        let start = Instant::now();
        assert_eq!(open_progress(start, start, false), 0.0);
        let half = open_progress(start, start + Duration::from_millis(75), false);
        assert!(half > 0.5 && half < 1.0, "eased out: {half}");
        assert_eq!(
            open_progress(start, start + Duration::from_millis(150), false),
            1.0
        );
        assert_eq!(
            open_progress(start, start + Duration::from_secs(5), false),
            1.0
        );
        assert_eq!(open_progress(start, start, true), 1.0);
    }

    #[test]
    fn ten_thousand_tracks_answer_well_inside_a_frame() {
        let library = library_of(10_000);
        let queries = [
            "8A 124 deep",
            "glass sig",
            "120-126 e6+",
            "a minor",
            "lantern",
        ];
        let mut slowest = Duration::ZERO;
        for typed in queries {
            let started = Instant::now();
            let shown = answer(
                typed,
                library.tracks(),
                |path| library.index_of(path),
                &RecentTracks::default(),
                &CommandUsage::default(),
            );
            let took = started.elapsed();
            assert!(shown.track_total > 0, "{typed}");
            slowest = slowest.max(took);
        }
        assert!(
            slowest < Duration::from_millis(100),
            "the slowest answer took {slowest:?}"
        );
        eprintln!(
            "slowest of {} queries over 10000 tracks: {slowest:?}",
            queries.len()
        );
    }
}
