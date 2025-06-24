use anyhow::Context;
use std::path::PathBuf;

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
}

impl Bootstrap {
    /// Create config dir, and fetch a remote ReindexerArchive.
    pub async fn run(self) -> anyhow::Result<()> {
        // Validate args
        if self.home.is_some() && self.archive_file.is_some() {
            tracing::warn!("in correct error state; i think it's fine to keep going here.");
            anyhow::bail!("cannot use both --home and --archive-file options");
        }

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
            tracing::info!(
                compressed_file = dest_file.display().to_string(),
                extracted_file = extracted_file.display().to_string(),
                "extracting gzipped archive"
            );
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

        Ok(())
    }
}
