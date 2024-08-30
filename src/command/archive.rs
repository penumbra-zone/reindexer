use anyhow::anyhow;
use std::path::PathBuf;

use crate::{
    cometbft,
    files::{default_penumbra_home, REINDEXER_FILE_NAME},
    storage::Storage,
};

// # Organization
//
// The data flow for this file is:
//  Archive -> ParsedCommand -> Archiver.
//
// First, we transform the user provided options into direct, unambiguous information.
// For example, figuring out the actual paths where data is stored, having used home directories
// and overrides as the user specified.
//
// Then, we read information in order to prepare the data we need to archive, and the storage
// we'll archive the data to.

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
    ///   - (maybe) ./reindexer_archive.bin, for existing archive data to append to.
    ///
    /// If unset, defaults to ~/.penumbra/network_data/node0.
    #[clap(long)]
    home: Option<PathBuf>,
    /// If set, use this directory for cometbft, instead of HOME/cometbft/.
    #[clap(long)]
    cometbft_dir: Option<PathBuf>,
    /// If set, use this file for archive data, instead of HOME/reindexer_archive.bin.
    #[clap(long)]
    archive_file: Option<PathBuf>,
}

impl Archive {
    /// Get the desired cometbft directory given the command arguments.
    ///
    /// This can fail if the arguments indicate that the home directory
    /// needs to be used, and the home directory cannot be found.
    fn cometbft_dir(&self) -> anyhow::Result<PathBuf> {
        let out = match (self.home.as_ref(), self.cometbft_dir.as_ref()) {
            (_, Some(x)) => x.to_owned(),
            (Some(x), None) => x.join("cometbft"),
            (None, None) => default_penumbra_home()?.join("cometbft"),
        };
        Ok(out)
    }

    /// Get the archive file, based on the command arguments.
    ///
    /// This can fail if we need to use the home directory, and such a directory does not exist.
    fn archive_file(&self) -> anyhow::Result<PathBuf> {
        let out = match (self.home.as_ref(), self.archive_file.as_ref()) {
            (_, Some(x)) => x.to_owned(),
            (Some(x), None) => {
                let mut buf = x.to_owned();
                buf.push(REINDEXER_FILE_NAME);
                buf
            }
            (None, None) => {
                let mut buf = default_penumbra_home()?;
                buf.push(REINDEXER_FILE_NAME);
                buf
            }
        };
        Ok(out)
    }

    /// Create or add to our full historical archive of blocks.
    pub async fn run(self) -> anyhow::Result<()> {
        let cometbft_dir = self.cometbft_dir()?;
        let archive_file = self.archive_file()?;
        ParsedCommand::new(cometbft_dir, archive_file).run().await
    }
}

/// This represents the result of performing a bit of parsing of the command.
///
/// We need to reduce some of the redundant options into a more direct set of information.
struct ParsedCommand {
    /// The directory where cometbft information is stored.
    cometbft_dir: PathBuf,
    /// The file to store our archive database.
    archive_file: PathBuf,
}

impl ParsedCommand {
    pub fn new(cometbft_dir: PathBuf, archive_file: PathBuf) -> Self {
        Self {
            cometbft_dir,
            archive_file,
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let config = cometbft::Config::read_dir(&self.cometbft_dir)?;
        let genesis = cometbft::Genesis::read_cometbft_dir(&self.cometbft_dir, &config)?;
        let store = cometbft::Store::new(&self.cometbft_dir, &config)?;
        let archive = Storage::new(Some(&self.archive_file), Some(&genesis.chain_id())).await?;

        Archiver::new(genesis, store, archive).run().await
    }
}

/// Responsible for actually running the archival process.
///
/// This is a bit of an OOP verb-object, but it serves the purpose of organizing
/// the information needed
struct Archiver {
    /// The genesis information we need to place in the archive.
    genesis: cometbft::Genesis,
    /// The store of cometbft information.
    store: cometbft::Store,
    /// The place where our archive resides.
    archive: Storage,
}

impl Archiver {
    pub fn new(genesis: cometbft::Genesis, store: cometbft::Store, archive: Storage) -> Self {
        Self {
            genesis,
            store,
            archive,
        }
    }

    /// Retreive the bounds we need to archive between
    async fn bounds(&mut self) -> anyhow::Result<Option<(u64, u64)>> {
        let (store_start, store_end) = match (self.store.first_height(), self.store.last_height()) {
            (None, _) | (_, None) => return Ok(None),
            (Some(start), Some(end)) => (start, end),
        };

        let archive_end = self.archive.last_height().await?;

        let start = std::cmp::max(store_start, archive_end.unwrap_or(0) + 1);
        let end = store_end;
        return Ok(Some((start, end)));
    }

    async fn archive_genesis(&self) -> anyhow::Result<()> {
        tracing::info!(
            initial_height = self.genesis.initial_height(),
            "archiving genesis"
        );
        self.archive.put_genesis(&self.genesis).await?;
        Ok(())
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        self.archive_genesis().await?;

        let (start, end) = match self.bounds().await? {
            None => {
                tracing::info!("empty archival range, returning");
                return Ok(());
            }
            Some((x, y)) if y < x => {
                tracing::info!("empty archival range, returning");
                return Ok(());
            }
            Some(x) => x,
        };

        tracing::info!("archiving blocks {}..{}", start, end);

        for height in start..=end {
            if (height - start) % 1000 == 0 {
                tracing::info!("archiving block {}", height);
            } else {
                tracing::debug!("archiving block {}", height);
            }

            let block = self
                .store
                .block_by_height(height)?
                .ok_or(anyhow!("missing block at height {}", height))?;
            self.archive.put_block(&block).await?;
        }

        Ok(())
    }
}
