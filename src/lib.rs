use anyhow;
use std::io::{stderr, IsTerminal as _};
use tracing_subscriber::EnvFilter;

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

    /// Initialize tracing for the console.
    pub fn init_console_tracing(&self) {
        tracing_subscriber::fmt()
            .with_ansi(stderr().is_terminal())
            .with_env_filter(
                EnvFilter::from_default_env()
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
