use tendermint::abci::Event;

/// Represents an indexer for raw ABCI events.
///
/// This will hook into the postgres backend that we expect to see.
pub struct Indexer {}

#[allow(dead_code)]
impl Indexer {
    /// Initialize the indexer with a given database url.
    pub async fn init(_database_url: &str) -> anyhow::Result<Self> {
        Ok(Self {})
    }

    /// Signal the start of a new block.
    ///
    /// This will index whatever information about the block we need, and also
    /// set a context for the current block for subsequent events.
    pub async fn enter_block(&self, height: u64) -> anyhow::Result<()> {
        tracing::debug!(height, "indexing block");
        Ok(())
    }

    /// Signal the delivery of a tx in the application.
    pub async fn enter_tx(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Deliver events, and have them indexed.
    pub async fn events(&self, events: &[Event]) -> anyhow::Result<()> {
        tracing::debug!(?events, "indexing events");
        Ok(())
    }
}
