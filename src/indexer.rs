use hex::ToHex;
use sha2::Digest;
use sqlx::{PgPool, Postgres, Transaction};

use crate::tendermint_compat::{Event, ResponseDeliverTx};

async fn fetch_block_id(
    dbtx: &mut Transaction<'static, Postgres>,
    height: u64,
) -> anyhow::Result<Option<i64>> {
    Ok(
        sqlx::query_scalar("SELECT rowid FROM blocks WHERE height = $1")
            .bind(i64::try_from(height)?)
            .fetch_optional(dbtx.as_mut())
            .await?,
    )
}

async fn block_exists(
    dbtx: &mut Transaction<'static, Postgres>,
    height: u64,
) -> anyhow::Result<bool> {
    Ok(fetch_block_id(dbtx, height).await?.is_some())
}

async fn tx_exists(
    dbtx: &mut Transaction<'static, Postgres>,
    height: u64,
    index: usize,
) -> anyhow::Result<bool> {
    Ok(sqlx::query_scalar(
        "
       SELECT EXISTS(
           SELECT 1
           FROM tx_results
           JOIN blocks ON blocks.rowid = tx_results.block_id
           WHERE height = $1
           AND index = $2
    )",
    )
    .bind(i64::try_from(height)?)
    .bind(i64::try_from(index)?)
    .fetch_one(dbtx.as_mut())
    .await?)
}

struct Context {
    block_id: i64,
    dbtx: Transaction<'static, Postgres>,
}

#[derive(Clone, Debug, Default)]
pub struct IndexerOpts {
    /// If set, will allow there to be existing data in the database, with the behavior
    /// of not overwriting that data, and instead continuing silently.
    pub allow_existing_data: bool,
}

/// Represents an indexer for raw ABCI events.
///
/// This will hook into the postgres backend that we expect to see.
pub struct Indexer {
    pool: PgPool,
    context: Option<Context>,
    opts: IndexerOpts,
}

impl Drop for Indexer {
    fn drop(&mut self) {
        // This assumes a multi-threaded tokio runtime.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.pool.close().await;
            });
        });
    }
}

#[allow(dead_code)]
impl Indexer {
    /// Initialize the indexer with a given database url.
    #[tracing::instrument]
    pub async fn init(database_url: &str, opts: IndexerOpts) -> anyhow::Result<Self> {
        tracing::info!("initializing database");

        let pool = PgPool::connect(database_url).await?;
        let mut dbtx = pool.begin().await?;
        for statement in include_str!("indexer/schema.sql").split(";") {
            sqlx::query(statement).execute(dbtx.as_mut()).await?;
        }
        dbtx.commit().await?;
        Ok(Self {
            pool,
            context: None,
            opts,
        })
    }

    /// Signal the start of a new block.
    ///
    /// This will index whatever information about the block we need, and also
    /// set a context for the current block for subsequent events.
    pub async fn enter_block(&mut self, height: u64, chain_id: &str) -> anyhow::Result<()> {
        tracing::debug!(height, "indexing block");
        assert!(self.context.is_none());
        let mut dbtx = self.pool.begin().await?;
        let block_id: i64 = match fetch_block_id(&mut dbtx, height).await? {
            None => {
                let (block_id,): (i64,) = sqlx::query_as(
                "INSERT INTO blocks VALUES (DEFAULT, $1, $2, CURRENT_TIMESTAMP) RETURNING rowid",
            )
            .bind(i64::try_from(height)?)
            .bind(chain_id)
            .fetch_one(dbtx.as_mut())
            .await?;
                block_id
            }
            Some(id) if self.opts.allow_existing_data => id,
            Some(_) => {
                anyhow::bail!("block at height {} has already been indexed", height)
            }
        };
        self.context = Some(Context { block_id, dbtx });
        self.events(
            height,
            vec![Event {
                kind: "block".to_string(),
                attributes: vec![(
                    "height".as_bytes().to_vec(),
                    height.to_string().into_bytes(),
                    true,
                )],
            }],
            None,
        )
        .await?;
        Ok(())
    }

    /// Signal the end of the block.
    ///
    /// This allows our changes to be committed.
    pub async fn end_block(&mut self, app_hash: &[u8]) -> anyhow::Result<()> {
        let old_context = self.context.take();
        let mut context = match old_context {
            None => panic!("we should be inside a block before ending it"),
            Some(ctx) => ctx,
        };
        let skip = if self.opts.allow_existing_data {
            sqlx::query_scalar(
                "
                SELECT EXISTS(
                    SELECT 1
                    FROM debug.app_hash       
                    WHERE block_id =  $1
                )",
            )
            .bind(context.block_id)
            .fetch_one(context.dbtx.as_mut())
            .await?
        } else {
            false
        };
        if !skip {
            sqlx::query("INSERT INTO debug.app_hash VALUES (DEFAULT, $1, $2)")
                .bind(context.block_id)
                .bind(app_hash)
                .execute(context.dbtx.as_mut())
                .await?;
        }
        context.dbtx.commit().await?;
        Ok(())
    }

    /// Deliver events, and have them indexed.
    ///
    /// We can optionally provide a transaction to exist as context for the events.
    /// This should only be called once per transaction.
    pub async fn events(
        &mut self,
        height: u64,
        events: Vec<Event>,
        tx: Option<(usize, &[u8], ResponseDeliverTx)>,
    ) -> anyhow::Result<()> {
        tracing::debug!("indexing {} events", events.len());
        let context = match &mut self.context {
            None => panic!("we should be inside a block before indexing events"),
            Some(ctx) => ctx,
        };
        if self.opts.allow_existing_data {
            // We want to skip indexing these events if the relevant generator (i.e. the block, or the tx)
            // has already been indexed. We do this at this level of granularity, because the underlying
            // cometbft impl https://github.com/cometbft/cometbft/blob/e820315631a81c230e4abe9bcede8e29382e8af5/state/txindex/indexer_service.go
            // does the same. It doesn't do one transaction per block, but rather one transaction for the events
            // tied to the block itself, and another for each transaction.
            if let Some((index, _, _)) = tx {
                if tx_exists(&mut context.dbtx, height, index).await? {
                    tracing::debug!("tx ({}, {}) exists; skipping", height, index);
                    return Ok(());
                }
            } else if block_exists(&mut context.dbtx, height).await? {
                tracing::debug!("block {} exists; skipping", height);
                return Ok(());
            }
        }
        let block_id = context.block_id;
        let (pseudo_events, tx_id): (Vec<Event>, Option<i64>) = match tx {
            None => (Vec::new(), None),
            Some((index, raw_tx, exec_result)) => {
                let tx_hash: String = sha2::Sha256::digest(raw_tx).encode_hex_upper();
                let tx_result_bytes =
                    exec_result.encode_to_latest_tx_result(height as i64, index as u32, raw_tx);

                let (tx_id,): (i64,) = sqlx::query_as(
                    "INSERT INTO tx_results VALUES (DEFAULT, $1, $2, CURRENT_TIMESTAMP, $3, $4) RETURNING rowid",
                )
                .bind(block_id)
                .bind(i32::try_from(index)?)
                .bind(&tx_hash)
                .bind(tx_result_bytes)
                .fetch_one(context.dbtx.as_mut())
                .await?;
                let pseudo_events = vec![
                    Event {
                        kind: "tx".to_string(),
                        attributes: vec![(
                            "hash".as_bytes().to_vec(),
                            tx_hash.as_bytes().to_vec(),
                            true,
                        )],
                    },
                    Event {
                        kind: "tx".to_string(),
                        attributes: vec![(
                            "height".as_bytes().to_vec(),
                            height.to_string().into_bytes(),
                            true,
                        )],
                    },
                ];
                (pseudo_events, Some(tx_id))
            }
        };
        for event in pseudo_events.into_iter().chain(events.into_iter()) {
            let (event_id,): (i64,) =
                sqlx::query_as("INSERT INTO events VALUES (DEFAULT, $1, $2, $3) RETURNING rowid")
                    .bind(block_id)
                    .bind(tx_id)
                    .bind(&event.kind)
                    .fetch_one(context.dbtx.as_mut())
                    .await?;
            for (key, value, _) in event.attributes {
                let key = String::from_utf8(key)?;
                let value = String::from_utf8(value)?;
                sqlx::query("INSERT INTO attributes VALUES ($1, $2, $3, $4)")
                    .bind(event_id)
                    .bind(&key)
                    .bind(format!("{}.{}", &event.kind, &key))
                    .bind(value)
                    .execute(context.dbtx.as_mut())
                    .await?;
            }
        }

        Ok(())
    }
}
