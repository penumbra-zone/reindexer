#![allow(dead_code)]
//! Common utilities for `penumbra-reindexer` integration tests.
//! Mostly handles downloading files and setting up `pd` node directories,
//! so that the reindexer can do its thing.

use anyhow::Context;
use assert_cmd::Command;
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use sqlx::sqlite::SqlitePool;
use sqlx::{Error, FromRow, Row};
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use tokio_stream::StreamExt;
use url::Url;

/// The directory for initializing local node state, to support the reindexing process.
const NETWORK_DIR: &str = "test_data/ephemeral-storage/network";

/// The directory for storing downloaded compressed archives of historical node state.
const ARCHIVE_DIR: &str = "test_data/archives";

/// Manager to house filepaths for a test run of the reindexer tool.
pub struct ReindexerTestRunner {
    /// The local path to a directory for generating network data for a node.
    ///
    /// The actual `node0`` directory will reside inside this dir, and the `pd` and `cometbft`
    /// directories inside of that.
    pub network_dir: PathBuf,

    /// The chain-id for the network in question. Used to look up artifacts, e.g. genesis.
    pub chain_id: String,
}

impl ReindexerTestRunner {
    /// Initialize the necessary data from test fixtures to run the reindexer.
    pub async fn setup(&self) -> anyhow::Result<()> {
        self.pd_init().await?;
        Ok(())
    }

    /// We must have a working CometBFT config in order to run the reindexer.
    /// We'll generate a network, then clobber its genesis with a downloaded one.
    pub async fn pd_init(&self) -> anyhow::Result<()> {
        let mut cmd = Command::new("pd");
        cmd.args(vec![
            "network",
            "--network-dir",
            self.network_dir.to_str().unwrap(),
            "generate",
        ]);
        cmd.assert().success();
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
            .network_dir
            .join("node0")
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
            if self.network_dir.exists() {
                tracing::debug!(
                    "removing network_dir {}",
                    self.network_dir.clone().display()
                );
                std::fs::remove_dir_all(self.network_dir.clone())?;
            }
            self.setup().await?;
        }
        // Retrieve relevant archive
        let archive_list = HistoricalArchiveSeries::from_chain_id(&self.chain_id)?;
        let archive = &archive_list.archives[step];
        archive.download().await?;
        archive.extract(&self.node_dir()).await?;
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
        self.network_dir.join("node0").join("reindexer_archive.bin")
    }

    /// Query the sqlite3 database for total number of `genesis`,
    /// and expect that the total number is one greater than the current step.
    pub async fn check_num_geneses(&self, step: usize) -> anyhow::Result<()> {
        // Connect to the database
        let pool = SqlitePool::connect(self.reindexer_db_filepath().to_str().unwrap()).await?;
        let query = sqlx::query("SELECT COUNT(*) FROM geneses;");
        let count: u64 = query.fetch_one(&pool).await?.get(0);
        let expected: u64 = step as u64 + 1;
        assert_eq!(
            count, expected,
            "expected {} geneses, but found {}",
            expected, count
        );
        Ok(())
    }

    /// Query the sqlite3 database for any missing blocks, defined as `BlockGap`s,
    /// and fail if any are found.
    pub async fn check_for_gaps(&self) -> anyhow::Result<()> {
        // Connect to the database
        let pool = SqlitePool::connect(self.reindexer_db_filepath().to_str().unwrap()).await?;

        let query = sqlx::query_as::<_, BlockGap>(
            r#"
    WITH numbered_blocks AS (
        SELECT height,
               LEAD(height) OVER (ORDER BY height) as next_height
        FROM blocks
    )
    SELECT height + 1 as gap_start, next_height - 1 as gap_end
    FROM numbered_blocks
    WHERE next_height - height > 1
    "#,
        );
        let results = query.fetch_all(&pool).await?;

        // TODO: read fields to format an error message
        assert!(results.is_empty(), "found missing blocks in the sqlite3 db");
        Ok(())
    }

    /// Query the sqlite3 database for total number of known blocks.
    /// Fail if it doesn't match the expected number of blocks, or
    /// 1 less than the expected number. The tolerance is to acknowledge
    /// that the sqlite3 db can be 1 block behind the local node state.
    pub async fn check_num_blocks(&self, expected: u64) -> anyhow::Result<u64> {
        // Connect to the database
        let pool = SqlitePool::connect(self.reindexer_db_filepath().to_str().unwrap()).await?;
        let query = sqlx::query("SELECT COUNT(*) FROM blocks");
        let count: u64 = query.fetch_one(&pool).await?.get(0);
        assert!(
            [expected, expected - 1].contains(&count),
            "archived blocks count looks wrong; expected: {}, found {}",
            count,
            expected
        );
        Ok(count)
    }

    /// Look up the node directory, by appending `node0`
    /// to the `network_dir`.
    pub fn node_dir(&self) -> PathBuf {
        self.network_dir.join("node0")
    }

    /// Run `reindexer-archive` against the [node_dir].
    ///
    /// Will block until all available blocks have been archived, or else error.
    pub async fn archive(&self) -> anyhow::Result<()> {
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

/// Set up [tracing_subscriber], so that tests can emit logging information.
pub fn init_tracing() {
    // TODO this is copy/pasted from `src/lib.rs`, reuse.
    use std::io::{stderr, IsTerminal as _};
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_ansi(stderr().is_terminal())
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

#[derive(Debug)]
/// Representation of a range of missing blocks.
///
/// Used to check that created databases are complete, in that they're fully contiguous:
/// no blocks are absent from the range specified.
pub struct BlockGap {
    /// The first block in the range.
    gap_start: i64,
    /// The last block in the range.
    gap_end: i64,
}

/// Ensure that we can query the sqlite3 and receive BlockGap results.
impl<'r> FromRow<'r, sqlx::sqlite::SqliteRow> for BlockGap {
    fn from_row(row: &'r sqlx::sqlite::SqliteRow) -> Result<Self, Error> {
        Ok(BlockGap {
            gap_start: row.try_get("start_block")?, // if column is named differently
            gap_end: row.try_get("end_block")?,
        })
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
    crate::common::init_tracing();

    // Initialize testbed.
    let test_runner = ReindexerTestRunner {
        chain_id: chain_id.to_owned(),
        // Append chain id to network dir to disambiguate local paths.
        network_dir: PathBuf::from(NETWORK_DIR).join(chain_id),
    };

    test_runner.prepare_local_workbench(step).await?;

    tracing::info!("running reindexer archive step {}", step);
    test_runner.archive().await?;
    test_runner.check_for_gaps().await?;
    test_runner.check_num_blocks(expected_blocks).await?;
    test_runner.check_num_geneses(step).await?;
    Ok(())
}

/// `pd/rocksdb` and `cometbft/data` directories for a
/// A complete set of node state archives, constituting
/// node, representing each protocol version, segmented
/// on upgrade boundaries.
pub struct HistoricalArchiveSeries {
    chain_id: String,
    pub archives: Vec<HistoricalArchive>,
}

/// A single archive containing historical node state.
/// Requires a download URL so the archive can be fetched.
/// The expected structure is quite strict: should be a `.tar.gz`
/// file, containing only `comebtft/data` and `pd/rocksdb` directories,
/// so that it can be extracted on top of an existing `node0` dir.
pub struct HistoricalArchive {
    chain_id: String,
    download_url: Url,
    dest_dir: PathBuf,
    checksum_sha256: String,
}

impl HistoricalArchive {
    /// Determine a reasonable filename for the archive, based on the URL.
    pub fn basename(&self) -> anyhow::Result<String> {
        let basename = self
            .download_url
            .path_segments()
            .ok_or_else(|| anyhow::anyhow!("URL has no path segments"))?
            .last()
            .ok_or_else(|| anyhow::anyhow!("URL has no basename"))?;

        Ok(basename.to_string())
    }
    /// Determine a reasonable fullpath for the archive locally,
    /// based on the `dest_dir` and `download_url`.
    pub fn dest_file(&self) -> anyhow::Result<PathBuf> {
        Ok(self.dest_dir.join(self.basename()?))
    }
    /// Take an archive, assumed to be in `.tar.gz` format, and decompress it
    /// across the `node0` directory for a Penumbra node.
    pub async fn extract(&self, node_dir: &PathBuf) -> anyhow::Result<()> {
        let mut unpack_opts = std::fs::OpenOptions::new();
        unpack_opts.read(true);
        let f = unpack_opts
            .open(self.dest_file()?)
            .context("failed to open local archive for extraction")?;
        let tar = GzDecoder::new(f);
        let mut archive = tar::Archive::new(tar);
        archive
            .unpack(node_dir)
            .context("failed to extract tar.gz archive")?;
        Ok(())
    }
    /// Fetch the archive from the `download_url` and save it locally.
    pub async fn download(&self) -> anyhow::Result<()> {
        if self.dest_file()?.exists() {
            let existing_hash = get_sha256sum(&self.dest_file()?)?;
            if existing_hash == self.checksum_sha256 {
                tracing::debug!(
                    "archive already exists with correct hash: {} {}",
                    self.dest_file()?.display(),
                    self.checksum_sha256,
                );
                return Ok(());
            } else {
                tracing::warn!(
                    "archive failed to verify via checksum: {} ; expected {}, got {}",
                    self.dest_file()?.display(),
                    self.checksum_sha256,
                    existing_hash,
                );
                tracing::warn!("re-downloading archive: {}", self.dest_file()?.display());
            }
        }
        // Create all parent directories
        if let Some(parent) = self.dest_file()?.parent() {
            tracing::debug!(?parent, "creating parent directory prior to downloading");
            std::fs::create_dir_all(parent)?;
        }
        tracing::info!(%self.download_url, "downloading archive");
        let response = reqwest::get(self.download_url.clone()).await?;
        let mut download_opts = std::fs::OpenOptions::new();
        // We set truncate to true because we bailed above if checksum matched.
        //
        download_opts.create(true).write(true).truncate(true);
        let mut f = download_opts
            .open(&self.dest_file()?)
            .context("failed to open dest filepath for downloading archive")?;

        // Download via stream, as the file is too large to shove into RAM.
        let mut stream = response.bytes_stream();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            f.write_all(&chunk)?;
        }
        f.flush()?;

        let actual_checksum = get_sha256sum(&self.dest_file()?)?;
        if actual_checksum != self.checksum_sha256 {
            let msg = format!(
                "archive failed to verify via checksum: {} ; expected {}, got {}",
                self.dest_file()?.display(),
                self.checksum_sha256,
                actual_checksum,
            );
            tracing::error!(msg);
            anyhow::bail!(msg);
        }
        tracing::info!("download complete: {}", self.dest_file()?.display());

        Ok(())
    }
}

/// Utility function to grab a sha256sum for a target file.
fn get_sha256sum<P: AsRef<Path>>(path: P) -> anyhow::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

impl HistoricalArchiveSeries {
    /// Parse a chain id to determine whether that network is supported
    /// by the reindexer test suite.
    fn from_chain_id(chain_id: &str) -> anyhow::Result<Self> {
        if chain_id == "penumbra-testnet-phobos-2" {
            let archives = Self::for_penumbra_testnet_phobos_2()?;
            Ok(archives)
        } else if chain_id == "penumbra-1" {
            let archives = Self::for_penumbra_1()?;
            Ok(archives)
        } else {
            anyhow::bail!("chain id '{}' not supported", chain_id);
        }
    }

    /// List all sequential node state archives required
    /// to reconstruct chain state for `penumbra-testnet-phobos-2`.
    pub fn for_penumbra_testnet_phobos_2() -> anyhow::Result<HistoricalArchiveSeries> {
        let chain_id = "penumbra-testnet-phobos-2".to_owned();
        let dest_dir = PathBuf::from(format!(
            "{}/{}/{}",
            env!("CARGO_MANIFEST_DIR"),
            ARCHIVE_DIR,
            chain_id,
        ));
        let archives: Vec<HistoricalArchive> = vec![
            HistoricalArchive {
                download_url: "https://artifacts.plinfra.net/penumbra-testnet-phobos-2/penumbra-node-archive-height-1459800-pre-upgrade.tar.gz".try_into()?,
                checksum_sha256: "797e57b837acb3875b1b3948f89cdcb5446131a9eff73a40c77134550cf1b5f7".to_owned(),
                chain_id: chain_id.clone(),
                dest_dir: dest_dir.clone(),

            },

            HistoricalArchive {
                download_url: "https://artifacts.plinfra.net/penumbra-testnet-phobos-2/penumbra-node-archive-height-2358329-pre-upgrade.tar.gz".try_into()?,
                checksum_sha256: "5a079394e041f4280c3dc8e8ef871ca109ccb7147da1f9626c6c585cac5dc1bc".to_owned(),
                chain_id: chain_id.clone(),
                dest_dir: dest_dir.clone(),
            },

            HistoricalArchive {
                download_url: "https://artifacts.plinfra.net/penumbra-testnet-phobos-2/penumbra-node-archive-height-3280053.tar.gz".try_into()?,
                checksum_sha256: "e28f1a82845f4e2b3cd972ce8025a38b7e7e9fcbb3ee98efd766f984603988f4".to_owned(),
                chain_id: chain_id.clone(),
                dest_dir: dest_dir.clone(),
            },
        ];

        Ok(HistoricalArchiveSeries {
            chain_id: chain_id.to_owned(),
            archives,
        })
    }

    /// List all sequential node state archives required
    /// to reconstruct chain state for `penumbra-1`.
    pub fn for_penumbra_1() -> anyhow::Result<HistoricalArchiveSeries> {
        let chain_id = "penumbra-1".to_owned();
        let dest_dir = PathBuf::from(format!(
            "{}/{}/{}",
            env!("CARGO_MANIFEST_DIR"),
            ARCHIVE_DIR,
            chain_id,
        ));

        let archives: Vec<HistoricalArchive> = vec![
            HistoricalArchive {
                download_url: "https://artifacts.plinfra.net/penumbra-1/penumbra-node-archive-height-501974-pre-upgrade.tar.gz".try_into()?,
                checksum_sha256: "146462ee5c01fba5d13923ef20cec4a121cc58da37d61f04ce7ee41328d2cbd0".to_owned(),
                chain_id: chain_id.clone(),
                dest_dir: dest_dir.clone(),

            },

            HistoricalArchive {
                download_url: "https://artifacts.plinfra.net/penumbra-1/penumbra-node-archive-height-2611800-pre-upgrade.tar.gz".try_into()?,
                checksum_sha256: "66e08e5d527607891136bddd9df768b8fd0ba8c7d57d0b6dc27976cc5a8fbbbb".to_owned(),
                chain_id: chain_id.clone(),
                dest_dir: dest_dir.clone(),
            },

            HistoricalArchive {
                download_url: "https://artifacts.plinfra.net/penumbra-1/penumbra-node-archive-height-4027443.tar.gz".try_into()?,
                checksum_sha256: "cfb93391ae348275b221bb1811d59833b4bc2854c92c234fe266506b4a6b7c71".to_owned(),
                chain_id: chain_id.clone(),
                dest_dir: dest_dir.clone(),
            },
        ];

        Ok(HistoricalArchiveSeries {
            chain_id: chain_id.to_owned(),
            archives,
        })
    }
}
