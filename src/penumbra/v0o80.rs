use std::path::Path;

use async_trait::async_trait;
use cnidarium_v0o80::Storage;
use penumbra_app_v0o80::{app::App, PenumbraHost, SUBSTORE_PREFIXES};
use penumbra_ibc_v0o80::component::HostInterface as _;

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

    async fn current_height(&self) -> anyhow::Result<u64> {
        Ok(PenumbraHost::get_block_height(self.storage.latest_snapshot()).await?)
    }
}
