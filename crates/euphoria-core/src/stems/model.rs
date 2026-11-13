// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The htdemucs model: the network graph built into Euphoria, and the
//! weights downloaded once from the Demucs author's published files.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::error::{ErrorSource, EuphoriaError};

/// The htdemucs network without its weights, exported from Demucs by
/// `scripts/stems/export_htdemucs_onnx.py`.
pub(crate) const GRAPH: &[u8] = include_bytes!("../../models/htdemucs.onnx");

/// The SHA-256 of [`GRAPH`], as recorded in `models/README.md`.
pub(crate) const GRAPH_SHA256: &str =
    "b69d128b976899d71a1f3293130adafb6ed0b2b25adc02804105ef6819e1147d";

/// Where the weights are downloaded from: the `htdemucs` model as its
/// author publishes it on Hugging Face, pinned to one revision.
pub const WEIGHTS_URL: &str = "https://huggingface.co/adefossez/HTDemucs/resolve/cbc8a9b1a87023b7fd74e7b3412e6321c0eab003/955717e8.safetensors";
/// The SHA-256 of the weights file. A download that does not match is
/// discarded.
pub const WEIGHTS_SHA256: &str = "d9fa14133cfcc034a6758923bb3a8ca9f8dfd0b582134643bbf83f72c17576dd";
/// The size of the weights file in bytes.
pub const WEIGHTS_BYTES: u64 = 84_025_440;
const WEIGHTS_FILE_NAME: &str = "htdemucs-955717e8.safetensors";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
// Includes headers, redirects and the complete 84 MB response body.
const DOWNLOAD_DEADLINE: Duration = Duration::from_secs(600);
const READ_BUFFER_BYTES: usize = 1 << 16;
static PARTIAL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Where the weights are kept in Euphoria's data folder.
pub fn default_weights_path() -> Result<PathBuf, EuphoriaError> {
    Ok(crate::cache::location::default_data_directory()?
        .join("models")
        .join(WEIGHTS_FILE_NAME))
}

/// Reads the weights at `path`, downloading them first when the file is
/// missing. `downloaded` hears the bytes received so far and the total.
///
/// The file is checked against [`WEIGHTS_SHA256`] every time it is read, so
/// a damaged file is reported, never used. Downloads have a ten-minute
/// deadline covering connection, redirects, headers and the response body.
pub(crate) fn load_weights(
    path: &Path,
    downloaded: &dyn Fn(u64, u64),
) -> Result<Vec<u8>, EuphoriaError> {
    if !path.exists() {
        download_weights(WEIGHTS_URL, path, downloaded)?;
    }
    let bytes = std::fs::read(path).map_err(|source| EuphoriaError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let actual = sha256_hex(&bytes);
    if actual != WEIGHTS_SHA256 {
        return Err(EuphoriaError::ModelIntegrity {
            path: path.to_path_buf(),
            expected: WEIGHTS_SHA256,
            actual,
        });
    }
    Ok(bytes)
}

fn download_weights(
    url: &str,
    destination: &Path,
    downloaded: &dyn Fn(u64, u64),
) -> Result<(), EuphoriaError> {
    download_weights_with_deadline(url, destination, downloaded, DOWNLOAD_DEADLINE)
}

fn download_weights_with_deadline(
    url: &str,
    destination: &Path,
    downloaded: &dyn Fn(u64, u64),
    deadline: Duration,
) -> Result<(), EuphoriaError> {
    let download_error = |source: ErrorSource| EuphoriaError::ModelDownload {
        url: url.to_string(),
        destination: destination.to_path_buf(),
        source,
    };
    let file_error = |source: std::io::Error| EuphoriaError::ModelFile {
        path: destination.to_path_buf(),
        source,
    };
    if let Some(folder) = destination.parent() {
        std::fs::create_dir_all(folder).map_err(file_error)?;
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(deadline))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .build(),
        )
        .build()
        .into();
    let response = agent
        .get(url)
        .call()
        .map_err(|error| download_error(Box::new(error)))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(WEIGHTS_BYTES);
    let mut reader = response
        .into_body()
        .into_with_config()
        .limit(WEIGHTS_BYTES + 1)
        .reader();

    let (file, partial) = create_partial(destination).map_err(file_error)?;
    let saved = save_download(&mut reader, file, total, downloaded);
    let actual = match saved {
        Ok(actual) => actual,
        Err(error) => {
            let _ = std::fs::remove_file(&partial);
            return Err(match error {
                SaveError::Network(source) => download_error(Box::new(source)),
                SaveError::Disk(source) => file_error(source),
            });
        }
    };
    if actual != WEIGHTS_SHA256 {
        let _ = std::fs::remove_file(&partial);
        return Err(EuphoriaError::ModelIntegrity {
            path: destination.to_path_buf(),
            expected: WEIGHTS_SHA256,
            actual,
        });
    }
    if let Err(source) = std::fs::rename(&partial, destination) {
        let _ = std::fs::remove_file(&partial);
        return Err(file_error(source));
    }
    Ok(())
}

fn create_partial(destination: &Path) -> std::io::Result<(File, PathBuf)> {
    for _ in 0..64 {
        let partial = destination.with_file_name(format!(
            ".euphoria-model-{}-{}.part",
            std::process::id(),
            PARTIAL_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        match File::options().write(true).create_new(true).open(&partial) {
            Ok(file) => return Ok((file, partial)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "no unused model download name was available",
    ))
}

enum SaveError {
    Network(std::io::Error),
    Disk(std::io::Error),
}

/// Writes the body into the reserved file and returns its SHA-256.
fn save_download(
    reader: &mut impl Read,
    mut file: File,
    total: u64,
    downloaded: &dyn Fn(u64, u64),
) -> Result<String, SaveError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; READ_BUFFER_BYTES];
    let mut received: u64 = 0;
    loop {
        let read = reader.read(&mut buffer).map_err(SaveError::Network)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(SaveError::Disk)?;
        received += read as u64;
        downloaded(received, total);
    }
    file.sync_all().map_err(SaveError::Disk)?;
    Ok(hex(&hasher.finalize()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_graph_is_an_onnx_model_without_weights() {
        assert!(GRAPH.len() < 8 * 1024 * 1024, "{} bytes", GRAPH.len());
        assert!(
            GRAPH
                .windows(b"htdemucs.weights".len())
                .any(|window| window == b"htdemucs.weights")
        );
    }

    #[test]
    fn the_graph_is_the_recorded_export() {
        assert_eq!(sha256_hex(GRAPH), GRAPH_SHA256);
    }

    #[test]
    fn hashes_are_written_in_lowercase_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_download_never_opens_a_preexisting_partial_symlink() {
        use std::net::TcpListener;
        let folder =
            std::env::temp_dir().join(format!("euphoria-weights-symlink-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let original = folder.join("original");
        std::fs::write(&original, b"original file bytes").unwrap();
        let destination = folder.join("weights.safetensors");
        let partial = destination.with_extension("part");
        std::os::unix::fs::symlink(&original, &partial).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/weights", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            assert!(client.read(&mut request).unwrap() > 0);
            client
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc")
                .unwrap();
        });
        let result = download_weights(&url, &destination, &|_, _| {});
        server.join().unwrap();
        let original_bytes = std::fs::read(&original).unwrap();
        let unowned_link_remains = std::fs::symlink_metadata(&partial)
            .is_ok_and(|metadata| metadata.file_type().is_symlink());
        std::fs::remove_dir_all(folder).unwrap();
        assert!(matches!(result, Err(EuphoriaError::ModelIntegrity { .. })));
        assert_eq!(original_bytes, b"original file bytes");
        assert!(
            unowned_link_remains,
            "cleanup must not remove an unclaimed partial file"
        );
    }

    #[test]
    fn stalled_headers_and_bodies_time_out_and_leave_no_partial_file() {
        use std::net::TcpListener;
        for headers_first in [false, true] {
            let folder = std::env::temp_dir().join(format!(
                "euphoria-weights-timeout-{}-{headers_first}",
                std::process::id()
            ));
            let destination = folder.join("weights");
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/weights", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut client, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                assert!(client.read(&mut request).unwrap() > 0);
                let headers = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\n";
                if headers_first {
                    client.write_all(headers).unwrap();
                }
                std::thread::sleep(Duration::from_millis(500));
                if !headers_first {
                    let _ = client.write_all(headers);
                }
                let _ = client.write_all(b"abc");
            });
            let began = std::time::Instant::now();
            let result = download_weights_with_deadline(
                &url,
                &destination,
                &|_, _| {},
                Duration::from_millis(100),
            );
            let elapsed = began.elapsed();
            server.join().unwrap();
            let empty = std::fs::read_dir(&folder).unwrap().next().is_none();
            let _ = std::fs::remove_dir_all(folder);
            assert!(
                matches!(result, Err(EuphoriaError::ModelDownload { .. })),
                "{result:?}"
            );
            assert!(elapsed < Duration::from_millis(400), "{elapsed:?}");
            assert!(empty, "failed downloads leave no file");
        }
    }

    #[test]
    fn an_unreachable_server_is_a_download_error_and_leaves_no_file() {
        let folder = std::env::temp_dir().join(format!("euphoria-weights-{}", std::process::id()));
        let destination = folder.join("weights.safetensors");
        let error = download_weights("http://127.0.0.1:9/weights", &destination, &|_, _| {})
            .expect_err("nothing listens on the discard port");
        assert!(
            matches!(error, EuphoriaError::ModelDownload { .. }),
            "{error:?}"
        );
        assert!(!destination.exists());
        let _ = std::fs::remove_dir_all(folder);
    }
}
