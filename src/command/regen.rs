use std::path::PathBuf;

use crate::files::{default_penumbra_home, REINDEXER_FILE_NAME};

#[derive(clap::Parser)]
pub struct Regen {
    /// The URL for the database where we should store the produced events.
    #[clap(long)]
    database_url: String,
    /// A home directory to read penumbra data from.
    ///
    /// The equivalent of pd's --network-dir.
    ///
    /// This will be overriden by --archive-file.
    ///
    /// We expect there to be a ./reindexer_archive.bin file in this directory otherwise.
    #[clap(long)]
    home: Option<PathBuf>,
    /// If set, use this file to read the archive file from directory, ignoring other options.
    #[clap(long)]
    archive_file: Option<PathBuf>,
    /// If set, index events up to and including this height.
    ///
    /// For example, if this is set to 2, only events in blocks 1, 2 will be indexed.
    #[clap(long)]
    stop_height: Option<u64>,
    /// If set, use a given directory to store the working reindexing state.
    ///
    /// This allow resumption of reindexing, by reusing the directory.
    #[clap(long)]
    working_dir: Option<PathBuf>,
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
        tracing::info!(?archive_file, ?self.stop_height, "regenerating index");
        todo!()
    }
}
