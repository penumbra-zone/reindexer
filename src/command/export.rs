use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

/// Export data from the archive.
#[derive(Debug, Parser)]
pub struct Export {
    #[command(subcommand)]
    command: ExportCommands,
}

// to allow for exporting blocks, etc. later
#[derive(Debug, Subcommand)]
enum ExportCommands {
    /// Export the genesis file for a specific height.
    Genesis(GenesisCmd),
}

/// Export the genesis file for a specific height.
#[derive(Debug, Args)]
pub struct GenesisCmd {
    /// The block height to export the genesis for.
    ///
    /// To query for available genesis blocks in the archive, run:
    /// `SELECT initial_height FROM geneses;`
    pub height: u64,

    /// Output file to write the genesis to.
    ///
    /// If not set, the genesis content will be printed to stdout.
    #[arg(short = 'o', long)]
    pub output_file: Option<PathBuf>,

    /// Path to the archive file to read from.
    #[arg(long)]
    pub archive_file: PathBuf,
}

impl Export {
    /// Run the export command.
    pub async fn run(self) -> Result<()> {
        match self.command {
            ExportCommands::Genesis(cmd) => cmd.run().await,
        }
    }
}

impl GenesisCmd {
    /// Run the genesis export command.
    pub async fn run(&self) -> Result<()> {
        // Initialize storage from the archive file.
        // We make no assumption about the chain id, and this will fail if the archive is empty,
        // which is what we want.
        let archive = crate::storage::Storage::new(Some(&self.archive_file), None).await?;

        let genesis = archive
            .get_genesis(self.height)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Genesis not found for height {}", self.height))?;

        // This could be done more efficiently by adding methods to the underlying type here.
        let encoded = genesis.encode()?;
        let genesis_value: serde_json::Value = serde_json::from_slice(&encoded)?;
        let genesis_json = serde_json::to_string_pretty(&genesis_value)?;

        if let Some(output_file) = &self.output_file {
            std::fs::write(output_file, genesis_json)
                .map_err(|e| anyhow::anyhow!("Failed to write genesis to file: {}", e))?;
        } else {
            println!("{}", genesis_json);
        }
        Ok(())
    }
}
