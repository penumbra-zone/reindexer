use anyhow::anyhow;
use async_stream::try_stream;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::{ops::Range, time::Duration};
use tokio::time::Instant;

use super::{Block, BlockStream, Genesis};

trait ValueExtension: Sized {
    fn expect_key(&self, key: &str) -> anyhow::Result<&Self>;
    fn expect_u64_string(&self) -> anyhow::Result<u64>;
    fn expect_array(&self) -> anyhow::Result<&Vec<Self>>;
}

impl ValueExtension for Value {
    fn expect_key(&self, key: &str) -> anyhow::Result<&Self> {
        self.get(key).ok_or(anyhow!("expected key `{}`", key))
    }

    fn expect_u64_string(&self) -> anyhow::Result<u64> {
        let out = self.as_str().ok_or(anyhow!("expected string"))?.parse()?;
        Ok(out)
    }

    fn expect_array(&self) -> anyhow::Result<&Vec<Self>> {
        self.as_array().ok_or(anyhow!("expected array"))
    }
}

async fn request<T>(
    client: &Client,
    url: String,
    params: &[(&str, &str)],
    parser: impl FnOnce(&Value) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let res: Value = client.get(url).query(params).send().await?.json().await?;
    if let Some(err) = res.get("error") {
        return Err(anyhow!("JSON RPC error: {}", err));
    }
    let body = res.expect_key("result")?;
    parser(body)
}

/// A store which accesses a remote penumbra node's cometbft RPC.
///
/// The block streaming implementation will continue polling the new node
/// for blocks, until the specified end height will be reached, allowing
/// following a node in real time.
#[derive(Clone)]
pub struct RemoteStore {
    #[allow(dead_code)]
    base_url: String,
    client: Client,
}

impl RemoteStore {
    /// This takes in the URL for the cometbft rpc.
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::new(),
        }
    }

    async fn get_blocks(&self, range: Range<u64>) -> anyhow::Result<Vec<Block>> {
        let mut out = Vec::with_capacity((range.end - range.start) as usize);
        let url = format!("{}/block_search", &self.base_url);
        let query = format!(
            "\"block.height >= {} AND block.height < {}\"",
            range.start, range.end
        );
        let params = [
            ("query", query.as_str()),
            ("per_page", "100"),
            ("page", "1"),
            ("order_by", "\"asc\""),
        ];
        request(&self.client, url, &params, move |value| {
            let blocks = value.expect_key("blocks")?.expect_array()?;
            for block in blocks {
                let res = block.expect_key("block")?.clone().try_into()?;
                out.push(res);
            }
            Ok(out)
        })
        .await
    }
}

#[async_trait]
impl super::Store for RemoteStore {
    async fn get_genesis(&self) -> anyhow::Result<Genesis> {
        let url = format!("{}/genesis", self.base_url);
        request(&self.client, url, &[], |value| {
            value.expect_key("genesis")?.clone().try_into()
        })
        .await
    }

    async fn get_height_bounds(&self) -> anyhow::Result<Option<(u64, u64)>> {
        let url = format!("{}/status", self.base_url);
        request(&self.client, url, &[], |value| {
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
        let blocks = self.get_blocks(height..height + 1).await?;
        Ok(blocks.into_iter().next())
    }

    fn stream_blocks(&self, start: Option<u64>, end: Option<u64>) -> BlockStream<'_> {
        const BLOCKS_AT_A_TIME: u64 = 100;
        const REQUEST_SLEEP: Duration = Duration::from_millis(100);
        const POLL_SLEEP: Duration = Duration::from_millis(1000);
        let this = self.clone();
        let mut height = start.unwrap_or(1);
        let stream = try_stream! {
            while end.map(|x| height <= x).unwrap_or(true) {
                let poll_start_time = Instant::now();
                let most_recent_block = {
                    let (_, mut most_recent_block) = this
                        .get_height_bounds()
                        .await?
                        .ok_or(anyhow!("RPC did not return any height bounds"))?;
                    if let Some(x) = end {
                        most_recent_block = most_recent_block.min(x)
                    }
                    most_recent_block
                };
                // In the case where height = most_recent_block, we have not yet indexed the last block.
                while height <= most_recent_block {
                    let request_start_time = Instant::now();
                    let buf = this.get_blocks(height..height + BLOCKS_AT_A_TIME).await?;
                    if buf.is_empty() {
                        // Macro shenanigans.
                        Err(anyhow!("RPC returned an empty list of blocks"))?;
                    }
                    tracing::info!(
                        start_height = buf.first().expect("buf is not empty").height,
                        end_height = buf.last().expect("buf is not empty").height,
                        "new blocks from remote store"
                    );
                    for block in buf.into_iter() {
                        let block_height = block.height();
                        if block_height != height {
                            // Macro shenanigans.
                            Err(anyhow!("unexpected block height: {}", block_height))?;
                        }
                        yield (height, block);
                        height += 1;
                    }
                    tokio::time::sleep_until(request_start_time + REQUEST_SLEEP).await;
                }
                tokio::time::sleep_until(poll_start_time + POLL_SLEEP).await;
            }
        };
        Box::pin(stream)
    }
}
