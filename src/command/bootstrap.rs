use anyhow::Context;
use std::path::PathBuf;
use tokio::task::JoinSet;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::io::IsTerminal;

use crate::files::archive_filepath_from_opts;

#[derive(clap::Parser)]
pub struct Bootstrap {
    /// The home directory for the penumbra-reindexer.
    ///
    /// Downloaded large files will be stored within this directory.
    ///
    /// Defaults to `~/.local/share/penumbra-reindexer`.
    /// Can be overridden with --archive-file.
    #[clap(long)]
    home: Option<PathBuf>,

    /// Override the filepath for the sqlite3 database.
    /// Defaults to <REINDEXER_HOME>/<CHAIN_ID>/reindexer-archive.sqlite
    #[clap(long)]
    archive_file: Option<PathBuf>,

    /// Declare a specific chain id to bootstrap a config for.
    ///
    /// Returns an error if the specified chain id is not supported.
    /// Defaults to `penumbra-1` for mainnet.
    #[clap(long)]
    chain_id: Option<String>,

    /// Use a remote CometBFT RPC URL to fetch chain id from.
    ///
    /// Setting this option will pool a remote node for chain info,
    /// and initialize event archives based on the `chain_id` returned,
    /// if supported.
    #[clap(long)]
    remote_rpc: Option<String>,

    /// Overwrite any pre-existing reindexer archive for the relevant chain.
    ///
    /// Ensures that the local reindexer archive is reset to match what's configured
    /// as a remote base storage.
    #[clap(long)]
    force: bool,

    /// Download all NodeArchives for the chain in parallel.
    ///
    /// This will download all historical node state archives required for the chain,
    /// which can be useful for bootstrapping a complete node history.
    #[clap(long)]
    download_node_archives: bool,
}

impl Bootstrap {
    /// Create config dir, and fetch a remote ReindexerArchive.
    pub async fn run(self) -> anyhow::Result<()> {
        // Validate args
        if self.home.is_some() && self.archive_file.is_some() {
            tracing::warn!("in correct error state; i think it's fine to keep going here.");
            anyhow::bail!("cannot use both --home and --archive-file options");
        }

        // Extract values before moving self
        let download_node_archives = self.download_node_archives;
        let force = self.force;

        // For now, let's default to a reasonable chain id.
        let chain_id = match self.chain_id {
            Some(c) => c,
            None => {
                tracing::warn!("no chain id specified, defaulting to 'penumbra-1' mainnet");
                String::from("penumbra-1")
            }
        };

        let home = self.home.unwrap_or(crate::files::default_reindexer_home()?);
        let archive_file = archive_filepath_from_opts(
            Some(home.clone()),
            self.archive_file,
            Some(chain_id.clone()),
        )?;

        // Create parent directory
        let par_dir = archive_file
            .parent()
            .expect("archive file must have parent directory");
        if !par_dir.exists() {
            std::fs::create_dir_all(par_dir)
                .context("failed to create parent directory for archive file")?;
        }

        tracing::info!(%chain_id, "bootstrapping reindexer setup");
        let reindexer_archive = crate::history::ReindexerArchive::try_from(chain_id.clone())?;

        // Get name of file as it will be downloaded
        let dest_file = home
            .join(chain_id.clone())
            .join(crate::history::basename_from_url(
                &reindexer_archive.download_url,
            )?);

        // Remember whether archive was downloaded, for appropriate logging messages post-op.
        let archive_already_existed = archive_file.exists();
        if dest_file.exists() {
            tracing::info!(
                dest_file = dest_file.display().to_string(),
                "reindexer archive already exists, validating checksum"
            );
        }

        reindexer_archive
            .download(&dest_file)
            .await
            .context("failed to download archive_file")?;

        // Extract gzipped file if necessary
        let final_dest_file = if dest_file.extension().and_then(|s| s.to_str()) == Some("gz") {
            let extracted_file = dest_file.with_extension("");
            if !extracted_file.exists() || self.force {
                tracing::info!(
                    compressed_file = dest_file.display().to_string(),
                    extracted_file = extracted_file.display().to_string(),
                    "extracting gzipped archive"
                );
            } else {
                tracing::debug!(
                    compressed_file = dest_file.display().to_string(),
                    extracted_file = extracted_file.display().to_string(),
                    "archive already extracted, not clobbering"
                );
            }
            reindexer_archive
                .extract(&dest_file, &extracted_file)
                .await
                .context("failed to extract gzipped archive")?;
            extracted_file
        } else {
            dest_file.clone()
        };

        // Warn about not clobbering
        if archive_already_existed && !self.force {
            tracing::warn!(
                archive_file = archive_file.display().to_string(),
                "reindexer archive already exists, not clobbering"
            );
        } else {
            // Copy file over to actual download location. The named params in the logging msg
            // are reversed.
            tracing::debug!(
                src_file = final_dest_file.display().to_string(),
                dest_file = archive_file.display().to_string(),
                "copying archive to final location"
            );
            std::fs::copy(&final_dest_file, &archive_file)
                .context("failed to copy reindexer archive to final location after downloading")?;
        }

        if download_node_archives {
            tracing::info!("downloading node archives for chain {}", chain_id);
            Bootstrap::download_node_archives_static(&chain_id, &home, force).await
                .context("failed to download node archives")?;
        }

        Ok(())
    }

    /// Download all NodeArchives for a given chain in parallel with progress bars.
    pub async fn download_node_archives_static(
        chain_id: &str,
        home: &PathBuf,
        force: bool,
    ) -> anyhow::Result<()> {
        let node_archive_series = crate::history::NodeArchiveSeries::from_chain_id(chain_id)
            .context("failed to get node archive series for chain")?;

        let archives = node_archive_series.archives;
        let num_archives = archives.len();

        if num_archives == 0 {
            tracing::info!("no node archives available for chain {}", chain_id);
            return Ok(());
        }

        tracing::info!(
            "downloading {} node archives for chain {} in parallel",
            num_archives,
            chain_id
        );

        let use_progress_bars = std::io::stderr().is_terminal();
        let multi_progress = if use_progress_bars {
            Some(MultiProgress::new())
        } else {
            None
        };

        let mut join_set = JoinSet::new();

        for (index, archive) in archives.into_iter().enumerate() {
            let home_dir = home.clone();
            let chain_id_clone = chain_id.to_string();
            let multi_progress_clone = multi_progress.clone();

            join_set.spawn(async move {
                Self::download_single_node_archive(
                    archive,
                    &home_dir,
                    &chain_id_clone,
                    force,
                    index,
                    multi_progress_clone,
                )
                .await
            });
        }

        let mut success_count = 0;
        let mut errors = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(())) => {
                    success_count += 1;
                }
                Ok(Err(e)) => {
                    errors.push(e);
                }
                Err(e) => {
                    errors.push(anyhow::anyhow!("task join error: {}", e));
                }
            }
        }

        if let Some(mp) = multi_progress {
            mp.clear().context("failed to clear multi progress")?;
        }

        if !errors.is_empty() {
            tracing::error!("some downloads failed:");
            for error in &errors {
                tracing::error!("  {}", error);
            }
            anyhow::bail!(
                "{} of {} downloads failed",
                errors.len(),
                num_archives
            );
        }

        tracing::info!(
            "successfully downloaded all {} node archives for chain {}",
            success_count,
            chain_id
        );

        Ok(())
    }

    async fn download_single_node_archive(
        archive: crate::history::NodeArchive,
        home: &PathBuf,
        chain_id: &str,
        force: bool,
        _index: usize,
        multi_progress: Option<MultiProgress>,
    ) -> anyhow::Result<()> {
        let basename = crate::history::basename_from_url(&archive.download_url)?;
        let dest_file = home.join(chain_id).join(&basename);

        if dest_file.exists() && !force {
            tracing::debug!(
                "archive {} already exists, skipping download",
                dest_file.display()
            );
            return Ok(());
        }

        if let Some(parent) = dest_file.parent() {
            std::fs::create_dir_all(parent)
                .context("failed to create parent directory for archive")?;
        }

        let progress_bar = if let Some(ref mp) = multi_progress {
            let pb = mp.add(ProgressBar::new_spinner());
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} [{elapsed_precise}] {msg}")
                    .context("failed to set progress bar template")?,
            );
            pb.set_message(format!("{}: Starting download...", basename));
            Some(pb)
        } else {
            None
        };

        let result = Self::download_with_progress_bar(
            &archive.download_url,
            &dest_file,
            &archive.checksum_sha256,
            progress_bar.as_ref(),
            &basename,
        )
        .await;

        if let Some(pb) = progress_bar {
            match &result {
                Ok(()) => pb.finish_with_message(format!("{}: ✓ Complete", basename)),
                Err(_) => pb.abandon_with_message(format!("{}: ✗ Failed", basename)),
            }
        }

        result
    }

    async fn download_with_progress_bar(
        download_url: &url::Url,
        dest_file: &std::path::Path,
        checksum_sha256: &str,
        progress_bar: Option<&ProgressBar>,
        basename: &str,
    ) -> anyhow::Result<()> {
        use reqwest::Client;
        use std::io::Write;
        use std::time::Instant;
        use tokio_stream::StreamExt;

        if dest_file.exists() {
            let existing_hash = Self::get_sha256sum(dest_file)?;
            if existing_hash == checksum_sha256 {
                if let Some(pb) = progress_bar {
                    pb.set_message(format!("{}: Already exists with correct checksum", basename));
                }
                return Ok(());
            } else {
                if let Some(pb) = progress_bar {
                    pb.set_message(format!("{}: Re-downloading (checksum mismatch)", basename));
                }
            }
        }

        let client = Client::new();
        
        let total_size = match client.head(download_url.clone()).send().await {
            Ok(response) => response
                .headers()
                .get("content-length")
                .and_then(|ct| ct.to_str().ok())
                .and_then(|ct| ct.parse::<u64>().ok())
                .unwrap_or(0),
            Err(_) => 0,
        };

        if let Some(pb) = progress_bar {
            if total_size > 0 {
                pb.set_length(total_size);
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template("{spinner:.green} [{elapsed_precise}] {msg} [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
                        .context("failed to set progress bar template")?
                        .progress_chars("##-"),
                );
            }
            pb.set_message(format!("{}: Downloading...", basename));
        }

        let response = client.get(download_url.clone()).send().await?;
        
        if !response.status().is_success() {
            anyhow::bail!("Failed to download: HTTP {}", response.status());
        }

        let mut file = std::fs::File::create(dest_file)
            .context("failed to create destination file")?;

        let mut stream = response.bytes_stream();
        let mut downloaded = 0u64;
        let start_time = Instant::now();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;

            if let Some(pb) = progress_bar {
                pb.set_position(downloaded);
                if total_size > 0 {
                    let percentage = (downloaded as f64 / total_size as f64) * 100.0;
                    pb.set_message(format!("{}: {:.1}%", basename, percentage));
                }
            }
        }

        file.flush()?;

        let actual_checksum = Self::get_sha256sum(dest_file)?;
        if actual_checksum != checksum_sha256 {
            anyhow::bail!(
                "checksum verification failed: expected {}, got {}",
                checksum_sha256,
                actual_checksum
            );
        }

        let elapsed = start_time.elapsed();
        if let Some(pb) = progress_bar {
            pb.set_message(format!(
                "{}: Downloaded {:.2} MB in {:.1}s",
                basename,
                downloaded as f64 / 1_048_576.0,
                elapsed.as_secs_f64()
            ));
        }

        Ok(())
    }

    fn get_sha256sum<P: AsRef<std::path::Path>>(path: P) -> anyhow::Result<String> {
        use sha2::{Digest, Sha256};
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher)?;
        Ok(format!("{:x}", hasher.finalize()))
    }
}
