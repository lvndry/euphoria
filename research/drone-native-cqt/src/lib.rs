// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared research compute API and artifact contracts.
mod evidence;
pub mod plan;
pub use evidence::{artifact_root, diagnostics, read_f32};
