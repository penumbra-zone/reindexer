#![allow(dead_code)]
//! Common utilities for `penumbra-reindexer` integration tests.
//! Mostly handles downloading files and setting up `pd` node directories,
//! so that the reindexer can do its thing.

use anyhow::Context;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

// use penumbra_reindexer::history::NodeArchive;
use penumbra_reindexer::history::NodeArchiveSeries;

/// Manager to house filepaths for a test run of the reindexer tool.
pub struct ReindexerTestRunner {
    /// The path for storing local path to a directory for generating network data for a node.
    ///
    /// The actual `node0` directory will reside inside this dir, and the `pd` and `cometbft`
    /// directories inside of that.
    pub home: PathBuf,

    /// The chain-id for the network in question. Used to look up artifacts, e.g. genesis.
    pub chain_id: String,
}

impl Default for ReindexerTestRunner {
    fn default() -> ReindexerTestRunner {
        ReindexerTestRunner {
            home: penumbra_reindexer::files::default_reindexer_home()
                .expect("failed to find default reindexer dir"),
            chain_id: "penumbra-1".to_owned(),
        }
    }
}

impl ReindexerTestRunner {
    /// We must have a working CometBFT config in order to run the reindexer.
    /// We'll generate a network, then clobber its genesis with a downloaded one.
    pub async fn pd_init(&self) -> anyhow::Result<()> {
        let mut cmd = std::process::Command::new("pd");
        cmd.args(vec![
            "network",
            "--network-dir",
            self.archive_working_dir().to_str().unwrap(),
            "generate",
        ]);
        cmd.status()
            .context("failed to run 'pd network generate'; is pd available on PATH?")?;
        Ok(())
    }

    /// We need a real genesis file for the relevant network, in place within the CometBFT config.
    /// Generating an ad-hoc network will generate a random genesis, so this fn clobbers it.
    /// Accepts a `step` argument so that the appropriate genesis file for the chain state is
    /// fetched, which is important for the `archive` functionality.
    pub async fn fetch_genesis(&self, step: usize) -> anyhow::Result<()> {
        let genesis_url = format!(
            "https://artifacts.plinfra.net/{}/genesis-{}.json",
            self.chain_id, step
        );

        tracing::debug!(genesis_url, "fetching");
        let r = reqwest::get(genesis_url).await?.error_for_status()?;
        let genesis_content = r.text().await?;

        let genesis_filepath = self
            .node_dir()
            .join("cometbft")
            .join("config")
            .join("genesis.json");

        // Ensure pardirs are present
        if let Some(parent) = genesis_filepath.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Open file for writing (this will create it if it doesn't exist)
        let mut f = std::fs::File::create(&genesis_filepath)?;
        f.write_all(genesis_content.as_bytes())?;

        Ok(())
    }

    /// Sets up the integration test suite with required local archive data.
    pub async fn prepare_local_workbench(&self, step: usize) -> anyhow::Result<()> {
        // If we're starting a reindex, then we should clear out the dirs.
        if step == 0 {
            let d = self.archive_working_dir();
            if d.exists() {
                tracing::debug!("removing archive-working-dir {}", d.clone().display());
                std::fs::remove_dir_all(d)?;
            }
            // Only initialize the node0 directory when starting from scratch;
            // subsequent steps will overlay more node state via extraction
            // on top of this scaffoldingclobber the relevant node state dirs..
            self.pd_init().await?;
        }
        // Retrieve relevant archive
        let archive_list = NodeArchiveSeries::from_chain_id(&self.chain_id)?;
        let archive = &archive_list.archives[step];
        penumbra_reindexer::history::download(
            &archive.download_url,
            &self.archive_dir(),
            &archive.checksum_sha256,
        )
        .await?;

        archive
            .extract(&archive.reindexer_db_filepath(), &self.node_dir())
            .await?;
        // Clobber any pre-existing genesis with the appropriate one for the current phase.
        self.fetch_genesis(step).await?;
        Ok(())
    }

    /// Prebuilt `penumbra-reindexer` command.
    pub async fn cmd(&self) -> anyhow::Result<escargot::CargoRun> {
        tracing::debug!("building reindexer for tests");
        let cmd = escargot::CargoBuild::new()
            .bin("penumbra-reindexer")
            .current_release()
            .current_target()
            .run()?;
        Ok(cmd)
    }

    /// Obtain filepath to the sqlite3 database created by `penumbra-reindexer archive`.
    pub fn reindexer_db_filepath(&self) -> PathBuf {
        self.node_dir().join("reindexer_archive.bin")
    }

    /// Obtain filepath to the directory in which downloaded archives will be saved.
    pub fn archive_dir(&self) -> PathBuf {
        self.home.join(self.chain_id.clone())
    }

    /// Obtain filepath to the node state that will be read for extracting block info
    /// into an sqlite3 archive, via `penumbra-reindexer archive`.
    pub fn archive_working_dir(&self) -> PathBuf {
        self.archive_dir().join("archive-working-dir")
    }
    /// Look up the node directory, by appending `node0` to the `archive-working-dir`.
    pub fn node_dir(&self) -> PathBuf {
        self.archive_working_dir().join("node0")
    }

    /// Run `reindexer-archive` against the [node_dir].
    ///
    /// Will block until all available blocks have been archived, or else error.
    pub async fn create_archive(&self) -> anyhow::Result<()> {
        let _result = self
            .cmd()
            .await?
            .command()
            .arg("archive")
            .arg("--home")
            .arg(self.node_dir())
            .status()?;
        Ok(())
    }
}

#[tracing::instrument]
/// Reusable function to handle running `penumbra-reindexer` archive
/// for a given network. The `step` value indicates which serial
/// protocol compatibility period the `archive` run is in, indexed from 0
/// being the original network genesis.
pub async fn run_reindexer_archive_step(
    chain_id: &str,
    step: usize,
    expected_blocks: u64,
) -> anyhow::Result<()> {
    // Set up logging
    penumbra_reindexer::Opt::init_console_tracing();

    // Relevant paths:
    //
    //  ~/.local/share/penumbra-reindexer/
    //  ~/.local/share/penumbra-reindexer/penumbra-1/
    //  ~/.local/share/penumbra-reindexer/penumbra-1/archive-working-dir/
    //  ~/.local/share/penumbra-reindexer/penumbra-1/regen-working-dir/
    //  ~/.local/share/penumbra-reindexer/penumbra-1/*.tar.gz
    //  ~/.local/share/penumbra-reindexer/penumbra-1/*.sqlite.gz

    // Initialize testbed.
    let test_runner = ReindexerTestRunner {
        // The unique identifier of the network for which events should be ingested.
        chain_id: chain_id.to_owned(),
        // TODO permit overriding home dir for reindexer state
        ..Default::default()
    };

    test_runner.prepare_local_workbench(step).await?;

    // Look up vars to inject into check fns, due to refactor
    let reindexer_archive_filepath = test_runner.reindexer_db_filepath();

    tracing::info!("running reindexer archive step {}", step);
    test_runner.create_archive().await?;

    penumbra_reindexer::check::check_for_gaps_sqlite(&reindexer_archive_filepath).await?;
    penumbra_reindexer::check::check_num_blocks_sqlite(
        &reindexer_archive_filepath,
        expected_blocks,
    )
    .await?;
    penumbra_reindexer::check::check_num_geneses(&reindexer_archive_filepath, step).await?;

    Ok(())
}

/// Utility function to grab a sha256sum for a target file.
fn get_sha256sum<P: AsRef<Path>>(path: P) -> anyhow::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}
