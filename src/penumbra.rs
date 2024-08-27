use anyhow::anyhow;
use async_trait::async_trait;
use std::{
    path::{Path, PathBuf},
    u64,
};
use tendermint::{
    abci::{
        types::{
            BlockSignatureInfo, CommitInfo, Misbehavior, MisbehaviorKind, Validator, VoteInfo,
        },
        Event,
    },
    block::CommitSig,
    evidence::Evidence,
    v0_37::abci::request::{BeginBlock, DeliverTx, EndBlock},
};

use crate::{cometbft::Genesis, indexer::Indexer, storage::Storage as Archive};

mod v0o79;
mod v0o80;

#[async_trait]
trait Penumbra {
    async fn release(self: Box<Self>);
    async fn genesis(&mut self, genesis: Genesis) -> anyhow::Result<()>;
    async fn current_height(&self) -> anyhow::Result<u64>;
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
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Version {
    V0o79,
    V0o80,
}

#[derive(Debug, Clone, Copy, PartialEq)]
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
    working_dir: PathBuf,
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
        Ok(Self {
            working_dir: working_dir.to_owned(),
            archive,
            indexer,
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
        let current_height = self.find_current_height().await?;
        self.run_from(start_height.or(current_height), stop_height)
            .await
    }

    async fn find_current_height(&self) -> anyhow::Result<Option<u64>> {
        let mut out = None;
        for version in [Version::V0o79, Version::V0o80] {
            if let Some(_) = out {
                break;
            }
            let penumbra = make_a_penumbra(version, &self.working_dir).await?;
            match penumbra.current_height().await {
                Err(error) => {
                    tracing::debug!(?version, "error while fetching current height: {}", error);
                }
                Ok(x) => out = Some(x),
            }
            penumbra.release().await;
        }
        Ok(out)
    }

    async fn run_from(mut self, start: Option<u64>, stop: Option<u64>) -> anyhow::Result<()> {
        let plan = RegenerationPlan::penumbra_1().truncate(start, stop);
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
        let genesis = self
            .archive
            .get_genesis(genesis_height)
            .await?
            .ok_or(anyhow!("expected genesis before height {}", genesis_height))?;
        let mut penumbra = make_a_penumbra(version, &self.working_dir).await?;
        penumbra.genesis(genesis).await?;

        self.run_to_inner(penumbra, first_block, last_block).await
    }

    #[tracing::instrument(skip(self))]
    async fn run_to(
        &mut self,
        version: Version,
        first_block: u64,
        last_block: Option<u64>,
    ) -> anyhow::Result<()> {
        tracing::info!("regeneration step");
        let penumbra = make_a_penumbra(version, &self.working_dir).await?;
        self.run_to_inner(penumbra, first_block, last_block).await
    }

    async fn run_to_inner(
        &mut self,
        mut penumbra: APenumbra,
        first_block: u64,
        last_block: Option<u64>,
    ) -> anyhow::Result<()> {
        // The first block we need to process is 1 after our current height.
        // The last block we need to process is the one dictated to us, or the one past the last
        let last_height_in_archive = self
            .archive
            .last_height()
            .await?
            .ok_or(anyhow!("no blocks in archive"))?;
        let end = last_block.unwrap_or(u64::MAX).min(last_height_in_archive);
        tracing::info!("running chain from heights {} to {}", first_block, end);
        for height in first_block..=end {
            if height % 100 == 0 {
                tracing::info!("reached height {}", height);
            }
            let block = self
                .archive
                .get_block(height)
                .await?
                .ok_or(anyhow!("missing block at height {}", height))?
                .tendermint()?;
            self.indexer.enter_block(height).await?;
            let events = penumbra.begin_block(&create_begin_block(&block)).await;
            self.indexer.events(events).await?;
            for tx in block.data {
                let events = penumbra
                    .deliver_tx(&DeliverTx { tx: tx.into() })
                    .await
                    .unwrap_or_default();
                self.indexer.enter_tx().await?;
                self.indexer.events(events).await?;
            }
            self.indexer.before_end_block().await?;
            let events = penumbra
                .end_block(&EndBlock {
                    height: height.try_into()?,
                })
                .await;
            self.indexer.events(events).await?;
            penumbra.commit().await?;
        }
        penumbra.release().await;
        Ok(())
    }
}

fn create_begin_block(block: &tendermint::Block) -> BeginBlock {
    BeginBlock {
        hash: block.header.hash(),
        header: block.header.clone(),
        last_commit_info: commit_to_info(block.last_commit.as_ref()),
        byzantine_validators: block
            .evidence
            .iter()
            .flat_map(evidence_to_misbehavior)
            .collect(),
    }
}

fn commit_to_info(commit: Option<&tendermint::block::Commit>) -> CommitInfo {
    match commit {
        // DRAGON: We don't insert explicit votes for the validators that aren't present in this Commit,
        // which is fine with how Penumbra logic works. This may change in the future.
        Some(x) => CommitInfo {
            round: x.round,
            votes: x
                .signatures
                .iter()
                .filter_map(|x| match x {
                    CommitSig::BlockIdFlagAbsent => None,
                    CommitSig::BlockIdFlagCommit {
                        validator_address, ..
                    } => Some(VoteInfo {
                        // DRAGON: we assume that the penumbra logic will not care about the power
                        // we declare here.
                        validator: make_validator(*validator_address, Default::default()),
                        sig_info: BlockSignatureInfo::Flag(tendermint::block::BlockIdFlag::Commit),
                    }),
                    CommitSig::BlockIdFlagNil {
                        validator_address, ..
                    } => Some(VoteInfo {
                        // DRAGON: we assume that the penumbra logic will not care about the power
                        // we declare here.
                        validator: make_validator(*validator_address, Default::default()),
                        sig_info: BlockSignatureInfo::Flag(tendermint::block::BlockIdFlag::Nil),
                    }),
                })
                .collect(),
        },
        None => CommitInfo {
            round: Default::default(),
            votes: Default::default(),
        },
    }
}

fn evidence_to_misbehavior(evidence: &Evidence) -> Vec<Misbehavior> {
    match evidence {
        Evidence::DuplicateVote(bad) => vec![Misbehavior {
            kind: MisbehaviorKind::DuplicateVote,
            validator: make_validator(bad.vote_a.validator_address, bad.validator_power),
            height: bad.vote_a.height,
            time: bad.timestamp,
            total_voting_power: bad.total_voting_power,
        }],
        // I'm really not sure if this is correct, but seems logical?
        Evidence::LightClientAttack(bad) => bad
            .byzantine_validators
            .iter()
            .map(|v| Misbehavior {
                kind: MisbehaviorKind::LightClientAttack,
                validator: make_validator(v.address, v.power),
                height: bad.common_height,
                time: bad.timestamp,
                total_voting_power: bad.total_voting_power,
            })
            .collect(),
    }
}

fn make_validator(address: tendermint::account::Id, power: tendermint::vote::Power) -> Validator {
    Validator {
        address: address
            .as_bytes()
            .try_into()
            .expect("address should be the right size"),
        power,
    }
}
