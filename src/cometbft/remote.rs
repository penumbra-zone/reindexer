use async_trait::async_trait;

use super::{Block, Genesis};

/// A store which accesses a remote penumbra node's cometbft RPC.
pub struct RemoteStore {
    #[allow(dead_code)]
    base_url: String,
}

impl RemoteStore {
    /// This takes in the URL for the cometbft rpc.
    pub fn new(base_url: String) -> Self {
        Self { base_url }
    }
}

#[async_trait]
impl super::Store for RemoteStore {
    async fn get_genesis(&self) -> anyhow::Result<Genesis> {
        todo!()
    }

    async fn get_height_bounds(&self) -> anyhow::Result<Option<(u64, u64)>> {
        todo!()
    }

    async fn get_block(&self, _height: u64) -> anyhow::Result<Option<Block>> {
        todo!()
    }
}
