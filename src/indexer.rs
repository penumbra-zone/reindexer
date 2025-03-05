use hex::ToHex;
use sha2::Digest;
use sqlx::{PgPool, Postgres, Transaction};

use crate::tendermint_compat::{Event, ResponseDeliverTx};

struct Context {
    block_id: i64,
    dbtx: Transaction<'static, Postgres>,
}

/// Represents an indexer for raw ABCI events.
///
/// This will hook into the postgres backend that we expect to see.
pub struct Indexer {
    pool: PgPool,
    context: Option<Context>,
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
    pub async fn init(database_url: &str) -> anyhow::Result<Self> {
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
        let (block_id,): (i64,) = sqlx::query_as(
            "INSERT INTO blocks VALUES (DEFAULT, $1, $2, CURRENT_TIMESTAMP) RETURNING rowid",
        )
        .bind(i64::try_from(height)?)
        .bind(chain_id)
        .fetch_one(dbtx.as_mut())
        .await?;
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
    pub async fn end_block(&mut self) -> anyhow::Result<()> {
        let old_context = self.context.take();
        let context = match old_context {
            None => panic!("we should be inside a block before ending it"),
            Some(ctx) => ctx,
        };
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
