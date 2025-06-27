use std::path::PathBuf;
use std::process::Command;

use crate::{
    cometbft::{RemoteStore, Store},
    indexer::{Indexer, IndexerOpts},
    penumbra::{RegenerationPlan, Regenerator},
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
    /// If stop height is not set, the reindexer will build a regeneration plan
    /// according to the chain id, and handle upgrade boundaries for known chains.
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

    /// If set, remove the working directory before starting regeneration.
    ///
    /// This ensures a clean state for regeneration but will remove any
    /// existing regeneration progress.
    #[clap(long)]
    clean: bool,
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

    /// The CLI entrypoint, gating logic between:
    ///
    ///   1. full-auto mode
    ///   2. step mode
    ///
    /// The full-auto mode will look up a RegenerationPlan by chain id,
    /// and call out to step mode with the `--stop-height` flag specified.
    pub async fn run(self) -> anyhow::Result<()> {
        // If stop_height is provided, run in step mode
        if self.stop_height.is_some() {
            self.run_step_mode().await
        } else {
            self.run_auto_mode().await
        }
    }

    /// Handle regeneration only up to a specific height. Typically the stop-height indicates
    /// an upgrade boundary; for legacy reasons, the upstream pd code will `sys::exit` when
    /// encountering an upgrade boundary. We don't want that exit call to exit the reindexer
    /// process, thus proactively exiting when a specific stop height is reached.
    async fn run_step_mode(self) -> anyhow::Result<()> {
        let archive_file = self.archive_file()?;

        let store: Option<Box<dyn Store>> = match self.follow {
            None => None,
            Some(x) => Some(Box::new(RemoteStore::new(x))),
        };

        let chain_id = match store.as_ref() {
            None => self.chain_id.unwrap_or_else(|| {
                tracing::info!("no chain_id specified, defaulting to 'penumbra-1'");
                String::from("penumbra-1")
            }),
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
                    .join(&chain_id)
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

    /// Look up a RegenerationPlan for the chosen chain id, and call out to the step-mode function
    /// of regeneration serially, supplying appropriate stop heights that match historical upgrade
    /// boundaries. Allows a single invocation to regenerate events for a target chain, without
    /// a wrapper script to set `--stop-height` on every upgrade boundary.
    async fn run_auto_mode(self) -> anyhow::Result<()> {
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
