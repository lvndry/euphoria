// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use lofty::config::WriteOptions;
use lofty::id3::v2::{Frame, FrameId, Id3v2Tag, TextInformationFrame};
use lofty::mp4::{Atom, AtomData, AtomIdent, Ilst};
use lofty::ogg::tag::VorbisComments;
use lofty::tag::TagExt;
use lofty::TextEncoding;
use std::path::Path;

fn seed(path: &Path) {
    match path.extension().and_then(|e| e.to_str()).unwrap() {
        "mp3" => {
            let mut tag = Id3v2Tag::new();
            tag.insert(Frame::Text(TextInformationFrame::new(
                FrameId::new("TKEY").unwrap(),
                TextEncoding::UTF8,
                "8A 114",
            )));
            tag.save_to_path(path, WriteOptions::new().use_id3v23(true))
                .unwrap();
        }
        "m4a" => {
            let mut ilst = Ilst::new();
            ilst.insert(Atom::new(
                AtomIdent::Freeform {
                    mean: "com.apple.iTunes".into(),
                    name: "initialkey".into(),
                },
                AtomData::UTF8("8A".to_string()),
            ));
            ilst.insert(Atom::new(
                AtomIdent::Freeform {
                    mean: "com.apple.iTunes".into(),
                    name: "KEY".into(),
                },
                AtomData::UTF8("A minor".to_string()),
            ));
            ilst.save_to_path(path, WriteOptions::new()).unwrap();
        }
        "flac" | "ogg" => {
            let mut comments = VorbisComments::new();
            comments.push("INITIALKEY".to_string(), "8A".to_string());
            comments.push("KEY".to_string(), "A minor".to_string());
            comments.save_to_path(path, WriteOptions::new()).unwrap();
        }
        other => panic!("no seed for {other}"),
    }
}

fn main() {
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let tmp = Path::new(std::env::temp_dir().to_str().unwrap())
        .join(format!("euphoria-probe-keys-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let names = [
        "sine-440hz-mono.mp3",
        "sine-440hz-mono.m4a",
        "sine-440hz-mono.flac",
        "sine-440hz-mono.ogg",
    ];
    for name in names {
        let p = tmp.join(name);
        std::fs::copy(dir.join(name), &p).unwrap();
        seed(&p);
        let extension = p.extension().and_then(|e| e.to_str()).unwrap();
        let stream = MediaSourceStream::new(Box::new(std::fs::File::open(&p).unwrap()), Default::default());
        let mut hint = Hint::new();
        hint.with_extension(extension);
        match symphonia::default::get_probe().probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        ) {
            Ok(mut reader) => {
                let mut md = reader.metadata();
                println!("=== {name} ===");
                loop {
                    if let Some(rev) = md.current() {
                        for tag in rev.media.tags.iter() {
                            println!(
                                "  has_std={} raw.key={:?} raw.value={:?}",
                                tag.std.is_some(),
                                tag.raw.key,
                                tag.raw.value
                            );
                        }
                    }
                    if md.pop().is_none() {
                        break;
                    }
                }
            }
            Err(e) => println!("{name}: {e}"),
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
