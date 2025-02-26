use std::path::Path;

use async_trait::async_trait;
use cnidarium_v0o81::Storage;
use penumbra_app_v0o81::{app::App, PenumbraHost, SUBSTORE_PREFIXES};
use penumbra_ibc_v0o81::component::HostInterface as _;
use tendermint::{
    abci::Event,
    v0_37::abci::request::{BeginBlock, DeliverTx, EndBlock},
};

use crate::cometbft::Genesis;

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
        self.app.begin_block(req).await
    }

    async fn deliver_tx(&mut self, req: &DeliverTx) -> anyhow::Result<Vec<Event>> {
        self.app.deliver_tx_bytes(&req.tx).await
    }

    async fn end_block(&mut self, req: &EndBlock) -> Vec<Event> {
        self.app.end_block(req).await
    }

    async fn commit(&mut self) -> anyhow::Result<()> {
        self.app.commit(self.storage.clone()).await;
        Ok(())
    }
}

mod migration {
    use cnidarium_v0o81::StateDelta;
    use penumbra_app_v0o80::SUBSTORE_PREFIXES;
    // use penumbra_app_v0o81::app::StateReadExt as _;
    use penumbra_governance_v0o81::StateWriteExt;
    use penumbra_sct_v0o81::component::clock::EpochManager as _;
    // use penumbra_transaction_v0o81::{Action, Transaction};

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
