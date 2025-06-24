//! Logic to inspect reindexer-generated archives and databases
//! to perform health checks. Useful for validating assumptions
//! about how comprehensive a given archive is in particular, as downstream
//! consumers of raw events databases, such as pindexer, will require
//! every single historical block up to current height.
use sqlx::sqlite::SqlitePool;
use sqlx::PgPool;
use sqlx::{Error, FromRow, Row};
use std::path::Path;

// Allowing dead_code because no logic explicitly reads from the `gap_start` and `gap_end` fields;
// these are used via debug-printing, but debug derivations don't count as live code.
#[allow(dead_code)]
#[derive(Debug)]
/// Representation of a range of missing blocks.
///
/// Used to check that created databases are complete, in that they're fully contiguous:
/// no blocks are absent from the range specified.
pub struct BlockGap {
    /// The first block in the range.
    gap_start: i64,
    /// The last block in the range.
    gap_end: i64,
}

/// Ensure that we can query the sqlite3 db and receive BlockGap results.
impl<'r> FromRow<'r, sqlx::sqlite::SqliteRow> for BlockGap {
    fn from_row(row: &'r sqlx::sqlite::SqliteRow) -> Result<Self, Error> {
        Ok(BlockGap {
            gap_start: row.try_get("gap_start")?,
            gap_end: row.try_get("gap_end")?,
        })
    }
}

/// Ensure that we can query the postgres db and receive BlockGap results.
impl<'r> FromRow<'r, sqlx::postgres::PgRow> for BlockGap {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, Error> {
        Ok(BlockGap {
            gap_start: row.try_get("gap_start")?,
            gap_end: row.try_get("gap_end")?,
        })
    }
}

/// Query the sqlite3 database for total number of `genesis`,
/// and expect that the total number is one greater than the current step.
pub async fn check_num_geneses(reindexer_db_filepath: &Path, step: usize) -> anyhow::Result<()> {
    // Connect to the database
    let pool = SqlitePool::connect(reindexer_db_filepath.to_str().unwrap()).await?;
    let query = sqlx::query("SELECT COUNT(*) FROM geneses;");
    let count: u64 = query.fetch_one(&pool).await?.get(0);
    let expected: u64 = step as u64 + 1;
    if count != expected {
        tracing::error!(
            count,
            expected,
            "expected {} geneses, but found {}",
            expected,
            count
        );
        anyhow::bail!("failed genesis count")
    }
    Ok(())
}

/// Query the sqlite3 database for any missing blocks, defined as `BlockGap`s,
/// and fail if any are found.
pub async fn check_for_gaps_sqlite(reindexer_db_filepath: &Path) -> anyhow::Result<()> {
    // Connect to the database
    let pool = SqlitePool::connect(reindexer_db_filepath.to_str().unwrap()).await?;

    let sql = gaps_query();
    let query = sqlx::query_as::<_, BlockGap>(&sql);
    let results = query.fetch_all(&pool).await?;

    // TODO: read fields to format an error message
    if !results.is_empty() {
        let msg = format!("found missing blocks in the sqlite3 db: {:?}", results);
        tracing::error!(msg);
        anyhow::bail!(msg);
    }
    Ok(())
}

/// Query the postgres database for any missing blocks, defined as `BlockGap`s,
/// and fail if any are found.
pub async fn check_for_gaps_postgres(pg_db_url: String) -> anyhow::Result<()> {
    // Connect to the database
    let pool = PgPool::connect(pg_db_url.as_str()).await?;

    let sql = gaps_query();
    let query = sqlx::query_as::<_, BlockGap>(&sql);
    let results = query.fetch_all(&pool).await?;

    // TODO: read fields to format an error message
    if !results.is_empty() {
        let msg = format!("found missing blocks in the postgres db: {:?}", results);
        tracing::error!(msg);
        anyhow::bail!(msg);
    }
    Ok(())
}

/// Private function for generating SQL that checks for gaps within a database.
fn gaps_query() -> String {
    String::from(
        r#"
    WITH numbered_blocks AS (
        SELECT height,
               LEAD(height) OVER (ORDER BY height) as next_height
        FROM blocks
    )
    SELECT height + 1 as gap_start, next_height - 1 as gap_end
    FROM numbered_blocks
    WHERE next_height - height > 1
    "#,
    )
}

/// Query the sqlite3 database for total number of known blocks.
/// Fail if it doesn't match the expected number of blocks, or
/// 1 less than the expected number. The tolerance is to acknowledge
/// that the sqlite3 db can be 1 block behind the local node state.
pub async fn check_num_blocks_sqlite(
    reindexer_db_filepath: &Path,
    expected: u64,
) -> anyhow::Result<u64> {
    // Connect to the database
    let pool = SqlitePool::connect(reindexer_db_filepath.to_str().unwrap()).await?;
    let query = sqlx::query("SELECT COUNT(*) FROM blocks");
    let count: u64 = query.fetch_one(&pool).await?.get(0);

    if ![expected, expected - 1].contains(&count) {
        let msg = format!(
            "archived blocks count looks wrong; expected: {}, found {}",
            expected, count,
        );
        tracing::error!(msg);
        anyhow::bail!(msg);
    }

    Ok(count)
}

/// Query the postgres database for total number of known blocks.
/// Fail if it doesn't match the expected number of blocks, or
/// 1 less than the expected number. The tolerance is to acknowledge
/// that the postgres db can be 1 block behind the local node state.
pub async fn check_num_blocks_postgres(pg_db_url: String, expected: u64) -> anyhow::Result<u64> {
    // Connect to the database
    let pool = PgPool::connect(pg_db_url.as_str()).await?;
    let query = sqlx::query("SELECT COUNT(*) FROM blocks");
    let count_raw: i64 = query.fetch_one(&pool).await?.get(0);
    let count = count_raw as u64;
    if ![expected, expected - 1].contains(&count) {
        let msg = format!(
            "regenerated blocks count looks wrong; expected: {}, found {}",
            expected, count,
        );
        tracing::error!(msg);
        anyhow::bail!(msg);
    }
    Ok(count)
}
