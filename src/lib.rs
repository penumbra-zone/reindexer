use std::io::{stderr, IsTerminal as _};
use tracing_subscriber::EnvFilter;

mod cometbft;
mod command;
mod files;
mod indexer;
mod penumbra;
mod storage;

/// This is a utility around re-indexing historical Penumbra events.
#[derive(clap::Parser)]
#[command(version)]
pub enum Opt {
    /// Create or add to our full historical archive.
    Archive(command::Archive),
    /// Regenerate an index of events, given a historical archive.
    Regen(command::Regen),
}

impl Opt {
    /// Run this command.
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Opt::Archive(x) => x.run().await,
            Opt::Regen(x) => x.run().await,
        }
    }

    /// Initialize tracing for the console.
    pub fn init_console_tracing(&self) {
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
