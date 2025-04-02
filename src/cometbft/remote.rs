use anyhow::anyhow;
use async_trait::async_trait;
use serde_json::Value;

use super::{Block, Genesis};

trait ValueExtension {
    fn expect_key(&self, key: &str) -> anyhow::Result<&Self>;
    fn expect_u64_string(&self) -> anyhow::Result<u64>;
}

impl ValueExtension for Value {
    fn expect_key(&self, key: &str) -> anyhow::Result<&Self> {
        self.get(key).ok_or(anyhow!("expected key `{}`", key))
    }

    fn expect_u64_string(&self) -> anyhow::Result<u64> {
        let out = self.as_str().ok_or(anyhow!("expected string"))?.parse()?;
        return Ok(out);
    }
}

async fn request<T>(
    url: String,
    params: &[(&str, &str)],
    parser: impl FnOnce(&Value) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let client = reqwest::Client::new();
    let res: Value = client.get(url).query(params).send().await?.json().await?;
    if let Some(err) = res.get("error") {
        return Err(anyhow!("JSON RPC error: {}", err));
    }
    let body = res.expect_key("result")?;
    parser(body)
}

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
        let url = format!("{}/genesis", self.base_url);
        request(url, &[], |value| {
            value.expect_key("genesis")?.clone().try_into()
        })
        .await
    }

    async fn get_height_bounds(&self) -> anyhow::Result<Option<(u64, u64)>> {
        let url = format!("{}/status", self.base_url);
        request(url, &[], |value| {
            let sync_info = value.expect_key("sync_info")?;
            let start = sync_info
                .expect_key("earliest_block_height")?
                .expect_u64_string()?;
            let end = sync_info
                .expect_key("latest_block_height")?
                .expect_u64_string()?;
            Ok(Some((start, end)))
        })
        .await
    }

    async fn get_block(&self, height: u64) -> anyhow::Result<Option<Block>> {
        let url = format!("{}/block", self.base_url);
        request(url, &[("height", &height.to_string())], |value| {
            let block = value.expect_key("block")?;
            Ok(Some(block.clone().try_into()?))
        })
        .await
    }
}
