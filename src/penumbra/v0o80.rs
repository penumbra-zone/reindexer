use std::path::Path;

use async_trait::async_trait;
use cnidarium_v0o80::Storage;
use penumbra_app_v0o80::{app::App, PenumbraHost, SUBSTORE_PREFIXES};
use penumbra_ibc_v0o80::component::HostInterface as _;

use tendermint_v0o34 as tendermint;

use crate::cometbft::Genesis;
use crate::tendermint_compat::{BeginBlock, DeliverTx, EndBlock, Event};

pub struct Penumbra {
    storage: Storage,
    app: App,
}

impl Penumbra {
    pub async fn load(working_dir: &Path) -> anyhow::Result<Self> {
        let storage = Storage::load(working_dir.to_owned(), SUBSTORE_PREFIXES.to_vec()).await?;
        let app = App::new(storage.latest_snapshot());
        Ok(Self { storage, app })
    }
}

#[async_trait]
impl super::Penumbra for Penumbra {
    async fn release(self: Box<Self>) {
        self.storage.release().await
    }

    async fn genesis(&mut self, genesis: Genesis) -> anyhow::Result<()> {
        self.app
            .init_chain(&serde_json::from_value(genesis.app_state().clone())?)
            .await;
        Ok(())
    }

    async fn metadata(&self) -> anyhow::Result<(u64, String)> {
        let snapshot = self.storage.latest_snapshot();
        let height = PenumbraHost::get_block_height(snapshot.clone()).await?;
        let chain_id = PenumbraHost::get_chain_id(snapshot).await?;
        Ok((height, chain_id))
    }

    async fn begin_block(&mut self, req: &BeginBlock) -> Vec<Event> {
        let compat_block: tendermint::abci::request::BeginBlock = req.clone().try_into().unwrap();
        self.app
            .begin_block(&compat_block)
            .await
            .into_iter()
            .map(|e| e.try_into().unwrap())
            .collect()
    }

    async fn deliver_tx(&mut self, req: &DeliverTx) -> anyhow::Result<Vec<Event>> {
        let compat_tx: tendermint::abci::request::DeliverTx = req.clone().into();
        let events = self.app.deliver_tx_bytes(&compat_tx.tx).await?;
        Ok(events.into_iter().map(|e| e.try_into().unwrap()).collect())
    }

    async fn end_block(&mut self, req: &EndBlock) -> Vec<Event> {
        let compat_block: tendermint::abci::request::EndBlock = req.clone().into();
        self.app
            .end_block(&compat_block)
            .await
            .into_iter()
            .map(|e| e.try_into().unwrap())
            .collect()
    }

    async fn commit(&mut self) -> anyhow::Result<super::RootHash> {
        Ok(self.app.commit(self.storage.clone()).await.0)
    }
}

mod migration {
    use cnidarium_v0o80::{Snapshot, StateDelta};
    use ibc_types::core::channel::{Packet, PortId};
    use ibc_types::transfer::acknowledgement::TokenTransferAcknowledgement;
    use penumbra_app_v0o79::SUBSTORE_PREFIXES;
    use penumbra_app_v0o80::app::StateReadExt as _;
    use penumbra_governance_v0o80::StateWriteExt;
    use penumbra_ibc_v0o80::{component::ChannelStateWriteExt as _, IbcRelay};
    use penumbra_sct_v0o80::component::clock::{EpochManager as _, EpochRead as _};
    use penumbra_transaction_v0o80::{Action, Transaction};

    use super::super::Version;
    use super::*;

    /// The block where proposal #2 passed, enabling outbound ICS20 transfers.
    const ICS20_TRANSFER_START_HEIGHT: u64 = 411616;

    /// Find all of the lost transfers inside of a transaction.
    ///
    /// In other words, look for relayed packet acknowledgements relating to ICS20 transfers containing an error.
    /// These packets were not correctly handled, being deleted when the ack had an error,
    /// as if the ack were successful.
    fn tx_lost_transfers(transaction: Transaction) -> impl Iterator<Item = Packet> {
        transaction
            .transaction_body()
            .actions
            .into_iter()
            .filter_map(move |action| match action {
                Action::IbcRelay(IbcRelay::Acknowledgement(m)) => {
                    // Make sure we're only looking at ICS20 related packets
                    if m.packet.port_on_b != PortId::transfer() {
                        return None;
                    }
                    // This shouldn't fail to parse, because the transaction wouldn't have been
                    // included otherwise, but if for some reason it doesn't, ignore it.
                    let transfer: TokenTransferAcknowledgement =
                        match serde_json::from_slice(m.acknowledgement.as_slice()) {
                            Err(_) => return None,
                            Ok(x) => x,
                        };
                    // If the ack was successful, then that packet was correctly handled, so don't
                    // consider it.
                    match transfer {
                        TokenTransferAcknowledgement::Success(_) => None,
                        TokenTransferAcknowledgement::Error(_) => Some(m.packet),
                    }
                }
                _ => None,
            })
    }

    /// Retrieve all the packets resulting in a locked transfer because of error acks.
    ///
    /// This does so by looking at all transactions, looking for the relayed acknowledgements.
    async fn lost_transfers(state: &StateDelta<Snapshot>) -> anyhow::Result<Vec<Packet>> {
        let mut out = Vec::new();
        let end_height = state.get_block_height().await?;
        // We only need to start from the height where transfers were enabled via governance.
        for height in ICS20_TRANSFER_START_HEIGHT..=end_height {
            let transactions = state.transactions_by_height(height).await?.transactions;
            for tx in transactions.into_iter() {
                for lost in tx_lost_transfers(tx.try_into()?) {
                    out.push(lost);
                }
            }
        }
        Ok(out)
    }

    /// Replace all the packets that were erroneously removed from the state.
    async fn replace_lost_packets(delta: &mut StateDelta<Snapshot>) -> anyhow::Result<()> {
        let lost_packets = lost_transfers(delta).await?;
        for packet in lost_packets {
            // This will undo what happens in https://github.com/penumbra-zone/penumbra/blob/882a061bd69ce14b01711041bbc0c0ce209e2823/crates/core/component/ibc/src/component/msg_handler/acknowledgement.rs#L99.
            delta.put_packet_commitment(&packet);
        }
        Ok(())
    }

    pub async fn migrate(from: Version, working_dir: &Path) -> anyhow::Result<()> {
        anyhow::ensure!(from == Version::V0o79, "version must be v0.79.x");
        let storage = Storage::load(working_dir.to_owned(), SUBSTORE_PREFIXES.to_vec()).await?;
        let initial_state = storage.latest_snapshot();
        let mut delta = StateDelta::new(initial_state);

        // Reinsert all of the erroneously removed packets
        replace_lost_packets(&mut delta).await?;

        // Reset the application height and halt flag.
        delta.ready_to_start();
        delta.put_block_height(0u64);

        // Finally, commit the changes to the chain state.
        let post_upgrade_root_hash = storage.commit_in_place(delta).await?;
        tracing::info!(?post_upgrade_root_hash, "post-migration root hash");
        storage.release().await;

        Ok(())
    }
}

pub use migration::migrate;
