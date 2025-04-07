use std::{ops::Range, time::Duration};

use anyhow::anyhow;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_stream::wrappers::ReceiverStream;

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
        const BUFFER: usize = 10;
        const BLOCKS_AT_A_TIME: u64 = 100;
        const REQUEST_SLEEP: Duration = Duration::from_millis(100);
        const POLL_SLEEP: Duration = Duration::from_millis(1000);
        let this = self.clone();
        let (tx, rx) = mpsc::channel::<anyhow::Result<(u64, Block)>>(BUFFER);
        tokio::spawn(async move {
            let (start_block, end_block) = match this.get_height_bounds().await {
                Err(e) => {
                    tx.send(Err(e)).await?;
                    return Ok(());
                }
                Ok(None) => {
                    tx.send(Err(anyhow!("RPC did not return any height bounds")))
                        .await?;
                    return Ok(());
                }
                Ok(Some((mut start_block, mut end_block))) => {
                    if let Some(x) = start {
                        start_block = start_block.max(x);
                    }
                    if let Some(x) = end {
                        end_block = end_block.min(x);
                    }
                    (start_block, end_block)
                }
            };
            // `height` is *always* the next block we have not indexed.
            let mut height = start_block;
            // In the case where height = end_block, we have not yet indexed the last block.
            while height <= end_block {
                let request_start_time = Instant::now();
                let buf = match this.get_blocks(height..height + BLOCKS_AT_A_TIME).await {
                    Err(e) => {
                        tx.send(Err(e)).await?;
                        return Ok(());
                    }
                    Ok(blocks) => blocks,
                };
                if buf.is_empty() {
                    tx.send(Err(anyhow!("RPC returned an empty list of blocks")))
                        .await?;
                    return Ok(());
                }
                for block in buf.into_iter() {
                    let block_height = block.height();
                    if block_height != height {
                        tx.send(Err(anyhow!("unexpected block height: {}", block_height)))
                            .await?;
                        return Ok(());
                    }
                    tx.send(Ok((height, block))).await?;
                    height += 1;
                }
                tokio::time::sleep_until(request_start_time + REQUEST_SLEEP).await;
            }
            // Now, transition to fetching remaining blocks, one-by-one.
            // If there's no specified end, go on forever.
            while end.map(|x| height <= x).unwrap_or(true) {
                let request_start_time = Instant::now();
                let next_block = this.get_block(height).await?;
                if let Some(block) = next_block {
                    tx.send(Ok((height, block))).await?;
                    height += 1;
                }
                tokio::time::sleep_until(request_start_time + POLL_SLEEP).await;
            }
            Result::<(), anyhow::Error>::Ok(())
        });

        Box::pin(ReceiverStream::new(rx))
    }
}
