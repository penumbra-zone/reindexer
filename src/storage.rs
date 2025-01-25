use std::{path::Path, str::FromStr};

use anyhow::anyhow;
use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};

use crate::cometbft::{Block, Genesis};

/// The current version of the storage
const VERSION: &'static str = "penumbra-reindexer-archive-v1";

async fn create_pool(path: Option<&Path>) -> anyhow::Result<SqlitePool> {
    let url = match path {
        None => "sqlite://:memory:".to_string(),
        Some(path) => {
            format!(
                "sqlite://{}",
                path.to_str()
                    .ok_or(anyhow!("unable to convert database path to UTF-8"))?
            )
        }
    };
    let options = SqliteConnectOptions::from_str(&url)?
        .create_if_missing(true)
        // This is ok because we only write during archival, and if you crash: rearchive
        .synchronous(sqlx::sqlite::SqliteSynchronous::Off);
    SqlitePool::connect_with(options).await.map_err(Into::into)
}

/// Storage used for the archive format.
pub struct Storage {
    pool: SqlitePool,
}

impl Drop for Storage {
    fn drop(&mut self) {
        // This assumes a multi-threaded tokio runtime.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                tracing::debug!("closing archive database");
                self.pool.close().await;
            });
        });
    }
}

impl Storage {
    async fn init(&self, chain_id: Option<&str>) -> anyhow::Result<()> {
        async fn create_tables(pool: &SqlitePool) -> anyhow::Result<()> {
            tracing::debug!("creating archive tables");
            sqlx::query(
                r#"CREATE TABLE IF NOT EXISTS metadata (
                    id INTEGER PRIMARY KEY CHECK (id = 0),
                    version TEXT NOT NULL UNIQUE,
                    chain_id TEXT NOT NULL UNIQUE
                );"#,
            )
            .execute(pool)
            .await?;

            // This table exists to store large blobs outside of tables.
            // This allows us to scan, e.g. for querying the max height,
            // without having to traverse the big blobs.
            sqlx::query(
                r#"CREATE TABLE IF NOT EXISTS blobs (
                    data BLOB NOT NULL
                )
                "#,
            )
            .execute(pool)
            .await?;

            sqlx::query(
                r#"CREATE TABLE IF NOT EXISTS blocks (
                    height INTEGER NOT NULL PRIMARY KEY,
                    data_id INTEGER NOT NULL
                )
                "#,
            )
            .execute(pool)
            .await?;

            // For efficient joins between blocks and the data inside.
            sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_blocks_data_id ON blocks(data_id)")
                .execute(pool)
                .await?;

            sqlx::query(
                r#"CREATE TABLE IF NOT EXISTS geneses (
                    initial_height INTEGER NOT NULL PRIMARY KEY,
                    data_id INTEGER NOT NULL
                )
                "#,
            )
            .execute(pool)
            .await?;

            // For efficient joins between geneses and the data inside.
            sqlx::query(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_geneses_data_id ON geneses(data_id)",
            )
            .execute(pool)
            .await?;

            Ok(())
        }

        /// Attempt to populate metadata, failing on version mismatches.
        async fn populate_metadata(
            pool: &SqlitePool,
            chain_id: Option<&str>,
        ) -> anyhow::Result<()> {
            let existing_metadata: Option<(String, String)> =
                sqlx::query_as("SELECT version, chain_id FROM metadata")
                    .fetch_optional(pool)
                    .await?;
            // The chain id is only None when we're reading the database with no intention
            // to populate the chain id, in which case we expect it to already have been
            // initialized.
            if chain_id.is_none() && existing_metadata.is_none() {
                anyhow::bail!("expected archive database to already be initialized");
            }
            match existing_metadata {
                Some((version, archive_chain_id)) => {
                    anyhow::ensure!(
                        version == VERSION,
                        "expected version '{}' found '{}'",
                        VERSION,
                        version
                    );
                    if let Some(chain_id) = chain_id {
                        anyhow::ensure!(
                            archive_chain_id == chain_id,
                            "expected chain_id '{}' found '{}'",
                            chain_id,
                            archive_chain_id
                        );
                    }
                }
                None => {
                    sqlx::query("INSERT INTO metadata (id, version, chain_id) VALUES (0, ?, ?)")
                        .bind(VERSION)
                        .bind(chain_id)
                        .execute(pool)
                        .await?;
                }
            }

            Ok(())
        }

        create_tables(&self.pool).await?;
        populate_metadata(&self.pool, chain_id).await?;

        Ok(())
    }

    /// Create a new storage instance.
    #[tracing::instrument(skip_all)]
    pub async fn new(
        path: Option<&dyn AsRef<Path>>,
        chain_id: Option<&str>,
    ) -> anyhow::Result<Self> {
        let path = path.map(|x| x.as_ref());
        tracing::debug!(
            path = path.map(|x| x.to_string_lossy().to_string()),
            "initializing archive database"
        );
        let out = Self {
            pool: create_pool(path).await?,
        };

        out.init(chain_id).await?;

        Ok(out)
    }

    /// The version of the storage.
    ///
    /// Different versions will be incompatible, requiring a data migration.
    #[cfg(test)]
    pub async fn version(&self) -> anyhow::Result<String> {
        let (out,) = sqlx::query_as("SELECT version FROM metadata")
            .fetch_one(&self.pool)
            .await?;
        Ok(out)
    }

    /// Get the chain id embedded in this archive format.
    pub async fn chain_id(&self) -> anyhow::Result<String> {
        let (out,) = sqlx::query_as("SELECT chain_id FROM metadata")
            .fetch_one(&self.pool)
            .await?;
        Ok(out)
    }

    /// Put a block into storage.
    ///
    /// This will fail if a block at that height already exists.
    pub async fn put_block(&self, block: &Block) -> anyhow::Result<()> {
        let height = block.height();

        let mut tx = self.pool.begin().await?;

        let exists: Option<_> = sqlx::query("SELECT 1 FROM blocks WHERE height = ?")
            .bind(i64::try_from(height)?)
            .fetch_optional(tx.as_mut())
            .await?;
        anyhow::ensure!(
            exists.is_none(),
            "block at height {} already exists",
            height
        );

        let (data_id,): (i64,) =
            sqlx::query_as("INSERT INTO blobs(data) VALUES (?) RETURNING rowid")
                .bind(&block.encode())
                .fetch_one(tx.as_mut())
                .await?;
        sqlx::query("INSERT INTO blocks(height, data_id) VALUES (?, ?)")
            .bind(i64::try_from(height)?)
            .bind(data_id)
            .execute(tx.as_mut())
            .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Put a genesis into storage.
    pub async fn put_genesis(&self, genesis: &Genesis) -> anyhow::Result<()> {
        let initial_height = genesis.initial_height();

        let mut tx = self.pool.begin().await?;

        let exists: Option<_> = sqlx::query("SELECT 1 FROM geneses WHERE initial_height = ?")
            .bind(i64::try_from(initial_height)?)
            .fetch_optional(tx.as_mut())
            .await?;
        if exists.is_some() {
            tracing::info!(
                "genesis with initial_height {} already exists, skipping archival",
                initial_height
            );
            return Ok(());
        }

        let (data_id,): (i64,) =
            sqlx::query_as("INSERT INTO blobs(data) VALUES (?) RETURNING rowid")
                .bind(&genesis.encode()?)
                .fetch_one(tx.as_mut())
                .await?;
        sqlx::query("INSERT INTO geneses(initial_height, data_id) VALUES (?, ?)")
            .bind(i64::try_from(initial_height)?)
            .bind(data_id)
            .execute(tx.as_mut())
            .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Attempt to retrieve a genesis with a given initial height.
    #[allow(dead_code)]
    pub async fn get_genesis(&self, initial_height: u64) -> anyhow::Result<Option<Genesis>> {
        let data: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT (data) FROM geneses JOIN blobs ON data_id = blobs.rowid WHERE initial_height = ?",
        )
        .bind(i64::try_from(initial_height)?)
        .fetch_optional(&self.pool)
        .await?;
        Ok(data.map(|x| Genesis::decode(&x.0)).transpose()?)
    }

    pub async fn genesis_does_exist(&self, initial_height: u64) -> anyhow::Result<bool> {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM geneses WHERE initial_height = ?)")
                .bind(i64::try_from(initial_height)?)
                .fetch_one(&self.pool)
                .await?;
        Ok(exists)
    }

    /// Get a block from storage.
    ///
    /// This will return [Option::None] if there's no such block.
    #[allow(dead_code)]
    pub async fn get_block(&self, height: u64) -> anyhow::Result<Option<Block>> {
        let data: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT (data) FROM blocks JOIN blobs ON data_id = blobs.rowid WHERE height = ?",
        )
        .bind(i64::try_from(height)?)
        .fetch_optional(&self.pool)
        .await?;
        Ok(data.map(|x| Block::decode(&x.0)).transpose()?)
    }

    pub async fn block_does_exist(&self, height: u64) -> anyhow::Result<bool> {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM blocks WHERE height = ?)")
                .bind(i64::try_from(height)?)
                .fetch_one(&self.pool)
                .await?;
        Ok(exists)
    }

    /// Get the highest known block in the storage.
    #[allow(dead_code)]
    pub async fn last_height(&self) -> anyhow::Result<Option<u64>> {
        let height: Option<(i64,)> = sqlx::query_as("SELECT MAX(height) FROM blocks")
            .fetch_optional(&self.pool)
            .await?;
        Ok(height.map(|x| x.0.try_into()).transpose()?)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const CHAIN_ID: &'static str = "penumbra-test";

    #[tokio::test(flavor = "multi_thread")]
    async fn test_storage_can_get_version() -> anyhow::Result<()> {
        assert_eq!(
            Storage::new(None, Some(CHAIN_ID))
                .await?
                .version()
                .await?
                .as_str(),
            VERSION
        );
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_storage_can_get_chain_id() -> anyhow::Result<()> {
        assert_eq!(
            Storage::new(None, Some(CHAIN_ID))
                .await?
                .chain_id()
                .await?
                .as_str(),
            CHAIN_ID
        );
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_put_then_get_block() -> anyhow::Result<()> {
        let in_block = Block::test_value();
        let height = in_block.height();
        let storage = Storage::new(None, Some(CHAIN_ID)).await?;
        storage.put_block(&in_block).await?;
        let out_block = storage.get_block(height).await?;
        assert_eq!(out_block, Some(in_block));
        let last_height = storage.last_height().await?;
        assert_eq!(last_height, Some(height));
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_bad_height_returns_no_block() -> anyhow::Result<()> {
        let storage = Storage::new(None, Some(CHAIN_ID)).await?;
        assert!(storage.get_block(100).await?.is_none());
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_put_twice() -> anyhow::Result<()> {
        let storage = Storage::new(None, Some(CHAIN_ID)).await?;
        let block = Block::test_value();
        storage.put_block(&block).await?;
        assert!(storage.put_block(&block).await.is_err());
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_put_then_get_genesis() -> anyhow::Result<()> {
        let storage = Storage::new(None, Some(CHAIN_ID)).await?;
        let genesis = Genesis::test_value();
        storage.put_genesis(&genesis).await?;
        let out = storage
            .get_genesis(genesis.initial_height())
            .await?
            .ok_or(anyhow!("expected genesis to be present"))?;
        assert_eq!(out.initial_height(), genesis.initial_height());
        Ok(())
    }
}
