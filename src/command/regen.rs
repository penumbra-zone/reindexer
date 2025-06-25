use std::path::PathBuf;

use crate::{
    cometbft::{RemoteStore, Store},
    indexer::{Indexer, IndexerOpts},
    penumbra::Regenerator,
    storage::Storage,
};

#[derive(clap::Parser)]
pub struct Regen {
    /// The URL for the database where we should store the produced events.
    #[clap(long)]
    database_url: String,
    /// The directory containing pd and cometbft data for a full node.
    ///
    /// In this directory we expect there to be:
    ///
    /// - ./cometbft/config/config.toml, for reading cometbft configuration
    /// - ./cometbft/data/, for reading historical blocks
    ///
    /// Defaults to `~/.penumbra/network_data/node0`, the same default used for `pd start`.
    ///
    /// The node state will be read from this directory, and saved inside
    /// an sqlite3 database at ~/.local/share/penumbra-reindexer/<CHAIN_ID>/reindexer-archive.sqlite.
    #[clap(long)]
    node_home: Option<PathBuf>,

    /// The home directory for the penumbra-reindexer.
    ///
    /// Downloaded large files will be stored within this directory.
    ///
    /// Defaults to `~/.local/share/penumbra-reindexer`.
    /// Can be overridden with --archive-file.
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

    /// If set, allows the indexing database to have data.
    ///
    /// This will make the indexer add any data that's not there
    /// (e.g. blocks that are missing, etc.). The indexer will not overwrite existing
    /// data, and simply skip indexing anything that would do so.
    #[clap(long)]
    allow_existing_data: bool,

    #[clap(long)]
    /// Specify a network for which events should be regenerated.
    ///
    /// The sqlite3 database must already have events in it from this chain.
    /// If the chain id in the sqlite3 database doesn't match this value,
    /// the program will exit with an error.
    chain_id: Option<String>,
}

impl Regen {
    /// Resolve the path of the archive file
    fn archive_file(&self) -> anyhow::Result<PathBuf> {
        crate::files::archive_filepath_from_opts(
            self.home.clone(),
            self.archive_file.clone(),
            self.chain_id.clone(),
        )
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let archive_file = self.archive_file()?;

        let store: Option<Box<dyn Store>> = match self.follow {
            None => None,
            Some(x) => Some(Box::new(RemoteStore::new(x))),
        };

        let chain_id = match store.as_ref() {
            None => {
                tracing::info!("no chain_id specified, defaulting to 'penumbra-1'");
                String::from("penumbra-1")
            }
            Some(store) => {
                let genesis = store.get_genesis().await?;
                genesis.chain_id()
            }
        };

        let archive = Storage::new(Some(&archive_file), Some(&chain_id)).await?;
        let working_dir = match self.working_dir {
            Some(d) => d,
            None => {
                let p = crate::files::default_reindexer_home()?
                    .join(chain_id)
                    .join("regen-working-dir");

                tracing::debug!(
                    "working dir not specified, defaulting to {}",
                    p.display().to_string()
                );
                p
            }
        };

        let indexer_opts = IndexerOpts {
            allow_existing_data: self.allow_existing_data,
        };
        let indexer = Indexer::init(&self.database_url, indexer_opts).await?;
        let regenerator = Regenerator::load(&working_dir, archive, indexer, store).await?;

        regenerator.run(self.start_height, self.stop_height).await
    }
}
