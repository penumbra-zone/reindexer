use std::path::Path;

use async_trait::async_trait;
use cnidarium_v0o79::Storage;
use penumbra_app_v0o79::{app::App, PenumbraHost, SUBSTORE_PREFIXES};
use penumbra_ibc_v0o79::component::HostInterface;
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
        Ok(self
            .app
            .init_chain(&serde_json::from_value(genesis.app_state().clone())?)
            .await)
    }

    async fn metadata(&self) -> anyhow::Result<(u64, String)> {
        let snapshot = self.storage.latest_snapshot();
        let height = PenumbraHost::get_block_height(snapshot.clone()).await?;
        let chain_id = PenumbraHost::get_chain_id(snapshot).await?;
        Ok((height, chain_id))
    }

    async fn begin_block(&mut self, req: &BeginBlock) -> Vec<Event> {
        self.app.begin_block(&req).await
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
