// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A bounded FLAC encoder: write each frame, then patch STREAMINFO at EOF.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use flacenc::component::{BitRepr, Stream};
use flacenc::error::{Verified, Verify};
use flacenc::source::{Context, Fill, FrameBuf};

use super::{BITS_PER_SAMPLE, write_error};
use crate::EuphoriaError;

pub(super) struct FlacWriter {
    path: PathBuf,
    file: BufWriter<File>,
    config: Verified<flacenc::config::Encoder>,
    header: Stream,
    frame: FrameBuf,
    context: Context,
    pending: Vec<i32>,
    channels: usize,
    frame_number: usize,
}

impl FlacWriter {
    pub(super) fn new(
        file: File,
        path: &Path,
        channels: u16,
        sample_rate: u32,
    ) -> Result<Self, EuphoriaError> {
        let error = |reason: String| write_error(path, reason);
        let config = flacenc::config::Encoder::default()
            .into_verified()
            .map_err(|e| error(format!("{e:?}")))?;
        let channels = usize::from(channels);
        let mut header = Stream::new(sample_rate as usize, channels, usize::from(BITS_PER_SAMPLE))
            .map_err(|e| error(e.to_string()))?;
        header
            .stream_info_mut()
            .set_block_sizes(config.block_size, config.block_size)
            .map_err(|e| error(e.to_string()))?;
        let frame =
            FrameBuf::with_size(channels, config.block_size).map_err(|e| error(e.to_string()))?;
        let mut writer = Self {
            path: path.to_path_buf(),
            file: BufWriter::new(file),
            header,
            frame,
            context: Context::new(usize::from(BITS_PER_SAMPLE), channels),
            pending: Vec::with_capacity(config.block_size * channels),
            config,
            channels,
            frame_number: 0,
        };
        writer.write_header()?;
        Ok(writer)
    }

    pub(super) fn write(&mut self, samples: &[f32]) -> Result<(), EuphoriaError> {
        let capacity = self.config.block_size * self.channels;
        for sample in samples {
            self.pending.push(super::to_integer(*sample));
            if self.pending.len() == capacity {
                self.write_frame()?;
            }
        }
        Ok(())
    }

    fn write_frame(&mut self) -> Result<(), EuphoriaError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let error = |reason: String| write_error(&self.path, reason);
        self.frame
            .fill_interleaved(&self.pending)
            .map_err(|e| error(e.to_string()))?;
        let frame = flacenc::encode_fixed_size_frame(
            &self.config,
            &self.frame,
            self.frame_number,
            self.header.stream_info(),
        )
        .map_err(|e| error(e.to_string()))?;
        let mut sink = flacenc::bitsink::ByteSink::new();
        frame
            .write(&mut sink)
            .map_err(|e| error(format!("{e:?}")))?;
        self.file
            .write_all(sink.as_slice())
            .map_err(|e| error(e.to_string()))?;
        self.header.stream_info_mut().update_frame_info(&frame);
        self.context
            .fill_interleaved(&self.pending)
            .map_err(|e| error(e.to_string()))?;
        self.frame_number += 1;
        self.pending.clear();
        Ok(())
    }

    fn write_header(&mut self) -> Result<(), EuphoriaError> {
        let mut sink = flacenc::bitsink::ByteSink::new();
        self.header
            .write(&mut sink)
            .map_err(|e| write_error(&self.path, format!("{e:?}")))?;
        self.file
            .write_all(sink.as_slice())
            .map_err(|e| write_error(&self.path, e.to_string()))
    }

    pub(super) fn finish(mut self) -> Result<u64, EuphoriaError> {
        self.write_frame()?;
        // Fixed-block frame numbers use this stride, including when the last
        // frame is shorter. Advertising the tail as the minimum breaks seeking.
        self.header
            .stream_info_mut()
            .set_block_sizes(self.config.block_size, self.config.block_size)
            .map_err(|e| write_error(&self.path, e.to_string()))?;
        self.header
            .stream_info_mut()
            .set_md5_digest(&self.context.md5_digest());
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|e| write_error(&self.path, e.to_string()))?;
        self.write_header()?;
        self.file
            .flush()
            .map_err(|e| write_error(&self.path, e.to_string()))?;
        Ok(self.context.total_samples() as u64 * self.channels as u64)
    }
}
