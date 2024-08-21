use anyhow::Context as _;
use cnidarium::Storage;
use penumbra_app::{PenumbraHost, SUBSTORE_PREFIXES};
use penumbra_ibc::component::HostInterface as _;
use std::path::Path;

/// A handle for working with a "Penumbra" chain.
///
/// This is the crux of our reindexing scheme, and is a way to easily access
/// and run the penumbra app logic for processing blocks and producing events.
pub struct Penumbra {
    storage: Storage,
}

impl Penumbra {
    pub async fn load(working_dir: &Path) -> anyhow::Result<Self> {
        let storage = Storage::load(working_dir.to_path_buf(), SUBSTORE_PREFIXES.to_vec())
            .await
            .context(format!(
                "Unable to initialize RocksDB storage in {}",
                working_dir.to_string_lossy()
            ))?;
        Ok(Self { storage })
    }

    pub async fn height(&self) -> Option<u64> {
        PenumbraHost::get_block_height(self.storage.latest_snapshot())
            .await
            .ok()
    }
}
