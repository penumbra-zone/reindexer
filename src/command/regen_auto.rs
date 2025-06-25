use std::path::PathBuf;
use std::process::Command;

use crate::penumbra::RegenerationPlan;

#[derive(clap::Parser)]
pub struct RegenAuto {
    /// The URL for the database where we should store the produced events.
    #[clap(long)]
    database_url: String,

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

    /// If set, use a given directory to store the working reindexing state.
    ///
    /// This allows resumption of reindexing, by reusing the directory.
    #[clap(long)]
    working_dir: Option<PathBuf>,

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

    /// If set, remove the working directory before starting regeneration.
    ///
    /// This ensures a clean state for regeneration but will remove any
    /// existing regeneration progress.
    #[clap(long)]
    clean: bool,
}

impl RegenAuto {
    pub async fn run(self) -> anyhow::Result<()> {
        // Determine chain_id - default to penumbra-1 if not specified
        let chain_id = self.chain_id.as_deref().unwrap_or("penumbra-1");

        // Handle clean option - remove working directory if it exists
        if self.clean {
            if let Some(ref working_dir) = self.working_dir {
                if working_dir.exists() {
                    tracing::info!(
                        "Removing existing working directory: {}",
                        working_dir.display()
                    );
                    std::fs::remove_dir_all(working_dir)?;
                } else {
                    tracing::info!("Working directory does not exist, nothing to clean");
                }
            } else {
                // If no working directory specified, we need to determine the default one
                // This matches the logic in the regen command
                let default_working_dir = crate::files::default_reindexer_home()?
                    .join(chain_id)
                    .join("regen-working-dir");

                if default_working_dir.exists() {
                    tracing::info!(
                        "Removing existing default working directory: {}",
                        default_working_dir.display()
                    );
                    std::fs::remove_dir_all(&default_working_dir)?;
                } else {
                    tracing::info!("Default working directory does not exist, nothing to clean");
                }
            }
        }

        // Get the regeneration plan for this chain
        let plan = RegenerationPlan::from_known_chain_id(chain_id).ok_or_else(|| {
            anyhow::anyhow!("no regeneration plan known for chain id '{}'", chain_id)
        })?;

        tracing::info!("Starting automatic regeneration for chain: {}", chain_id);
        tracing::info!("Found {} regeneration steps", plan.steps.len());

        // Get current executable path
        let current_exe = std::env::current_exe()?;

        // Extract stop heights from InitThenRunTo steps that have a last_block
        // The RegenerationPlan already handles the proper sequencing of migrate and run steps
        let mut regen_invocations = Vec::new();

        for (_, step) in &plan.steps {
            if let crate::penumbra::RegenerationStep::InitThenRunTo { last_block, .. } = step {
                regen_invocations.push(*last_block);
            }
        }

        tracing::info!(
            "Will execute {} regen commands with stop heights: {:?}",
            regen_invocations.len(),
            regen_invocations
        );

        for (i, stop_height) in regen_invocations.iter().enumerate() {
            let mut cmd = Command::new(&current_exe);
            cmd.arg("regen")
                .arg("--database-url")
                .arg(&self.database_url);

            if let Some(ref home) = self.home {
                cmd.arg("--home").arg(home);
            }

            if let Some(ref archive_file) = self.archive_file {
                cmd.arg("--archive-file").arg(archive_file);
            }

            if let Some(ref working_dir) = self.working_dir {
                cmd.arg("--working-dir").arg(working_dir);
            }

            if self.allow_existing_data {
                cmd.arg("--allow-existing-data");
            }

            if let Some(ref chain_id) = self.chain_id {
                cmd.arg("--chain-id").arg(chain_id);
            }

            // Add stop height if present
            if let Some(height) = stop_height {
                cmd.arg("--stop-height").arg(height.to_string());
                tracing::info!(
                    "Executing regen command {} of {} (stop-height: {})",
                    i + 1,
                    regen_invocations.len(),
                    height
                );
            } else {
                tracing::info!(
                    "Executing final regen command {} of {} (no stop-height)",
                    i + 1,
                    regen_invocations.len()
                );
            }

            let status = cmd.status()?;

            if !status.success() {
                return Err(anyhow::anyhow!(
                    "Regen command {} failed with exit code: {:?}",
                    i + 1,
                    status.code()
                ));
            }

            tracing::info!("Regen command {} completed successfully", i + 1);
        }

        tracing::info!("All regeneration commands completed successfully");
        Ok(())
    }
}
