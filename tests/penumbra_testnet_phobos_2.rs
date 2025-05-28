#![cfg(feature = "network-integration")]
//! These integration tests operate on historical events for the public Penumbra Labs
//! testnet, identified by chain id `penumbra-testnet-phobos-2`.
#[path = "common/mod.rs"]
mod common;
use crate::common::{run_reindexer_archive_step, run_reindexer_regen_step};

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

#[tokio::test]
/// Run `penumbra-reindexer archive` from the second upgrade boundary to present.
async fn run_reindexer_archive_step_3() -> anyhow::Result<()> {
    let expected_blocks = 3280053;
    run_reindexer_archive_step(PENUMBRA_CHAIN_ID, 2, expected_blocks).await?;
    Ok(())
}

#[tokio::test]
/// Run `penumbra-reindexer regen` from block 0 to the first upgrade boundary.
async fn run_reindexer_regen_step_1() -> anyhow::Result<()> {
    let stop_height = Some(1459800);
    run_reindexer_regen_step(PENUMBRA_CHAIN_ID, 0, stop_height).await?;
    Ok(())
}

#[tokio::test]
/// Run `penumbra-reindexer regen` from the first upgrade boundary to the second.
async fn run_reindexer_regen_step_2() -> anyhow::Result<()> {
    let stop_height = Some(2358329);
    run_reindexer_regen_step(PENUMBRA_CHAIN_ID, 1, stop_height).await?;
    Ok(())
}
#[tokio::test]
/// Run `penumbra-reindexer regen` from the first upgrade boundary to the second.
async fn run_reindexer_regen_step_3() -> anyhow::Result<()> {
    let stop_height = None;
    run_reindexer_regen_step(PENUMBRA_CHAIN_ID, 2, stop_height).await?;
    Ok(())
}
