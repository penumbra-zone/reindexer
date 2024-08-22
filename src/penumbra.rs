use anyhow::Context as _;
use cnidarium::Storage;
use penumbra_app::{PenumbraHost, SUBSTORE_PREFIXES};
use penumbra_ibc::component::HostInterface as _;
use std::path::Path;

use crate::{indexer::Indexer, storage::Storage as Archive};

/// A handle for working with a "Penumbra" chain.
///
/// This is the crux of our reindexing scheme, and is a way to easily access
/// and run the penumbra app logic for processing blocks and producing events.
struct Penumbra {
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

/// A utility to regenerate a raw events database given an archive of Penumbra data.
///
/// https://www.imdb.com/title/tt0089885/
pub struct Regenerator {
    storage: Storage,
    archive: Archive,
    indexer: Indexer,
}

impl Regenerator {
    /// Load up a regenerator.
    pub async fn load(
        working_dir: &Path,
        archive: Archive,
        indexer: Indexer,
    ) -> anyhow::Result<Self> {
        todo!()
    }

    pub async fn run(&self, stop_height: Option<u64>) -> anyhow::Result<()> {
        // Basic idea:
        //  1. Figure out the current height we've indexed to.
        //  2. Try and advance, height by height, until the stop height.
        //  2.1 If a migration needs to be run before this height, run it.
        //  2.2 If the chain needs to be initialized at this height, initialize it.
        //  2.3 Retrieve the block that needs to fed in, and then index the resulting events.
        //
        // It's regeneratin' time.
        let current_height = self.find_current_height().await?;
        self.run_from(current_height, stop_height).await
    }

    async fn find_current_height(&self) -> anyhow::Result<Option<u64>> {
        todo!()
    }

    async fn run_from(&self, start: Option<u64>, stop: Option<u64>) -> anyhow::Result<()> {
        todo!()
    }
}

/// Regenerate the index of raw events.
///
/// This uses:
///   - a working directory to hold the state of the Penumbra application during regeneration,
///   - an archive of blocks and genesis data to feed regeneration,
///   - an indexer to act as a sink of events.
pub async fn regenerate(
    working_dir: &Path,
    archive: &Archive,
    indexer: &Indexer,
    stop_height: Option<u64>,
) -> anyhow::Result<()> {
    // Basic idea:
    //  1. Figure out the current height we've indexed to.
    //  2. Try and advance, height by height, until the stop height.
    //  2.1 If a migration needs to be run before this height, run it.
    //  2.2 If the chain needs to be initialized at this height, initialize it.
    //  2.3 Retrieve the block that needs to fed in, and then index the resulting events.
    todo!()
}
