#![cfg(feature = "expensive-tests")]

#[path = "common/mod.rs"]
mod common;

use penumbra_reindexer::{storage::Storage, tendermint_compat};
use std::path::PathBuf;
use std::str::FromStr as _;

#[tokio::test(flavor = "multi_thread")]
/// Ensure that the `BeginBlock` struct can be parsed from existing
/// archived blocks. Exercises the conversion between multiple
/// Tendermint crate versions, for backwards compatibility with historical chain data.
async fn test_begin_block_parsing() -> anyhow::Result<()> {
    penumbra_reindexer::Opt::init_console_tracing();
    struct Args {
        archive_file: PathBuf,
    }

    impl Args {
        fn parse() -> anyhow::Result<Self> {
            // We use an env var rather than a CLI arg so that different invocations of
            // `cargo test` don't break the ordinal arg parsing.
            let env_var = "REINDEXER_SQLITE_DB_FILEPATH";
            match std::env::var(env_var) {
                Ok(f) => {
                    let archive_file = PathBuf::from_str(&f)?;
                    Ok(Self { archive_file })
                }
                Err(_) => anyhow::bail!("env var '{}' not set", env_var),
            }
        }
    }

    let args = match Args::parse() {
        Ok(x) if std::fs::exists(&x.archive_file)? => x,
        Ok(_) | Err(_) => {
            eprintln!("WARNING: failed to parse arguments, or the archive file doesn't exist. Skipping test.");
            return Ok(());
        }
    };
    tracing::info!(
        "running beginblock against local sqlite3 db: {}",
        &args.archive_file.display()
    );

    let archive = Storage::new(Some(&args.archive_file), None).await?;
    let mut height = 1u64;
    while let Some(block) = archive.get_block(height).await? {
        let block = tendermint_compat::Block::try_from(block)?;
        let begin_block = tendermint_compat::BeginBlock::from(block);
        let _begin_block_v0o34: tendermint_v0o34::abci::request::BeginBlock =
            begin_block.clone().try_into()?;
        let _begin_block_v0o40: tendermint_v0o40::abci::request::BeginBlock = begin_block.into();
        height += 1;
        // This loop is fast: approximately 100,000 blocks per 5s.
        if height % 100000 == 0 {
            tracing::info!("processed {} blocks from local sqlite3 fixture", height);
        }
    }

    Ok(())
}
