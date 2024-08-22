use async_trait::async_trait;

use crate::cometbft::Genesis;

struct Penumbra {}

#[async_trait]
impl super::Penumbra for Penumbra {
    async fn genesis(&self, _genesis: Genesis) -> anyhow::Result<()> {
        todo!()
    }
}
