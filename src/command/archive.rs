use anyhow::{anyhow, Context};
use std::path::{Path, PathBuf};

use crate::cometbft;

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
    /// If set, use this file for archive data, instead of HOME/archive.bin.
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
            (_, Some(x)) => Path::new(x).to_path_buf(),
            (Some(x), None) => Path::new(x).join("cometbft"),
            (None, None) => home_dir()
                .context("create a home directory, or manually specify a cometbft path")?
                .join(".penumbra/network_data/node0/cometbft"),
        };
        Ok(out)
    }

    /// Create or add to our full historical archive of blocks.
    pub async fn run(self) -> anyhow::Result<()> {
        let mut store = cometbft::Store::new(&self.cometbft_dir()?)?;
        let first_height = store.first_height().unwrap();
        let last_height = store.last_height();
        tracing::info!(first_height, last_height);
        println!("{:X?}", store.block_by_height(first_height));
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
