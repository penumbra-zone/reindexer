#![cfg(feature = "network-integration")]
//! These integration tests operate on historical events for the public Penumbra Labs
//! testnet, identified by chain id `penumbra-testnet-phobos-3`.
#[path = "common/mod.rs"]
mod common;
use crate::common::{run_reindexer_archive_step, run_reindexer_regen_step};

/// The chain id for the network being reindexed.
const PENUMBRA_CHAIN_ID: &str = "penumbra-testnet-phobos-3";

#[tokio::test]
/// Run `penumbra-reindexer archive` from block 0 to present.
async fn run_reindexer_archive_step_1() -> anyhow::Result<()> {
    let expected_blocks = 368331;
    run_reindexer_archive_step(PENUMBRA_CHAIN_ID, 0, expected_blocks).await?;
    Ok(())
}

#[tokio::test]
/// Run `penumbra-reindexer regen` from block 0 to the present.
async fn run_reindexer_regen_step_1() -> anyhow::Result<()> {
    let stop_height = None;
    run_reindexer_regen_step(PENUMBRA_CHAIN_ID, 0, stop_height).await?;
    Ok(())
}
