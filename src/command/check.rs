use std::path::PathBuf;

use crate::check;
use crate::files::archive_filepath_from_opts;

#[derive(clap::Parser)]
/// Inspect a local SQLite3 database for Penumbra Reindexer, and ensure it's validly
/// structured. Checks that a db contains all historical blocks, with no gaps,
/// which can occur when cometbft fails to index a block to its db.
pub struct Check {
    #[clap(long)]
    /// The home directory for the penumbra-reindexer.
    ///
    /// Downloaded large files will be stored within this directory.
    ///
    /// Defaults to `~/.local/share/penumbra-reindexer`.
    /// Can be overridden with --archive-file.
    home: Option<PathBuf>,

    /// Override the filepath for the sqlite3 database.
    /// Defaults to <HOME>/<CHAIN_ID>/reindexer-archive.sqlite
    #[clap(long)]
    archive_file: Option<PathBuf>,

    #[clap(long)]
    /// Perform healthchecks ensuring a specific chain id. Defaults to `penumbra-1` for mainnet.
    chain_id: Option<String>,

    /// Use a remote CometBFT RPC URL to fetch chain id from.
    ///
    /// Setting this option will pool a remote node for chain info,
    /// and initialize event archives based on the `chain_id` returned,
    /// if supported.
    #[clap(long)]
    remote_rpc: Option<String>,
}

impl Check {
    /// Create config dir, and fetch a remote ReindexerArchive.
    pub async fn run(self) -> anyhow::Result<()> {
        // Validate args
        if self.home.is_some() && self.archive_file.is_some() {
            anyhow::bail!("cannot use both --home and --archive-file options");
        }

        // Default to penumbra-1
        let chain_id = self.chain_id.unwrap_or(String::from("penumbra-1"));

        let archive_file =
            archive_filepath_from_opts(self.home, self.archive_file, Some(chain_id.clone()))?;

        if !archive_file.exists() {
            let msg = "archive file does not exist; specify one with `--archive-file`, or run `penumbra-reindexer bootstrap`";
            tracing::error!(archive_file = archive_file.display().to_string(), msg);
            anyhow::bail!(msg);
        }

        tracing::info!(
            "inspecting local db: {} ",
            &archive_file.as_path().as_os_str().to_str().unwrap(),
        );

        // Initialize reporting var
        let mut failed_checks = 0;

        // To figure out how many geneses should be in the archive,
        // we'll iterate over all upgrades and count 'em.:
        let x = crate::history::NodeArchiveSeries::from_chain_id(&chain_id)?;
        let expected_num_geneses = x.archives.len();

        match check::check_for_gaps_sqlite(&archive_file).await {
            Ok(_) => println!("✅ no gaps found found"),

            Err(_) => {
                println!("❌ found gaps of missing blocks");
                failed_checks += 1;
            }
        }

        // TODO check that chain id matches expectations

        match check::check_num_geneses(&archive_file, expected_num_geneses).await {
            Ok(_) => println!(
                "✅ found all {} expected genesis records",
                expected_num_geneses
            ),
            Err(_) => {
                println!(
                    "❌ genesis records are missing; expected {}",
                    expected_num_geneses
                );
                failed_checks += 1;
            }
        }
        if failed_checks == 0 {
            println!("💯 finished all checks, archive is valid");
        } else {
            println!("👎 failed {} checks", failed_checks);
            anyhow::bail!("failed {} checks", failed_checks);
        }

        Ok(())
    }
}
