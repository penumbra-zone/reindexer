#![cfg(feature = "expensive-tests")]
use anyhow::anyhow;
use penumbra_reindexer::{storage::Storage, tendermint_compat};
use std::str::FromStr as _;

#[tokio::test(flavor = "multi_thread")]
async fn test_begin_block_parsing() -> anyhow::Result<()> {
    use std::path::PathBuf;

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

    let args = Args::parse()?;
    let archive = Storage::new(Some(&args.archive_file), None).await?;
    let mut height = 1u64;
    while let Some(block) = archive.get_block(height).await? {
        let block = tendermint_compat::Block::try_from(block)?;
        let begin_block = tendermint_compat::BeginBlock::from(block);
        let _begin_block_v0o34: tendermint_v0o34::abci::request::BeginBlock =
            begin_block.clone().try_into()?;
        let _begin_block_v0o40: tendermint_v0o40::abci::request::BeginBlock = begin_block.into();
        height += 1;
    }

    Ok(())
}
