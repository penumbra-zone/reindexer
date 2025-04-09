use std::path::PathBuf;

use crate::{
    cometbft::{RemoteStore, Store},
    files::{default_penumbra_home, REINDEXER_FILE_NAME},
    indexer::Indexer,
    penumbra::Regenerator,
    storage::Storage,
};

#[derive(clap::Parser)]
pub struct Regen {
    /// The URL for the database where we should store the produced events.
    #[clap(long)]
    database_url: String,
    /// A home directory to read Penumbra data from.
    ///
    /// We expect there to be a ./reindexer_archive.bin file in this directory.
    /// Use `--archive-file` to specify an archive in a different location.
    ///
    /// Defaults to `~/.penumbra/network_data/node0`, the same default for `pd start`.
    #[clap(long)]
    home: Option<PathBuf>,
    /// Override the location of the sqlite3 database from which event data will be read.
    /// Defaults to `<HOME>/reindexer_archive.bin`.
    #[clap(long)]
    archive_file: Option<PathBuf>,
    /// If set, index events starting from this height.
    #[clap(long)]
    start_height: Option<u64>,
    /// If set, index events up to and including this height.
    ///
    /// For example, if this is set to 2, only events in blocks 1, 2 will be indexed.
    #[clap(long)]
    stop_height: Option<u64>,
    /// If set, use a given directory to store the working reindexing state.
    ///
    /// This allows resumption of reindexing, by reusing the directory.
    #[clap(long)]
    working_dir: Option<PathBuf>,
    /// If set, poll a remote CometBFT RPC URL to fetch new blocks continuously.
    ///
    /// If a stop height is not set, this will run regeneration indefinitely.
    #[clap(long)]
    follow: Option<String>,
}

impl Regen {
    /// Resolve the path of the archive file
    fn archive_file(&self) -> anyhow::Result<PathBuf> {
        let out = match (self.home.as_ref(), self.archive_file.as_ref()) {
            (_, Some(x)) => x.to_owned(),
            (Some(x), None) => x.join(REINDEXER_FILE_NAME),
            (None, None) => default_penumbra_home()?.join(REINDEXER_FILE_NAME),
        };
        Ok(out)
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let archive_file = self.archive_file()?;

        let store: Option<Box<dyn Store>> = match self.follow {
            None => None,
            Some(x) => Some(Box::new(RemoteStore::new(x))),
        };

        let archive = Storage::new(Some(&archive_file), None).await?;
        let working_dir = self.working_dir.expect("TODO: generate temp dir");
        let indexer = Indexer::init(&self.database_url).await?;
        let regenerator = Regenerator::load(&working_dir, archive, indexer, store).await?;

        regenerator.run(self.start_height, self.stop_height).await
    }
}
