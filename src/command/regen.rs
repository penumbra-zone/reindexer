use std::path::PathBuf;

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
    home: Option<String>,
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
    pub async fn run(self) -> anyhow::Result<()> {
        todo!()
    }
}
