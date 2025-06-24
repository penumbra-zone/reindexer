use std::io::{stderr, IsTerminal as _};
use tracing_subscriber::EnvFilter;

pub mod check;
mod cometbft;
mod command;
pub mod files;
pub mod history;
mod indexer;
mod penumbra;
pub mod storage;
pub mod tendermint_compat;

/// This is a utility around re-indexing historical Penumbra events.
#[derive(clap::Parser)]
#[command(version)]
pub enum Opt {
    /// Create or add to our full historical archive.
    Archive(command::Archive),
    /// Regenerate an index of events, given a historical archive.
    Regen(command::Regen),
    /// Export data from the archive.
    Export(command::Export),
    /// Bootstrap initial config for the reindexer.
    Bootstrap(command::Bootstrap),
    /// Inspect local reindexer archive and perform healthchecks on it.
    Check(command::Check),
}

impl Opt {
    /// Run this command.
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Opt::Archive(x) => x.run().await,
            Opt::Regen(x) => x.run().await,
            Opt::Export(x) => x.run().await,
            Opt::Bootstrap(x) => x.run().await,
            Opt::Check(x) => x.run().await,
        }
    }

    /// Initialize tracing for the console.
    pub fn init_console_tracing() {
        tracing_subscriber::fmt()
            .with_ansi(stderr().is_terminal())
            .with_target(true)
            .with_env_filter(
                EnvFilter::try_from_default_env()
                    // Default to "info"-level logging.
                    .or_else(|_| EnvFilter::try_new("info"))
                    .expect("failed to initialize logging")
                    // Without explicitly disabling the `r1cs` target, the ZK proof implementations
                    // will spend an enormous amount of CPU and memory building useless tracing output.
                    .add_directive(
                        "r1cs=off"
                            .parse()
                            .expect("rics=off is a valid filter directive"),
                    ),
            )
            .with_writer(stderr)
            .init();
    }
}
