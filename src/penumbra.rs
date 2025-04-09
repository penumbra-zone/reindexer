use crate::cometbft::Store;
use crate::tendermint_compat::{BeginBlock, Block, DeliverTx, EndBlock, Event, ResponseDeliverTx};
use crate::{cometbft::Genesis, indexer::Indexer, storage::Storage as Archive};
use anyhow::anyhow;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_stream::StreamExt as _;

mod v0o79;
mod v0o80;
mod v1;
mod v2;

#[async_trait]
/// Representation of the Penumbra state machine from the perspective of CometBFT.
trait Penumbra {
    /// Drop the storage handle, permitting writes from other handles.
    async fn release(self: Box<Self>);
    /// Genesis event. At block 0, this is a full genesis, but Penumbra networks
    /// will have snapshot types for genesis at every upgrade boundary,
    /// where the protocol changes.
    async fn genesis(&mut self, genesis: Genesis) -> anyhow::Result<()>;
    async fn metadata(&self) -> anyhow::Result<(u64, String)>;
    async fn begin_block(&mut self, req: &BeginBlock) -> Vec<Event>;
    async fn deliver_tx(&mut self, req: &DeliverTx) -> anyhow::Result<Vec<Event>>;
    async fn end_block(&mut self, req: &EndBlock) -> Vec<Event>;
    async fn commit(&mut self) -> anyhow::Result<()>;
}

type APenumbra = Box<dyn Penumbra>;

async fn make_a_penumbra(version: Version, working_dir: &Path) -> anyhow::Result<APenumbra> {
    match version {
        Version::V0o79 => Ok(Box::new(v0o79::Penumbra::load(working_dir).await?)),
        Version::V0o80 => Ok(Box::new(v0o80::Penumbra::load(working_dir).await?)),
        Version::V1 => Ok(Box::new(v1::Penumbra::load(working_dir).await?)),
        Version::V2 => Ok(Box::new(v2::Penumbra::load(working_dir).await?)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Version {
    V0o79,
    V0o80,
    V1,
    V2,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RegenerationStep {
    /// Represents a migration, as would be performed by `pd migrate`,
    /// so munge node state on a planned upgrade boundary.
    Migrate { from: Version, to: Version },
    InitThenRunTo {
        /// The `genesis_height` is the block at which the chain will resume, post-upgrade.
        /// For reference, this is the same block specified in an `upgrade-plan` proposal,
        /// as the `upgradePlan.height` field.
        genesis_height: u64,
        version: Version,
        /// The `last_block` is the block immediately preceding a planned chain upgrade.
        /// For reference, this is the block specified in an `upgrade-plan` proposal,
        /// minus 1. For chains with no known upgrade, this should be `None`.
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

    /// Check the feasability of this step against an archive.
    ///
    /// Will return `Ok(Err(_))` if this step is guaranteed to fail (at that starting point).
    pub async fn check_against_archive(
        &self,
        start: u64,
        archive: &Archive,
    ) -> anyhow::Result<anyhow::Result<()>> {
        match self {
            RegenerationStep::Migrate { .. } => Ok(Ok(())),
            // For this to work, we need to be able to fetch the genesis,
            // and then to be able to do a "run to" from the start to the potential last block.
            RegenerationStep::InitThenRunTo {
                genesis_height,
                last_block,
                ..
            } => {
                if !archive.genesis_does_exist(*genesis_height).await? {
                    return Err(anyhow!(
                        "genesis at height {} does not exist",
                        genesis_height,
                    ));
                }
                if start > 0 && !archive.block_does_exist(start).await? {
                    return Err(anyhow!("missing block at height {}", start));
                }
                if let Some(block) = last_block {
                    if !archive.block_does_exist(*block).await? {
                        return Err(anyhow!("missing block at height {}", block));
                    }
                }
                Ok(Ok(()))
            }
            // To run from a start block to a last block, both blocks should exist.
            RegenerationStep::RunTo { last_block, .. } => {
                if start > 0 && !archive.block_does_exist(start).await? {
                    return Err(anyhow!("missing block at height {}", start));
                }
                if let Some(block) = last_block {
                    if !archive.block_does_exist(*block).await? {
                        return Err(anyhow!("missing block at height {}", block));
                    }
                }
                Ok(Ok(()))
            }
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
#[derive(Debug)]
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
                    step.with_moved_start(start).map(|x| (start, x))
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

    /// Check the integrity of this plan against an archive.
    ///
    /// This avoids running a plan which can't possibly succeed against an archive.
    ///
    /// If this plan returns `Ok(false)`, then running it against that archive *will*
    /// fail. An error might just be something spurious, e.g. an IO error.
    pub async fn check_against_archive(
        &self,
        archive: &Archive,
    ) -> anyhow::Result<anyhow::Result<()>> {
        let mut good = Ok(());
        for (start, step) in &self.steps {
            good = good.and(step.check_against_archive(*start, archive).await?);
        }
        Ok(good)
    }

    /// Some regeneration plans are pre-specified, by a chain id.
    pub fn from_known_chain_id(chain_id: &str) -> Option<Self> {
        match chain_id {
            "penumbra-1" => Some(Self::penumbra_1()),
            "penumbra-testnet-phobos-2" => Some(Self::penumbra_testnet_phobos_2()),
            _ => None,
        }
    }

    pub fn penumbra_testnet_phobos_2() -> Self {
        use RegenerationStep::*;
        use Version::*;

        Self {
            steps: vec![
                (
                    0,
                    InitThenRunTo {
                        genesis_height: 1,
                        version: V0o80,
                        last_block: Some(1459799),
                    },
                ),
                (
                    1459799,
                    Migrate {
                        from: V0o80,
                        to: V1,
                    },
                ),
                (
                    1459799,
                    InitThenRunTo {
                        genesis_height: 1459800,
                        version: V1,
                        last_block: Some(2358329),
                    },
                ),
                (23583289, Migrate { from: V1, to: V2 }),
                (
                    2358329,
                    InitThenRunTo {
                        genesis_height: 2358330,
                        version: V2,
                        last_block: None,
                    },
                ),
            ],
        }
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
                        last_block: Some(2611799),
                    },
                ),
                (
                    2611799,
                    Migrate {
                        from: V0o80,
                        to: V1,
                    },
                ),
                (
                    2611799,
                    InitThenRunTo {
                        genesis_height: 2611800,
                        version: V1,
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
    chain_id: String,
    working_dir: PathBuf,
    archive: Archive,
    indexer: Indexer,
    store: Option<Arc<dyn Store>>,
}

impl Regenerator {
    /// Load up a regenerator.
    pub async fn load(
        working_dir: &Path,
        archive: Archive,
        indexer: Indexer,
        store: Option<Box<dyn Store>>,
    ) -> anyhow::Result<Self> {
        let chain_id = archive.chain_id().await?;
        Ok(Self {
            chain_id,
            working_dir: working_dir.to_owned(),
            archive,
            indexer,
            store: store.map(|x| x.into()),
        })
    }

    pub async fn run(
        self,
        start_height: Option<u64>,
        stop_height: Option<u64>,
    ) -> anyhow::Result<()> {
        // Basic idea:
        //  1. Figure out the current height we've indexed to.
        //  2. Try and advance, height by height, until the stop height.
        //  2.1 If a migration needs to be run before this height, run it.
        //  2.2 If the chain needs to be initialized at this height, initialize it.
        //  2.3 Retrieve the block that needs to fed in, and then index the resulting events.
        //
        // It's regeneratin' time.
        let metadata = self.find_current_metadata().await?;
        if let Some((_, chain_id)) = &metadata {
            anyhow::ensure!(
                chain_id == &self.chain_id,
                "archive chain_id is '{}' but state is '{}'",
                self.chain_id,
                chain_id
            );
        }
        self.run_from(start_height.or(metadata.map(|x| x.0)), stop_height)
            .await
    }

    async fn find_current_metadata(&self) -> anyhow::Result<Option<(u64, String)>> {
        let mut out = None;
        for version in [Version::V0o79, Version::V0o80, Version::V1, Version::V2] {
            if out.is_some() {
                break;
            }
            let penumbra = make_a_penumbra(version, &self.working_dir).await?;
            match penumbra.metadata().await {
                Err(error) => {
                    tracing::debug!(?version, "error while fetching current metadata: {}", error);
                }
                Ok(x) => out = Some(x),
            }
            penumbra.release().await;
        }
        Ok(out)
    }

    async fn run_from(mut self, start: Option<u64>, stop: Option<u64>) -> anyhow::Result<()> {
        let plan = RegenerationPlan::from_known_chain_id(&self.chain_id)
            .map(|x| x.truncate(start, stop))
            .ok_or(anyhow!("no plan known for chain id '{}'", &self.chain_id))?;
        tracing::info!(
            "plan for {} truncated between {:?}..={:?}: {:?}",
            &self.chain_id,
            start,
            stop,
            plan
        );
        plan.check_against_archive(&self.archive).await??;
        for (start, step) in plan.steps.into_iter() {
            use RegenerationStep::*;
            match step {
                Migrate { from, to } => self.migrate(from, to).await?,
                InitThenRunTo {
                    genesis_height,
                    version,
                    last_block,
                } => {
                    self.init_then_run_to(genesis_height, version, start + 1, last_block)
                        .await?
                }
                RunTo {
                    version,
                    last_block,
                } => self.run_to(version, start + 1, last_block).await?,
            }
        }
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn migrate(&mut self, from: Version, to: Version) -> anyhow::Result<()> {
        tracing::info!("regeneration step");
        match to {
            Version::V0o80 => v0o80::migrate(from, &self.working_dir).await?,
            Version::V1 => v1::migrate(from, &self.working_dir).await?,
            Version::V2 => v2::migrate(from, &self.working_dir).await?,
            v => anyhow::bail!("impossible version {:?} to migrate from", v),
        }
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn init_then_run_to(
        &mut self,
        genesis_height: u64,
        version: Version,
        first_block: u64,
        last_block: Option<u64>,
    ) -> anyhow::Result<()> {
        tracing::info!("regeneration step");
        // Get genesis information, possibly from the store.
        let genesis = match self.archive.get_genesis(genesis_height).await? {
            Some(g) => g,
            None => {
                let Some(store) = self.store.as_mut() else {
                    anyhow::bail!("expected genesis at height {}", genesis_height);
                };
                let g = store.get_genesis().await?;
                self.archive.put_genesis(&g).await?;
                g
            }
        };
        let mut penumbra = make_a_penumbra(version, &self.working_dir).await?;
        penumbra.genesis(genesis).await?;

        self.run_to_inner(&mut penumbra, first_block, last_block)
            .await
    }

    #[tracing::instrument(skip(self))]
    async fn run_to(
        &mut self,
        version: Version,
        first_block: u64,
        last_block: Option<u64>,
    ) -> anyhow::Result<()> {
        tracing::info!("regeneration step");
        let mut penumbra = make_a_penumbra(version, &self.working_dir).await?;
        let res = self
            .run_to_inner(&mut penumbra, first_block, last_block)
            .await;
        penumbra.release().await;
        res
    }

    async fn run_to_inner(
        &mut self,
        penumbra: &mut APenumbra,
        first_block: u64,
        last_block: Option<u64>,
    ) -> anyhow::Result<()> {
        // First, regenerate using the blocks inside the archive.
        let last_height_in_archive = self
            .archive
            .last_height()
            .await?
            .ok_or(anyhow!("no blocks in archive"))?;
        let end = last_block.unwrap_or(u64::MAX).min(last_height_in_archive);
        tracing::info!(
            "running chain from heights {} to {}",
            first_block,
            last_block.map(|x| x.to_string()).unwrap_or("∞".to_string())
        );
        for height in first_block..=end {
            let block: Block = self
                .archive
                .get_block(height)
                .await?
                .ok_or(anyhow!("missing block at height {}", height))?
                .try_into()?;
            self.process_block(penumbra, height, block).await?;
        }
        let next_height = last_height_in_archive + 1;
        let Some(store) = self.store.clone() else {
            return Ok(());
        };

        tracing::info!("reached end of archive");
        // Set up a buffered producer of blocks.
        const BLOCK_BUFFER_SIZE: usize = 400;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(u64, _)>(BLOCK_BUFFER_SIZE);
        let producer: JoinHandle<anyhow::Result<()>> = tokio::spawn(async move {
            let mut stream = store.stream_blocks(Some(next_height), last_block);
            while let Some((height, block)) = stream.try_next().await? {
                tx.send((height, block)).await?;
            }
            Ok(())
        });
        while let Some((height, block)) = rx.recv().await {
            self.archive.put_block(&block).await?;
            self.process_block(penumbra, height, block.try_into()?)
                .await?;
        }

        // Make sure the producer hasn't created some kind of error.
        producer.await??;

        Ok(())
    }

    async fn process_block(
        &mut self,
        penumbra: &mut APenumbra,
        height: u64,
        block: Block,
    ) -> anyhow::Result<()> {
        if height % 100 == 0 {
            tracing::info!("reached height {}", height);
        }
        let block_tendermint: tendermint_v0o40::Block = block.clone().into();
        let begin_block = BeginBlock::from(block);
        self.indexer
            .enter_block(height, block_tendermint.header.chain_id.as_str())
            .await?;
        let events = penumbra.begin_block(&begin_block).await;
        self.indexer.events(height, events, None).await?;
        for (i, tx) in block_tendermint.data.into_iter().enumerate() {
            let events = penumbra.deliver_tx(&DeliverTx { tx: tx.clone() }).await;
            self.indexer
                .events(
                    height,
                    // anyhow::Error doesn't impl Clone, thus the as_ref -> map chain.
                    #[allow(clippy::useless_asref)]
                    events.as_ref().map(|x| x.clone()).unwrap_or_default(),
                    Some((i, &tx, ResponseDeliverTx::with_defaults(events))),
                )
                .await?;
        }
        let events = penumbra
            .end_block(&EndBlock {
                height: height.try_into()?,
            })
            .await;
        self.indexer.events(height, events, None).await?;
        penumbra.commit().await?;
        self.indexer.end_block().await?;

        Ok(())
    }
}
