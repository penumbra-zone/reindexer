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

#[derive(Clone, Copy, PartialEq)]
enum Version {
    V0o79,
    V0o80,
}

#[derive(Clone, Copy, PartialEq)]
enum RegenerationStep {
    Migrate {
        from: Version,
        to: Version,
    },
    InitThenRunTo {
        genesis_height: u64,
        version: Version,
        last_block: Option<u64>,
    },
    RunTo {
        version: Version,
        last_block: Option<u64>,
    },
}

impl RegenerationStep {
    /// Attempt to construct a new regeneration step with a different starting point.
    ///
    /// For some steps, they still need to be run, albeit in a modified form,
    /// if we've already processed blocks up to a given height.
    pub fn with_moved_start(self, start: u64) -> Option<Self> {
        match self {
            // A migration has to be run at the exact point.
            RegenerationStep::Migrate { .. } => None,
            RegenerationStep::InitThenRunTo {
                genesis_height,
                version,
                last_block,
            } => {
                // To perform the genesis again, we need to not have reached post-genesis height.
                if genesis_height > start {
                    Some(self)
                } else {
                    // In this case, we have no genesis to run, but may still need to process
                    // blocks.
                    RegenerationStep::RunTo {
                        version,
                        last_block,
                    }
                    .with_moved_start(start)
                }
            }
            RegenerationStep::RunTo { last_block, .. } => {
                match last_block {
                    // If this runs forever, then the block we've reached doesn't matter.
                    None => Some(self),
                    // If this stops at a certain block, we need to not have reached it yet.
                    Some(x) if x > start => Some(self),
                    _ => None,
                }
            }
        }
    }

    /// Create a new step which will potentially at the last block we want to index.
    pub fn with_moved_stop(self, stop: u64) -> Self {
        fn move_last_block(last_block: Option<u64>, stop: u64) -> Option<u64> {
            match last_block {
                None => Some(stop),
                Some(x) if x > stop => Some(stop),
                Some(x) => Some(x),
            }
        }

        match self {
            RegenerationStep::InitThenRunTo {
                genesis_height,
                version,
                last_block,
            } => RegenerationStep::InitThenRunTo {
                genesis_height,
                version,
                last_block: move_last_block(last_block, stop),
            },
            RegenerationStep::RunTo {
                version,
                last_block,
            } => RegenerationStep::RunTo {
                version,
                last_block: move_last_block(last_block, stop),
            },
            // All other steps are not modified
            _ => self,
        }
    }
}

/// Represents a series of steps to regenerate events.
///
/// This is useful to provide a concise overview of what we intend to regenerate and how,
/// and to allow for easy modifications if we need to stop early or to start at a different height.
///
/// This also makes the resulting logic in terms of creating and destroying penumbra applications
/// easier, because we know the given lifecycle of a version of the penumbra logic.
struct RegenerationPlan {
    pub steps: Vec<(u64, RegenerationStep)>,
}

impl RegenerationPlan {
    /// Truncate a regeneration plan, removing unnecessary actions for a given set of bounds.
    ///
    /// If present, `start` indicates the block we'll have *already* indexed.
    ///
    /// If present, `stop` indicates the last block we want to index.
    pub fn truncate(self, start: Option<u64>, stop: Option<u64>) -> Self {
        // For our logic, we can treat no start as 0
        let start = start.unwrap_or(0u64);
        let steps = self
            .steps
            .into_iter()
            // Keep all steps which start at the block we've reached,
            // but transform the other steps
            .filter_map(|(step_start, step)| {
                if start <= step_start {
                    Some((step_start, step))
                } else {
                    step.with_moved_start(start).map(|x| (step_start, x))
                }
            })
            // Keep all steps which don't start after the last block we want to index,
            // but potentially shorten their execution
            .filter_map(|(step_start, step)| {
                if let Some(stop) = stop {
                    if step_start >= stop {
                        None
                    } else {
                        Some((step_start, step.with_moved_stop(stop)))
                    }
                } else {
                    Some((step_start, step))
                }
            })
            .collect();
        Self { steps }
    }

    /// The regeneration plan for penumbra_1 chain.
    pub fn penumbra_1() -> Self {
        use RegenerationStep::*;
        use Version::*;

        Self {
            steps: vec![
                (
                    0,
                    InitThenRunTo {
                        genesis_height: 1,
                        version: V0o79,
                        last_block: Some(501974),
                    },
                ),
                (
                    501974,
                    Migrate {
                        from: V0o79,
                        to: V0o80,
                    },
                ),
                (
                    501974,
                    InitThenRunTo {
                        genesis_height: 501975,
                        version: V0o80,
                        last_block: None,
                    },
                ),
            ],
        }
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
        // TODO: update this logic to instead iterate over all penumbra versions.
        Ok(
            PenumbraHost::get_block_height(self.storage.latest_snapshot())
                .await
                .ok(),
        )
    }

    async fn run_from(&self, start: Option<u64>, stop: Option<u64>) -> anyhow::Result<()> {
        todo!()
    }
}
