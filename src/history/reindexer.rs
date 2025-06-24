use flate2::read::GzDecoder;
use std::path::PathBuf;
use url::Url;

use std::fs::File;
use std::io::{copy, BufReader, BufWriter};

/// An compressed file archive containing a `penumbra-reindexer` sqlite3 database.
///
/// Requires a download URL so the archive can be fetched, and a checksum
/// so that the integrity can be verified.
pub struct ReindexerArchive {
    /// The chain id of the chain for which the archive provides history.
    pub chain_id: String,
    /// The URL from which the archive will be downloaded.
    pub download_url: Url,
    /// The SHA256 checksum for verifying the integrity of the archive post-download.
    pub checksum_sha256: String,
}

impl ReindexerArchive {
    /// Provide up comprehensive reindexer database for chain `penumbra-1`.
    pub fn for_penumbra_1() -> ReindexerArchive {
        let chain_id = "penumbra-1".to_owned();
        ReindexerArchive {
            download_url:
                "https://artifacts.plinfra.net/penumbra-1/reindexer-archive-height-5598447.sqlite.gz"
                    .try_into()
                    .expect("failed to parse reindexer archive url"),
            checksum_sha256: "ee430e6087f8864dbc08ceb3150cb2ee0363a53e7c79bfb00413f46c6f802f24"
                .to_owned(),
            chain_id: chain_id.clone(),
        }
    }

    /// Provide up comprehensive reindexer database for chain `penumbra-testnet-phobos-2`.
    pub fn for_penumbra_testnet_phobos_2() -> ReindexerArchive {
        let chain_id = "penumbra-testnet-phobos-2".to_owned();
        ReindexerArchive {
            download_url: "https://artifacts.plinfra.net/penumbra-testnet-phobos-2/reindexer_archive-height-3352529.sqlite".try_into().expect("failed to parse reindexer archive url"),
            checksum_sha256: "ab641c062aebfb389e3304fff7cbb6cdf45ce6094accbfab9cad76672e05fb51".to_owned(),
            chain_id: chain_id.clone(),
        }
    }

    /// Provide up comprehensive reindexer database for chain `penumbra-testnet-phobos-3`.
    pub fn for_penumbra_testnet_phobos_3() -> ReindexerArchive {
        let chain_id = "penumbra-testnet-phobos-3".to_owned();
        ReindexerArchive {
            download_url: "https://artifacts.plinfra.net/penumbra-testnet-phobos-3/reindexer_archive-height-997958.sqlite".try_into().expect("failed to parse reindexer archive url"),
            checksum_sha256: "e2443fd39cb1567febb40515ed847f19e57022a9d083056dc46116ecb81990d5".to_owned(),
            chain_id: chain_id.clone(),
        }
    }

    /// Take a gzipped sqlite3 db and decompress it.
    pub async fn extract(
        &self,
        compressed_file: &PathBuf,
        dest_file: &PathBuf,
    ) -> anyhow::Result<()> {
        tracing::debug!("decompressing gzipped asset");
        // Open input file with buffered reader
        let compressed_f = File::open(compressed_file)?;
        let r = BufReader::new(compressed_f);
        let gz = GzDecoder::new(r);

        // Open output file with buffered writer
        let dest_f = File::create(dest_file)?;
        let mut w = BufWriter::new(dest_f);

        // Stream copy from decoder to output file
        copy(&mut BufReader::new(gz), &mut w)?;

        Ok(())
    }

    /// Fetch the archive from the `download_url` and save it locally.
    pub async fn download(&self, dest_file: &PathBuf) -> anyhow::Result<()> {
        crate::history::download(&self.download_url, dest_file, &self.checksum_sha256).await?;
        Ok(())
    }

    /// Look up the node directory, by appending `node0`
    /// to the `network_dir`.
    pub fn node_dir(&self) -> PathBuf {
        crate::files::default_penumbra_home()
            .expect("failed to look up default penumbra home directory")
    }
}

impl TryFrom<String> for ReindexerArchive {
    type Error = anyhow::Error;
    fn try_from(s: String) -> anyhow::Result<ReindexerArchive> {
        // TODO refactor as enum
        if s == *"penumbra-1" {
            Ok(ReindexerArchive::for_penumbra_1())
        } else if s == *"penumbra-testnet-phobos-2" {
            Ok(ReindexerArchive::for_penumbra_testnet_phobos_2())
        } else if s == *"penumbra-testnet-phobos-3" {
            Ok(ReindexerArchive::for_penumbra_testnet_phobos_3())
        } else {
            anyhow::bail!("chain id '{}' is not supported", s);
        }
    }
}
