#![cfg(feature = "network-integration")]
//! These integration tests operate on historical events for the public Penumbra Labs
//! testnet, identified by chain id `penumbra-testnet-phobos-2`.
#[path = "common/mod.rs"]
mod common;
use crate::common::run_reindexer_archive_step;

/// The chain id for the network being reindexed.
const PENUMBRA_CHAIN_ID: &str = "penumbra-testnet-phobos-2";

#[tokio::test]
/// Run `penumbra-reindexer archive` from block 0 to the first upgrade boundary.
async fn run_reindexer_archive_step_1() -> anyhow::Result<()> {
    let expected_blocks = 1459800;
    run_reindexer_archive_step(PENUMBRA_CHAIN_ID, 0, expected_blocks).await?;
    Ok(())
}

#[tokio::test]
/// Run `penumbra-reindexer archive` from the first upgrade boundary to the second.
async fn run_reindexer_archive_step_2() -> anyhow::Result<()> {
    let expected_blocks = 2358329;
    run_reindexer_archive_step(PENUMBRA_CHAIN_ID, 1, expected_blocks).await?;
    Ok(())
}
