use anyhow::Context;
use flate2::read::GzDecoder;
use std::io::Write;
use std::path::PathBuf;
use url::Url;

/// An compressed file archive containing historical node state.
///
/// The expected structure is quite strict: should be a `.tar.gz`
/// file, containing only `comebtft/data` and `pd/rocksdb` directories,
/// so that it can be extracted on top of an existing `node0` dir.
///
/// Requires a download URL so the archive can be fetched, and a checksum
/// so that the integrity can be verified.
pub struct NodeArchive {
    /// The chain id of the chain for which the archive provides history.
    pub chain_id: String,
    /// The URL from which the archive will be downloaded.
    pub download_url: Url,
    /// The SHA256 checksum for verifying the integrity of the archive post-download.
    pub checksum_sha256: String,
}

impl NodeArchive {
    /// Determine a reasonable fullpath for the archive locally,
    /// based on the `dest_dir` and `download_url`.
    pub fn dest_file(&self) -> anyhow::Result<PathBuf> {
        // TODO: reindexer dir in `~/.penumbra/network_data/node0/reindexer`?
        Ok(self
            .node_dir()
            .join(crate::history::basename_from_url(&self.download_url)?))
    }

    /// Take an archive, assumed to be in `.tar.gz` format, and decompress it
    /// across the `node0` directory for a Penumbra node.
    pub async fn extract(
        &self,
        archive_filepath: &PathBuf,
        dest_dir: &PathBuf,
    ) -> anyhow::Result<()> {
        let mut unpack_opts = std::fs::OpenOptions::new();
        unpack_opts.read(true);
        let f = unpack_opts
            .open(archive_filepath)
            .context("failed to open local archive for extraction")?;
        let tar = GzDecoder::new(f);
        let mut archive = tar::Archive::new(tar);
        archive
            .unpack(dest_dir)
            .context("failed to extract tar.gz archive")?;
        Ok(())
    }

    /// Fetch the archive from the `download_url` and save it locally.
    // TODO: make fn accept dest_file as arg
    pub async fn download(&self) -> anyhow::Result<()> {
        let dest_file = self.dest_file()?;
        crate::history::download(&self.download_url, &dest_file, &self.checksum_sha256).await?;
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

    /// Look up the node directory, by appending `node0`
    /// to the `network_dir`.
    pub fn node_dir(&self) -> PathBuf {
        crate::files::default_penumbra_home()
            .expect("failed to look up default penumbra home directory")
    }

    /// Obtain filepath to the sqlite3 database created by `penumbra-reindexer archive`.
    pub fn reindexer_db_filepath(&self) -> PathBuf {
        self.node_dir().join(crate::files::REINDEXER_FILE_NAME)
    }
}

/// A complete set of [NodeArchive]s, constituting
/// the entirety of blocks on a given chain. Assumes that
/// each archive contains all blocks for a specific protocol version,
/// with upgrade boundaries implied between each archive.
///
pub struct NodeArchiveSeries {
    /// The chain id of the chain represented by the archive series.
    chain_id: String,
    /// The historical archives representing chain state over multiple versions.
    pub archives: Vec<NodeArchive>,
}

impl NodeArchiveSeries {
    /// Parse a chain id to determine whether that network is supported
    /// by the reindexer test suite.
    pub fn from_chain_id(chain_id: &str) -> anyhow::Result<Self> {
        if chain_id == "penumbra-testnet-phobos-2" {
            let archives = Self::for_penumbra_testnet_phobos_2()?;
            Ok(archives)
        } else if chain_id == "penumbra-testnet-phobos-3" {
            let archives = Self::for_penumbra_testnet_phobos_3()?;
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
    pub fn for_penumbra_testnet_phobos_2() -> anyhow::Result<NodeArchiveSeries> {
        let chain_id = "penumbra-testnet-phobos-2".to_owned();
        let archives: Vec<NodeArchive> = vec![
            NodeArchive {
                download_url: "https://artifacts.plinfra.net/penumbra-testnet-phobos-2/penumbra-node-archive-height-1459800-pre-upgrade.tar.gz".try_into()?,
                checksum_sha256: "797e57b837acb3875b1b3948f89cdcb5446131a9eff73a40c77134550cf1b5f7".to_owned(),
                chain_id: chain_id.clone(),
            },

            NodeArchive {
                download_url: "https://artifacts.plinfra.net/penumbra-testnet-phobos-2/penumbra-node-archive-height-2358329-pre-upgrade.tar.gz".try_into()?,
                checksum_sha256: "5a079394e041f4280c3dc8e8ef871ca109ccb7147da1f9626c6c585cac5dc1bc".to_owned(),
                chain_id: chain_id.clone(),
            },

            NodeArchive {
                download_url: "https://artifacts.plinfra.net/penumbra-testnet-phobos-2/penumbra-node-archive-height-3280053.tar.gz".try_into()?,
                checksum_sha256: "e28f1a82845f4e2b3cd972ce8025a38b7e7e9fcbb3ee98efd766f984603988f4".to_owned(),
                chain_id: chain_id.clone(),
            },
        ];

        Ok(NodeArchiveSeries {
            chain_id: chain_id.to_owned(),
            archives,
        })
    }

    /// List all sequential node state archives required
    /// to reconstruct chain state for `penumbra-testnet-phobos-3`.
    pub fn for_penumbra_testnet_phobos_3() -> anyhow::Result<NodeArchiveSeries> {
        let chain_id = "penumbra-testnet-phobos-3".to_owned();
        let archives: Vec<NodeArchive> = vec![NodeArchive {
            download_url: "https://artifacts.plinfra.net/penumbra-testnet-phobos-3/penumbra-node-archive-height-368331.tar.gz".try_into()?,
            checksum_sha256: "53b449e99f0663f1c46dcb50f61f53eae6c2892eb740d41e6d0ed068c3eb62fc"
                .to_owned(),
            chain_id: chain_id.clone(),
        }];

        Ok(NodeArchiveSeries {
            chain_id: chain_id.to_owned(),
            archives,
        })
    }

    /// List all sequential node state archives required
    /// to reconstruct chain state for `penumbra-1`.
    pub fn for_penumbra_1() -> anyhow::Result<NodeArchiveSeries> {
        let chain_id = "penumbra-1".to_owned();
        let archives: Vec<NodeArchive> = vec![
            NodeArchive {
                download_url: "https://artifacts.plinfra.net/penumbra-1/penumbra-node-archive-height-501974-pre-upgrade.tar.gz".try_into()?,
                checksum_sha256: "146462ee5c01fba5d13923ef20cec4a121cc58da37d61f04ce7ee41328d2cbd0".to_owned(),
                chain_id: chain_id.clone(),

            },

            NodeArchive {
                download_url: "https://artifacts.plinfra.net/penumbra-1/penumbra-node-archive-height-2611800-pre-upgrade.tar.gz".try_into()?,
                checksum_sha256: "66e08e5d527607891136bddd9df768b8fd0ba8c7d57d0b6dc27976cc5a8fbbbb".to_owned(),
                chain_id: chain_id.clone(),
            },

            NodeArchive {
                download_url: "https://artifacts.plinfra.net/penumbra-1/penumbra-node-archive-height-4378762-pre-upgrade.tar.gz".try_into()?,
                checksum_sha256: "9840c4d0c93a928412fc55faa6edfe69faa19aac662cc133d6a45c64d1e0062c".to_owned(),
                chain_id: chain_id.clone(),
            },

            NodeArchive {
                download_url: "https://artifacts.plinfra.net/penumbra-1/penumbra-node-archive-height-4836782.tar.gz".try_into()?,
                checksum_sha256: "ffce4cfc5d783f0fc06645c4049b7affb8207b70e68012c9b33b46d108cdf996".to_owned(),
                chain_id: chain_id.clone(),
            },
        ];

        Ok(NodeArchiveSeries {
            chain_id: chain_id.to_owned(),
            archives,
        })
    }
}
