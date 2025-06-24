#![allow(dead_code)]
//! The history module represents history node archives, suitable
//! for fetching from remote storage to bootstrap a local configuration.
//! There are two different types of history:
//!
//!   1. NodeArchives, containing pd and cometbft node state at a certain height
//!   2. ReindexerArchives, containing all cometbft blocks up to a certain height
//!
//! The NodeArchives are necessary to create ReindexerArchives; in particular, they're required for
//! running a full "reindex" which is required when new ABCI events are backported to old protocol
//! versions.

use anyhow::Context;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio_stream::StreamExt as _;
use url::Url;

mod node;
mod reindexer;

pub use node::{NodeArchive, NodeArchiveSeries};

pub use reindexer::ReindexerArchive;

/// Fetch the archive from the `download_url` and save it locally with optional fancy progress bar.
///
/// In terms of developer experience, this function automatically detects if it's running in an
/// interactive terminal and shows a progress bar accordingly.
/// In headless environments, it falls back to periodic log messages.
///
// This is a rather verbose function, mostly because it supports pretty progress bars
// in interactive terminal sessions. Would be nice to factor out some of the logic.
pub async fn download(
    download_url: &Url,
    dest_file: &Path,
    checksum_sha256: &str,
) -> anyhow::Result<()> {
    if dest_file.exists() {
        tracing::debug!(
            dest_file = dest_file.display().to_string(),
            "file exists, comparing checksum"
        );
        let existing_hash = get_sha256sum(dest_file)?;
        if existing_hash == checksum_sha256 {
            tracing::debug!(
                "archive already exists with correct hash: {} {}",
                dest_file.display(),
                checksum_sha256,
            );
            return Ok(());
        } else {
            tracing::warn!(
                "archive failed to verify via checksum: {} ; expected {}, got {}",
                dest_file.display(),
                checksum_sha256,
                existing_hash,
            );
            tracing::warn!("re-downloading archive: {}", dest_file.display());
        }
    }

    // Create all parent directories before attempting to download file.
    if let Some(parent) = dest_file.parent() {
        tracing::debug!(?parent, "creating parent directory prior to downloading");
        std::fs::create_dir_all(parent)?;
    }

    tracing::info!(%download_url, dest_file=dest_file.display().to_string(), "downloading archive");

    // Determine if we should show fancy progress or use headless logging
    let use_progress_bar = std::io::stderr().is_terminal();

    // Create HTTP client for both HEAD and GET requests
    let client = Client::new();

    // Send HEAD request to get content length
    let total_size = match client.head(download_url.clone()).send().await {
        Ok(response) => response
            .headers()
            .get("content-length")
            .and_then(|ct| ct.to_str().ok())
            .and_then(|ct| ct.parse::<u64>().ok())
            .unwrap_or(0),
        Err(_) => {
            tracing::error!("failed to get content-length via HEAD request");
            0
        }
    };

    if total_size > 0 {
        tracing::debug!(
            "download size: {} bytes ({:.2} MB)",
            total_size,
            total_size as f64 / 1_048_576.0
        );
    } else {
        tracing::debug!("download size: unknown");
    }

    // Setup progress tracking
    let progress_bar = if use_progress_bar {
        let pb = ProgressBar::new(total_size);

        if total_size > 0 {
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
                    .context("failed to set progress bar template")?
                    .progress_chars("##-"),
            );
        } else {
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template(
                        "{spinner:.green} [{elapsed_precise}] {bytes} downloaded ({bytes_per_sec})",
                    )
                    .context("failed to set progress bar template")?,
            );
        }

        pb.set_message("Downloading...");
        Some(pb)
    } else {
        None
    };

    // For headless mode, e.g. running in batch jobs, setup periodic logging
    let mut last_log_time = Instant::now();
    let log_interval = Duration::from_secs(60);
    let mut last_logged_bytes = 0u64;

    // Start the actual download
    let response = client.get(download_url.clone()).send().await?;

    // Check if request was successful
    if !response.status().is_success() {
        if let Some(pb) = &progress_bar {
            pb.abandon_with_message("Download failed");
        }
        anyhow::bail!("Failed to download: HTTP {}", response.status());
    }

    // Create file with same options as original
    let mut download_opts = std::fs::OpenOptions::new();
    download_opts.create(true).write(true).truncate(true);
    let mut f = download_opts
        .open(dest_file)
        .context("failed to open dest filepath for downloading archive")?;

    // Download via stream
    let mut stream = response.bytes_stream();
    let mut downloaded = 0u64;
    let start_time = Instant::now();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        f.write_all(&chunk)?;

        downloaded += chunk.len() as u64;

        // Update progress bar if in headful mode
        if let Some(pb) = &progress_bar {
            pb.set_position(downloaded);

            if total_size > 0 {
                let percentage = (downloaded as f64 / total_size as f64) * 100.0;
                pb.set_message(format!("Downloading... {:.1}%", percentage));
            } else {
                pb.set_message("Downloading...");
            }
        // In headless mode, log periodically
        } else if last_log_time.elapsed() >= log_interval {
            let elapsed = start_time.elapsed();
            let speed = if elapsed.as_secs() > 0 {
                (downloaded - last_logged_bytes) as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };

            if total_size > 0 {
                let percentage = (downloaded as f64 / total_size as f64) * 100.0;
                tracing::info!(
                    "download progress: {:.1}% ({:.2} MB / {:.2} MB) at {:.2} MB/s",
                    percentage,
                    downloaded as f64 / 1_048_576.0,
                    total_size as f64 / 1_048_576.0,
                    speed / 1_048_576.0
                );
            } else {
                tracing::info!(
                    "download progress: {:.2} MB downloaded at {:.2} MB/s",
                    downloaded as f64 / 1_048_576.0,
                    speed / 1_048_576.0
                );
            }

            last_log_time = Instant::now();
            last_logged_bytes = downloaded;
        }
    }

    f.flush()?;

    // Finish progress reporting
    if let Some(pb) = &progress_bar {
        pb.finish_with_message("Download completed");
    } else {
        let elapsed = start_time.elapsed();
        let avg_speed = if elapsed.as_secs() > 0 {
            downloaded as f64 / elapsed.as_secs_f64() / 1_048_576.0
        } else {
            0.0
        };
        tracing::info!(
            "download completed: {:.2} MB in {:.1}s (avg {:.2} MB/s)",
            downloaded as f64 / 1_048_576.0,
            elapsed.as_secs_f64(),
            avg_speed
        );
    }

    // Verify checksum post-download.
    tracing::debug!("verifying checksum");
    let actual_checksum = get_sha256sum(dest_file)?;
    if actual_checksum != checksum_sha256 {
        let msg = format!(
            "archive failed to verify via checksum: {} ; expected {}, got {}",
            dest_file.display(),
            checksum_sha256,
            actual_checksum,
        );
        tracing::error!(msg);
        anyhow::bail!(msg);
    }

    tracing::info!("download complete: {}", dest_file.display());
    Ok(())
}

/// Determine a reasonable filename for the archive, based on a URL.
pub fn basename_from_url(download_url: &Url) -> anyhow::Result<String> {
    let basename = download_url
        .path_segments()
        .ok_or_else(|| anyhow::anyhow!("URL has no path segments"))?
        .last()
        .ok_or_else(|| anyhow::anyhow!("URL has no basename"))?;

    Ok(basename.to_string())
}

/// Utility function to grab a sha256sum for a target file.
fn get_sha256sum<P: AsRef<Path>>(path: P) -> anyhow::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}
