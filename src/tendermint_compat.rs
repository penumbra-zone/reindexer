//! Custom types for core Tendermint/CometBFT concepts,
//! intended as a stable interface. Versions of Penumbra crates
//! will depend on different versions of `tendermint-*` crates
//! over time, necessitating the use of intermediate custom types.

// TODO: should probably import less-than-current `tendermint` as `old_tendermint`,
// so that the `tendermint::` namespace always refers to current.
use tendermint::{
    abci::Event as OldEvent,
    v0_37::abci::request::{
        BeginBlock as OldBeginBlock, DeliverTx as OldDeliverTx, EndBlock as OldEndBlock,
    },
};

use tendermint_v0o40::{
    abci::Event,
    v0_37::abci::request::{BeginBlock, DeliverTx, EndBlock},
};

use tendermint_v0o40::block::Header;
use tendermint_v0o40::hash::Hash;

pub struct CompatBeginBlock {
    pub begin_block: BeginBlock,
}

impl TryInto<BeginBlock> for CompatBeginBlock {
    type Error = anyhow::Error;
    fn try_into(self) -> anyhow::Result<BeginBlock> {
        Ok(self.begin_block)
    }
}

impl TryInto<OldBeginBlock> for CompatBeginBlock {
    type Error = anyhow::Error;
    fn try_into(self) -> anyhow::Result<OldBeginBlock> {
        let bb: OldBeginBlock = OldBeginBlock {
            hash: tendermint::hash::Hash::Sha256(self.begin_block.hash.as_bytes().try_into()?),
            header: tendermint::block::Header {
                version: tendermint::block::header::Version {
                    // Version is a tuple of u64s, so it's easy to unpack.
                    block: self.begin_block.header.version.block,
                    app: self.begin_block.header.version.app,
                },
                // chain_id is just a string
                chain_id: tendermint::chain::id::Id::try_from(
                    self.begin_block.header.chain_id.into(),
                )?,
                // Height is a u64 inside, so easy enough
                height: tendermint::block::Height::try_from(self.begin_block.header.height.into())?,
                // TODO: should the nanos be 0? am i doubling the evaluted time by summing (time +
                // time-in-nanos)?
                time: tendermint::time::Time::from_unix_timestamp(
                    self.begin_block.header.time.unix_timestamp(),
                    self.begin_block
                        .header
                        .time
                        .unix_timestamp_nanos()
                        .try_into()?,
                )?,
                last_block_id: match self.begin_block.header.last_block_id {
                    Some(last_block_id) => Some(tendermint::block::Id {
                        hash: tendermint::hash::Hash::Sha256(
                            last_block_id.hash.as_bytes().try_into()?,
                        ),
                        part_set_header: tendermint::block::parts::Header::new(
                            last_block_id.part_set_header.total,
                            tendermint::hash::Hash::Sha256(
                                last_block_id.hash.as_bytes().try_into()?,
                            ),
                        )?,
                    }),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                last_commit_hash: match self.begin_block.header.last_commit_hash {
                    Some(last_commit_hash) => Some(tendermint::hash::Hash::Sha256(
                        last_commit_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                data_hash: match self.begin_block.header.data_hash {
                    Some(data_hash) => Some(tendermint::hash::Hash::Sha256(
                        data_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Round-trip as bytes
                validators_hash: tendermint::hash::Hash::Sha256(
                    self.begin_block
                        .header
                        .validators_hash
                        .as_bytes()
                        .try_into()?,
                ),
                // Round-trip as bytes
                next_validators_hash: tendermint::hash::Hash::Sha256(
                    self.begin_block
                        .header
                        .next_validators_hash
                        .as_bytes()
                        .try_into()?,
                ),
                // Round-trip as bytes
                consensus_hash: tendermint::hash::Hash::Sha256(
                    self.begin_block
                        .header
                        .consensus_hash
                        .as_bytes()
                        .try_into()?,
                ),
                // Round-trip as bytes
                app_hash: tendermint::hash::AppHash::try_from(
                    self.begin_block.header.app_hash.as_bytes().try_into()?,
                )?,
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                last_results_hash: match self.begin_block.header.last_results_hash {
                    Some(last_results_hash) => Some(tendermint::hash::Hash::Sha256(
                        last_results_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                evidence_hash: match self.begin_block.header.evidence_hash {
                    Some(evidence_hash) => Some(tendermint::hash::Hash::Sha256(
                        evidence_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Round-trip as bytes.
                proposer_address: tendermint::account::Id::new(
                    self.begin_block
                        .header
                        .proposer_address
                        .as_bytes()
                        .try_into()?,
                ),
            },
            last_commit_info: tendermint::abci::types::CommitInfo {
                // Round is a u32, simple to convert.
                round: self.begin_block.last_commit_info.round.value().try_into()?,
                votes: unimplemented!(),
            },
            byzantine_validators: unimplemented!(),
        };
        Ok(bb)
    }
}

pub struct CompatEndBlock {}
pub struct CompatDeliverTx {}
