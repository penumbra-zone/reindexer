#![cfg(feature = "network-integration")]
//! These integration tests operate on historical events for the
//! Penumbra mainnet, identified by chain id `penumbra-1`.

#[path = "common/mod.rs"]
mod common;
use crate::common::run_reindexer_archive_step;

/// The chain id for the network being reindexed.
const PENUMBRA_CHAIN_ID: &str = "penumbra-1";

#[tokio::test]
/// Run `penumbra-reindexer archive` from block 0 to the first upgrade boundary.
async fn run_reindexer_archive_step_1() -> anyhow::Result<()> {
    let expected_blocks = 501974;
    run_reindexer_archive_step(PENUMBRA_CHAIN_ID, 0, expected_blocks).await?;
    Ok(())
}

#[tokio::test]
/// Run `penumbra-reindexer archive` from the first upgrade boundary to the second.
async fn run_reindexer_archive_step_2() -> anyhow::Result<()> {
    let expected_blocks = 2611800;
    run_reindexer_archive_step(PENUMBRA_CHAIN_ID, 1, expected_blocks).await?;
    Ok(())
}

#[tokio::test]
/// Run `penumbra-reindexer archive` from the second upgrade boundary to the present.
async fn run_reindexer_archive_step_3() -> anyhow::Result<()> {
    let expected_blocks = 4027443;
    run_reindexer_archive_step(PENUMBRA_CHAIN_ID, 2, expected_blocks).await?;
    Ok(())
}
