use std::path::Path;

use async_trait::async_trait;
use cnidarium_v1::Storage;
use penumbra_sdk_app_v1o3::{app::App, PenumbraHost, SUBSTORE_PREFIXES};
use penumbra_sdk_ibc_v1o3::component::HostInterface as _;

use tendermint_v0o40 as tendermint;

use crate::cometbft::Genesis;
use crate::tendermint_compat::{BeginBlock, DeliverTx, EndBlock, Event};

pub struct Penumbra {
    storage: Storage,
    app: App,
}

impl Penumbra {
    pub async fn load(working_dir: &Path) -> anyhow::Result<Self> {
        let storage = Storage::load(working_dir.to_owned(), SUBSTORE_PREFIXES.to_vec()).await?;
        let app = App::new(storage.latest_snapshot());
        Ok(Self { storage, app })
    }
}

#[async_trait]
impl super::Penumbra for Penumbra {
    async fn release(self: Box<Self>) {
        self.storage.release().await
    }

    async fn genesis(&mut self, genesis: Genesis) -> anyhow::Result<()> {
        self.app
            .init_chain(&serde_json::from_value(genesis.app_state().clone())?)
            .await;
        Ok(())
    }

    async fn metadata(&self) -> anyhow::Result<(u64, String)> {
        let snapshot = self.storage.latest_snapshot();
        let height = PenumbraHost::get_block_height(snapshot.clone()).await?;
        let chain_id = PenumbraHost::get_chain_id(snapshot).await?;
        Ok((height, chain_id))
    }

    async fn begin_block(&mut self, req: &BeginBlock) -> Vec<Event> {
        let compat_block: tendermint::abci::request::BeginBlock = req.clone().into();
        self.app
            .begin_block(&compat_block)
            .await
            .into_iter()
            .map(|e| e.try_into().unwrap())
            .collect()
    }

    async fn deliver_tx(&mut self, req: &DeliverTx) -> anyhow::Result<Vec<Event>> {
        let compat_tx: tendermint::abci::request::DeliverTx = req.clone().into();
        let events = self.app.deliver_tx_bytes(&compat_tx.tx).await?;
        Ok(events.into_iter().map(|e| e.try_into().unwrap()).collect())
    }

    async fn end_block(&mut self, req: &EndBlock) -> Vec<Event> {
        let compat_block: tendermint::abci::request::EndBlock = req.clone().into();
        self.app
            .end_block(&compat_block)
            .await
            .into_iter()
            .map(|e| e.try_into().unwrap())
            .collect()
    }

    async fn commit(&mut self) -> anyhow::Result<()> {
        self.app.commit(self.storage.clone()).await;
        Ok(())
    }
}

mod migration {
    use cnidarium_v1::StateDelta;
    use penumbra_sdk_app_v1o3::SUBSTORE_PREFIXES;
    use penumbra_sdk_governance_v1o3::StateWriteExt;
    use penumbra_sdk_sct_v1o3::component::clock::EpochManager as _;

    use super::super::Version;
    use super::*;

    pub async fn migrate(from: Version, working_dir: &Path) -> anyhow::Result<()> {
        anyhow::ensure!(from == Version::V0o80, "version must be v0.80.x");
        let storage = Storage::load(working_dir.to_owned(), SUBSTORE_PREFIXES.to_vec()).await?;
        let initial_state = storage.latest_snapshot();
        let mut delta = StateDelta::new(initial_state);

        // Reset the application height and halt flag.
        delta.ready_to_start();
        delta.put_block_height(0u64);

        // Finally, commit the changes to the chain state.
        let post_upgrade_root_hash = storage.commit_in_place(delta).await?;
        tracing::info!(?post_upgrade_root_hash, "post-migration root hash");
        storage.release().await;

        Ok(())
    }
}

pub use migration::migrate;
