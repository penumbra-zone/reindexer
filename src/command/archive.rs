use anyhow::{anyhow, Context};
use std::path::{Path, PathBuf};

use crate::{cometbft, storage::Storage};

const REINDEXER_FILE_NAME: &'static str = "reindexer_archive.bin";

#[derive(clap::Parser)]
pub struct Archive {
    /// A starting point for reading and writing penumbra data.
    ///
    /// The equivalent of pd's --network-dir.
    ///
    /// Read usage can be overriden with --cometbft-data-dir.
    ///
    /// Write usage can be overriden with --archive-file.
    ///
    /// In this directory we expect there to be:
    ///   - ./cometbft/config/config.toml, for reading cometbft configuration,
    ///   - ./cometbft/data/, for reading cometbft data,
    ///   - (maybe) ./archive.bin, for existing archive data to append to.
    ///
    /// If unset, defaults to ~/.penumbra/network_data/node0.
    #[clap(long)]
    home: Option<String>,
    /// If set, use this directory for cometbft, instead of HOME/cometbft/.
    #[clap(long)]
    cometbft_dir: Option<String>,
    /// If set, use this file for archive data, instead of HOME/reindexer_archive.bin.
    #[clap(long)]
    archive_file: Option<String>,
}

impl Archive {
    /// Get the desired cometbft directory given the command arguments.
    ///
    /// This can fail if the arguments indicate that the home directory
    /// needs to be used, and the home directory cannot be found.
    fn cometbft_dir(&self) -> anyhow::Result<PathBuf> {
        let out = match (self.home.as_ref(), self.cometbft_dir.as_ref()) {
            (_, Some(x)) => x.try_into()?,
            (Some(x), None) => Path::new(x).join("cometbft"),
            (None, None) => home_dir()
                .context("create a home directory, or manually specify a cometbft path")?
                .join(".penumbra/network_data/node0/cometbft"),
        };
        Ok(out)
    }

    /// Get the archive file, based on the command arguments.
    ///
    /// This can fail if we need to use the home directory, and such a directory does not exist.
    fn archive_file(&self) -> anyhow::Result<PathBuf> {
        let out = match (self.home.as_ref(), self.archive_file.as_ref()) {
            (_, Some(x)) => x.try_into()?,
            (Some(x), None) => {
                let mut buf = PathBuf::try_from(x)?;
                buf.push(REINDEXER_FILE_NAME);
                buf
            }
            (None, None) => {
                let mut buf = home_dir()
                    .context("create a home directory, or manually specify an archive file")?;
                buf.push(".penumbra/network_data/node0");
                buf.push(REINDEXER_FILE_NAME);
                buf
            }
        };
        Ok(out)
    }

    /// Create or add to our full historical archive of blocks.
    pub async fn run(self) -> anyhow::Result<()> {
        let archive_file = self.archive_file()?;
        let archive = Storage::new(Some(&archive_file)).await?;

        let mut store = cometbft::Store::new(&self.cometbft_dir()?)?;

        let (store_start, store_end) = match (store.first_height(), store.last_height()) {
            (None, _) | (_, None) => {
                tracing::info!("empty block store, returning");
                return Ok(());
            }
            (Some(start), Some(end)) => (start, end),
        };

        let archive_end = archive.last_height().await?;

        let start = std::cmp::max(store_start, archive_end.unwrap_or(0) + 1);
        let end = store_end;
        // If the end is less than the start, that's odd, and we don't want to just do nothing.
        anyhow::ensure!(
            end >= start,
            "attempting to archive blocks {}..{}",
            start,
            end
        );

        tracing::info!("archiving blocks {}..{}", start, end);
        for height in start..end {
            tracing::debug!("archiving block {}", height);

            let block = store
                .block_by_height(height)?
                .ok_or(anyhow!("missing block at height {}", height))?;

            anyhow::ensure!(
                block.height() == height,
                "block with height {} instead of {}",
                block.height(),
                height
            );

            archive.put_block(block).await?;
            break;
        }
        Ok(())
    }
}

/// Retrieve the home directory for the user running this program.
///
/// This may not exist on certain platforms, hence the error.
fn home_dir() -> anyhow::Result<PathBuf> {
    Ok(directories::UserDirs::new()
        .ok_or(anyhow!("no user directories on platform"))?
        .home_dir()
        .to_path_buf())
}
