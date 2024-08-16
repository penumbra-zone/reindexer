use anyhow;

mod cometbft;
mod command;

/// This is a utility around re-indexing historical Penumbra events.
#[derive(clap::Parser)]
#[command(version)]
pub enum Opt {
    /// Create or add to our full historical archive of blocks.
    Archive(command::Archive),
}

impl Opt {
    /// Run this command.
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Opt::Archive(x) => x.run(),
        }
    }
}
