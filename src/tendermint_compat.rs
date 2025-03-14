#![allow(dead_code, unused_imports)]
//! Custom types for core Tendermint/CometBFT concepts, intended as a stable interface.
//! Versions of Penumbra crates will depend on different versions of `tendermint-*` crates
//! over time, necessitating the use of intermediate custom types.
//!
//! These custom types are used in the definition of the `Penumbra` trait. Each custom wrapper
//! type must support TryFrom and TryInto conversions for all required `tendermint` crates.
//! As of now, there are two versions of Tendermint crates in play:
//!
//!   * tendermint 0.34.x
//!   * tendermint 0.40.x
//!
//! This means that a custom wrapper type like `Block` must implement:
//!
//!   1. TryFrom<tendermint_v0o34::block::Block>
//!   2. TryInto<tendermint_v0o34::block::Block>
//!   3. TryFrom<tendermint_v0o40::block::Block>
//!   4. TryInto<tendermint_v0o40::block::Block>
//!
//! In the future, as the Penumbra protocol crates bump the tendermint crates further,
//! we'll need to update the `reindexer` compat modules to accommodate.

use anyhow::Context;

/// Wrapper type for handling conversions between incompatible versions of Tendermint ABCI
/// `Event`s.
#[derive(Clone, Debug)]
pub struct Event {
    /// Human-readable type for Event.
    pub kind: String,
    /// Representation of an `EventAttribute`, the fields of which map to <String, String, bool>.
    /// Storing raw bytes to defer UTF-8 decoding.
    pub attributes: Vec<(Vec<u8>, Vec<u8>, bool)>,
}

impl From<Event> for tendermint_proto::abci::Event {
    fn from(val: Event) -> Self {
        tendermint_proto::abci::Event {
            attributes: val
                .attributes
                .into_iter()
                .map(|(k, v, i)| tendermint_proto::abci::EventAttribute {
                    key: String::from_utf8_lossy(&k).to_string(),
                    value: String::from_utf8_lossy(&v).to_string(),
                    index: i,
                })
                .collect(),
            r#type: val.kind,
        }
    }
}

impl TryFrom<tendermint_v0o40::abci::Event> for Event {
    type Error = anyhow::Error;
    fn try_from(event: tendermint_v0o40::abci::Event) -> anyhow::Result<Event> {
        Ok(Event {
            kind: event.kind,
            attributes: event
                .attributes
                .into_iter()
                .map(|attribute| {
                    // Newer versions of the Tendermint crate wrap the EventAttribute in an Enum,
                    // for backwards-compat. In the context of Penumbra chain data, we only expect
                    // the newer of the two formats.
                    match attribute {
                        tendermint_v0o40::abci::EventAttribute::V037(x) => {
                            let key = x.key.as_bytes().to_vec();
                            let value = x.value.as_bytes().to_vec();
                            let index = x.index;
                            (key, value, index)
                        }
                        // But, let's be permissive in what we accept.
                        tendermint_v0o40::abci::EventAttribute::V034(x) => {
                            let key = x.key;
                            let value = x.value;
                            let index = x.index;
                            (key, value, index)
                        }
                    }
                })
                .collect(),
        })
    }
}

impl TryFrom<tendermint_v0o34::abci::Event> for Event {
    type Error = anyhow::Error;
    fn try_from(event: tendermint_v0o34::abci::Event) -> anyhow::Result<Event> {
        Ok(Event {
            kind: event.kind.clone(),
            // There's no Enum for the older-style Tendermint event attribute, so we can
            // just unpack the attribute directly.
            attributes: event
                .attributes
                .clone()
                .into_iter()
                .map(|x| {
                    let a = x.key.as_bytes().to_vec();
                    let b = x.value.as_bytes().to_vec();
                    let c = x.index;
                    (a, b, c)
                })
                .collect(),
        })
    }
}

/// Wrapper type for handling conversions between incompatible versions of Tendermint `Block`
/// types. Stores the most recent Tendermint version as a singleton, and defers conversions to
/// TryInto impls.
#[derive(Clone, Debug)]
pub struct Block(tendermint_v0o40::Block);

/// Provide for conversions from 0.40.x tendermint block types.
impl TryFrom<tendermint_v0o40::Block> for Block {
    type Error = anyhow::Error;

    fn try_from(block: tendermint_v0o40::Block) -> Result<Self, Self::Error> {
        Ok(Self(block))
    }
}

/// Provide for conversions from 0.40.x tendermint block types.
impl From<Block> for tendermint_v0o40::Block {
    fn from(val: Block) -> Self {
        val.0
    }
}

impl TryFrom<crate::cometbft::Block> for Block {
    type Error = anyhow::Error;

    fn try_from(value: crate::cometbft::Block) -> Result<Self, Self::Error> {
        Self::try_from(value.tendermint()?)
    }
}

/*
impl TryFrom<tendermint_v0o34::Block> for Block {
    type Error = anyhow::Error;
    fn try_from(block: tendermint_v0o34::Block) -> anyhow::Result<Block> {
        let block = Block(tendermint_v0o40::Block::new(
            tendermint_v0o40::block::Header {
                version: tendermint_v0o40::block::header::Version {
                    // Version is a tuple of u64s, so it's easy to unpack.
                    block: block.header.version.block,
                    app: block.header.version.app,
                },
                // chain_id is just a string
                chain_id: tendermint_v0o40::chain::id::Id::try_from(
                    block.header.chain_id.as_str(),
                )?,
                // Height is a u64 inside, so easy enough
                height: tendermint_v0o40::block::Height::try_from(block.header.height.value())?,
                // TODO: should the nanos be 0? am i doubling the evaluted time by summing (time +
                // time-in-nanos)?
                time: tendermint_v0o40::time::Time::from_unix_timestamp(
                    block.header.time.unix_timestamp(),
                    block.header.time.unix_timestamp_nanos().try_into()?,
                )?,
                last_block_id: match block.header.last_block_id {
                    Some(last_block_id) => Some(tendermint_v0o40::block::Id {
                        hash: tendermint_v0o40::hash::Hash::Sha256(
                            last_block_id.hash.as_bytes().try_into()?,
                        ),
                        part_set_header: tendermint_v0o40::block::parts::Header::new(
                            last_block_id.part_set_header.total,
                            tendermint_v0o40::hash::Hash::Sha256(
                                last_block_id.hash.as_bytes().try_into()?,
                            ),
                        )?,
                    }),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                last_commit_hash: match block.header.last_commit_hash {
                    Some(last_commit_hash) => Some(tendermint_v0o40::hash::Hash::Sha256(
                        last_commit_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                data_hash: match block.header.data_hash {
                    Some(data_hash) => Some(tendermint_v0o40::hash::Hash::Sha256(
                        data_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Round-trip as bytes
                validators_hash: tendermint_v0o40::hash::Hash::Sha256(
                    block.header.validators_hash.as_bytes().try_into()?,
                ),
                // Round-trip as bytes
                next_validators_hash: tendermint_v0o40::hash::Hash::Sha256(
                    block.header.next_validators_hash.as_bytes().try_into()?,
                ),
                // Round-trip as bytes
                consensus_hash: tendermint_v0o40::hash::Hash::Sha256(
                    block.header.consensus_hash.as_bytes().try_into()?,
                ),
                // Round-trip as bytes
                app_hash: tendermint_v0o40::hash::AppHash::try_from(
                    block.header.app_hash.as_bytes().to_vec(),
                )?,
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                last_results_hash: match block.header.last_results_hash {
                    Some(last_results_hash) => Some(tendermint_v0o40::hash::Hash::Sha256(
                        last_results_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                evidence_hash: match block.header.evidence_hash {
                    Some(evidence_hash) => Some(tendermint_v0o40::hash::Hash::Sha256(
                        evidence_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Round-trip as bytes.
                proposer_address: tendermint_v0o40::account::Id::new(
                    block.header.proposer_address.as_bytes().try_into()?,
                ),
            },
            // data
            block.data.into_iter().collect(),
            // TODO: need to unpack a compcliated evidence List and match its enums
            tendermint_v0o40::evidence::List::new(vec![]),
            match block.last_commit {
                None => None,
                Some(last_commit) => Some(tendermint_v0o40::block::Commit {
                    height: tendermint_v0o40::block::Height::try_from(last_commit.height.value())?,
                    round: tendermint_v0o40::block::Round::try_from(last_commit.round.value())?,
                    block_id: tendermint_v0o40::block::Id {
                        hash: match last_commit.block_id.hash {
                            tendermint_v0o34::Hash::Sha256(h) => tendermint_v0o40::Hash::Sha256(h),
                            tendermint_v0o34::Hash::None => tendermint_v0o40::Hash::None,
                        },

                        part_set_header: tendermint_v0o40::block::parts::Header::new(
                            last_commit.block_id.part_set_header.total,
                            //                            c.block_id.part_set_header.hash.into(),
                            match last_commit.block_id.part_set_header.hash {
                                tendermint_v0o34::Hash::Sha256(h) => {
                                    tendermint_v0o40::Hash::Sha256(h)
                                }
                                tendermint_v0o34::Hash::None => tendermint_v0o40::Hash::None,
                            },
                        )?,
                    },
                    signatures: last_commit
                        .signatures
                        .iter()
                        .map(|s| match s {
                            tendermint_v0o34::block::commit_sig::CommitSig::BlockIdFlagAbsent => {
                                tendermint_v0o40::block::commit_sig::CommitSig::BlockIdFlagAbsent
                            }
                            tendermint_v0o34::block::commit_sig::CommitSig::BlockIdFlagCommit {
                                ref validator_address,
                                ref timestamp,
                                ref signature,
                            } => {
                                tendermint_v0o40::block::commit_sig::CommitSig::BlockIdFlagCommit {
                                    validator_address: tendermint_v0o40::account::Id::new(
                                        validator_address.as_bytes().try_into().unwrap(),
                                    ),
                                    timestamp: tendermint_v0o40::time::Time::from_unix_timestamp(
                                        timestamp.unix_timestamp(),
                                        timestamp
                                            .unix_timestamp_nanos()
                                            .try_into()
                                            .expect("failed to convert timestamp"),
                                    )
                                    .unwrap(),
                                    signature: match signature {
                                        Some(s2) => tendermint_v0o40::signature::Signature::new(
                                            s2.as_bytes(),
                                        )
                                        .unwrap(),
                                        None => None,
                                    },
                                }
                            }
                            tendermint_v0o34::block::commit_sig::CommitSig::BlockIdFlagNil {
                                ref validator_address,
                                ref timestamp,
                                ref signature,
                            } => tendermint_v0o40::block::commit_sig::CommitSig::BlockIdFlagNil {
                                validator_address: tendermint_v0o40::account::Id::new(
                                    validator_address.as_bytes().try_into().unwrap(),
                                ),
                                timestamp: tendermint_v0o40::time::Time::from_unix_timestamp(
                                    timestamp.unix_timestamp(),
                                    timestamp
                                        .unix_timestamp_nanos()
                                        .try_into()
                                        .expect("failed to convert timestamp"),
                                )
                                .unwrap(),
                                signature: match signature {
                                    Some(s2) => {
                                        tendermint_v0o40::signature::Signature::new(s2.as_bytes())
                                            .unwrap()
                                    }
                                    None => None,
                                },
                            },
                        })
                        .collect(),
                }),
            },
        ));
        Ok(block)
    }
}
*/

/// Wrapper type for handling conversions between incompatible versions of Tendermint `BeginBlock`
/// types. Stores the most recent Tendermint version as a singleton, and defers conversions to
/// TryInto impls.
#[derive(Clone, Debug)]
pub struct BeginBlock(tendermint_v0o40::abci::request::BeginBlock);

/// Convenience conversion from `Block` to `BeginBlock`
impl From<Block> for BeginBlock {
    fn from(val: Block) -> Self {
        use tendermint_v0o40::{
            abci::types::{Misbehavior, MisbehaviorKind},
            evidence::Evidence,
        };

        fn make_validator(
            address: tendermint_v0o40::account::Id,
            power: tendermint_v0o40::vote::Power,
        ) -> tendermint_v0o40::abci::types::Validator {
            tendermint_v0o40::abci::types::Validator {
                address: address
                    .as_bytes()
                    .try_into()
                    .expect("address should be the right size"),
                power,
            }
        }

        fn evidence_to_misbehavior(evidence: &Evidence) -> Vec<Misbehavior> {
            match evidence {
                Evidence::DuplicateVote(bad) => vec![Misbehavior {
                    kind: MisbehaviorKind::DuplicateVote,
                    validator: make_validator(bad.vote_a.validator_address, bad.validator_power),
                    height: bad.vote_a.height,
                    time: bad.timestamp,
                    total_voting_power: bad.total_voting_power,
                }],
                // I'm really not sure if this is correct, but seems logical?
                Evidence::LightClientAttack(bad) => bad
                    .byzantine_validators
                    .iter()
                    .map(|v| Misbehavior {
                        kind: MisbehaviorKind::LightClientAttack,
                        validator: make_validator(v.address, v.power),
                        height: bad.common_height,
                        time: bad.timestamp,
                        total_voting_power: bad.total_voting_power,
                    })
                    .collect(),
            }
        }
        BeginBlock(tendermint_v0o40::abci::request::BeginBlock {
            hash: val.0.header.hash(),
            header: val.0.header.clone(),
            // last_commit_info: commit_to_info(self.0.last_commit.as_ref()),
            last_commit_info: match val.0.last_commit {
                None => tendermint_v0o40::abci::types::CommitInfo {
                    round: Default::default(),
                    votes: Default::default(),
                },
                Some(commit) => tendermint_v0o40::abci::types::CommitInfo {
                    round: commit.round,
                    votes: commit
                        .signatures
                        .iter()
                        .filter_map(|s| match s {
                            tendermint_v0o40::block::commit_sig::CommitSig::BlockIdFlagAbsent => {
                                None
                            }
                            tendermint_v0o40::block::commit_sig::CommitSig::BlockIdFlagCommit {
                                validator_address,
                                ..
                            } => Some(tendermint_v0o40::abci::types::VoteInfo {
                                // DRAGON: we assume that the penumbra logic will not care about the power
                                // we declare here.
                                // validator: make_validator(*validator_address, Default::default()),
                                validator: tendermint_v0o40::abci::types::Validator {
                                    address: validator_address.as_bytes().try_into().ok()?,
                                    power: 1u32.into(),
                                },
                                sig_info: tendermint_v0o40::abci::types::BlockSignatureInfo::Flag(
                                    tendermint_v0o40::block::BlockIdFlag::Commit,
                                ),
                            }),
                            tendermint_v0o40::block::commit_sig::CommitSig::BlockIdFlagNil {
                                validator_address,
                                ..
                            } => Some(tendermint_v0o40::abci::types::VoteInfo {
                                // DRAGON: we assume that the penumbra logic will not care about the power
                                // we declare here.
                                // validator: make_validator(*validator_address, Default::default()),
                                validator: tendermint_v0o40::abci::types::Validator {
                                    address: validator_address.as_bytes().try_into().ok()?,
                                    power: 1u32.into(),
                                },
                                sig_info: tendermint_v0o40::abci::types::BlockSignatureInfo::Flag(
                                    tendermint_v0o40::block::BlockIdFlag::Nil,
                                ),
                            }),
                        })
                        .collect(),
                },
            },
            byzantine_validators: val
                .0
                .evidence
                .iter()
                .flat_map(evidence_to_misbehavior)
                .collect(),
        })
    }
}

/// Convenience conversion for extracting the inner value.
impl From<BeginBlock> for tendermint_v0o40::abci::request::BeginBlock {
    fn from(val: BeginBlock) -> Self {
        val.0
    }
}

/// Fallible conversion from the current BeginBlock spec to an older version.
/// Unsure if this is actually useful in reindexer: do we only need TryFrom older blocks?
impl TryInto<tendermint_v0o34::abci::request::BeginBlock> for BeginBlock {
    type Error = anyhow::Error;
    fn try_into(self) -> anyhow::Result<tendermint_v0o34::abci::request::BeginBlock> {
        let bb = tendermint_v0o34::abci::request::BeginBlock {
            hash: tendermint_v0o34::hash::Hash::try_from(self.0.hash.as_bytes().to_vec())?,
            header: tendermint_v0o34::block::Header {
                version: tendermint_v0o34::block::header::Version {
                    // Version is a tuple of u64s, so it's easy to unpack.
                    block: self.0.header.version.block,
                    app: self.0.header.version.app,
                },
                // chain_id is just a string
                chain_id: tendermint_v0o34::chain::id::Id::try_from(
                    self.0.header.chain_id.as_str(),
                )?,
                // Height is a u64 inside, so easy enough
                height: tendermint_v0o34::block::Height::try_from(self.0.header.height.value())?,
                // TODO: should the nanos be 0? am i doubling the evaluted time by summing (time +
                // time-in-nanos)?
                time: tendermint_v0o34::time::Time::from_unix_timestamp(
                    self.0.header.time.unix_timestamp(),
                    (self.0.header.time.unix_timestamp_nanos() % 1_000_000_000).try_into()?,
                )?,
                last_block_id: match self.0.header.last_block_id {
                    Some(last_block_id) => Some(tendermint_v0o34::block::Id {
                        hash: tendermint_v0o34::hash::Hash::try_from(
                            last_block_id.hash.as_bytes().to_vec(),
                        )?,
                        part_set_header: tendermint_v0o34::block::parts::Header::new(
                            last_block_id.part_set_header.total,
                            tendermint_v0o34::hash::Hash::try_from(
                                last_block_id.hash.as_bytes().to_vec(),
                            )?,
                        )?,
                    }),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                last_commit_hash: match self.0.header.last_commit_hash {
                    Some(last_commit_hash) => Some(tendermint_v0o34::hash::Hash::try_from(
                        last_commit_hash.as_bytes().to_vec(),
                    )?),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                data_hash: match self.0.header.data_hash {
                    Some(data_hash) => Some(tendermint_v0o34::hash::Hash::try_from(
                        data_hash.as_bytes().to_vec(),
                    )?),
                    None => None,
                },
                // Round-trip as bytes
                validators_hash: tendermint_v0o34::hash::Hash::try_from(
                    self.0.header.validators_hash.as_bytes().to_vec(),
                )?,
                // Round-trip as bytes
                next_validators_hash: tendermint_v0o34::hash::Hash::try_from(
                    self.0.header.next_validators_hash.as_bytes().to_vec(),
                )?,
                // Round-trip as bytes
                consensus_hash: tendermint_v0o34::hash::Hash::try_from(
                    self.0.header.consensus_hash.as_bytes().to_vec(),
                )?,
                // Round-trip as bytes
                app_hash: tendermint_v0o34::hash::AppHash::try_from(
                    self.0.header.app_hash.as_bytes().to_vec(),
                )?,
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                last_results_hash: match self.0.header.last_results_hash {
                    Some(last_results_hash) => Some(tendermint_v0o34::hash::Hash::try_from(
                        last_results_hash.as_bytes().to_vec(),
                    )?),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                evidence_hash: match self.0.header.evidence_hash {
                    Some(evidence_hash) => Some(tendermint_v0o34::hash::Hash::try_from(
                        evidence_hash.as_bytes().to_vec(),
                    )?),
                    None => None,
                },
                // Round-trip as bytes.
                proposer_address: tendermint_v0o34::account::Id::new(
                    self.0.header.proposer_address.as_bytes().try_into()?,
                ),
            },
            last_commit_info: tendermint_v0o34::abci::types::CommitInfo {
                // Round is a u32, simple to convert.
                round: self.0.last_commit_info.round.value().try_into()?,
                votes: self
                    .0
                    .last_commit_info
                    .votes
                    .iter()
                    .map(|vote_info| tendermint_v0o34::abci::types::VoteInfo {
                        validator: tendermint_v0o34::abci::types::Validator {
                            address: vote_info.validator.address,
                            power: vote_info.validator.power.value().try_into().expect(
                                "failed to convert validator power to tendermint v0_37 format",
                            ),
                        },
                        sig_info: match vote_info.sig_info {
                            tendermint_v0o40::abci::types::BlockSignatureInfo::Flag(
                                block_id_flag,
                            ) => match block_id_flag {
                                tendermint_v0o40::block::BlockIdFlag::Absent => {
                                    tendermint_v0o34::abci::types::BlockSignatureInfo::Flag(
                                        tendermint_v0o34::block::BlockIdFlag::Absent,
                                    )
                                }
                                tendermint_v0o40::block::BlockIdFlag::Commit => {
                                    tendermint_v0o34::abci::types::BlockSignatureInfo::Flag(
                                        tendermint_v0o34::block::BlockIdFlag::Commit,
                                    )
                                }
                                tendermint_v0o40::block::BlockIdFlag::Nil => {
                                    tendermint_v0o34::abci::types::BlockSignatureInfo::Flag(
                                        tendermint_v0o34::block::BlockIdFlag::Nil,
                                    )
                                }
                            },
                            tendermint_v0o40::abci::types::BlockSignatureInfo::LegacySigned => {
                                tendermint_v0o34::abci::types::BlockSignatureInfo::LegacySigned
                            }
                        },
                    })
                    .collect(),
            },
            byzantine_validators: self
                .0
                .byzantine_validators
                .iter()
                .map(|misbehavior| tendermint_v0o34::abci::types::Misbehavior {
                    kind: match misbehavior.kind {
                        tendermint_v0o40::abci::types::MisbehaviorKind::Unknown => {
                            tendermint_v0o34::abci::types::MisbehaviorKind::Unknown
                        }
                        tendermint_v0o40::abci::types::MisbehaviorKind::DuplicateVote => {
                            tendermint_v0o34::abci::types::MisbehaviorKind::DuplicateVote
                        }
                        tendermint_v0o40::abci::types::MisbehaviorKind::LightClientAttack => {
                            tendermint_v0o34::abci::types::MisbehaviorKind::LightClientAttack
                        }
                    },
                    validator: tendermint_v0o34::abci::types::Validator {
                        address: misbehavior.validator.address,
                        power: misbehavior
                            .validator
                            .power
                            .value()
                            .try_into()
                            .expect("failed to convert validator power to tendermint v0_37 format"),
                    },
                    // Height is a u64 inside, so easy enough
                    height: tendermint_v0o34::block::Height::try_from(misbehavior.height.value())
                        .expect("failed to convert height to tendermint 0_37 format"),
                    // TODO: should the nanos be 0? am i doubling the evaluted time by summing (time +
                    // time-in-nanos)?
                    time: tendermint_v0o34::time::Time::from_unix_timestamp(
                        misbehavior.time.unix_timestamp(),
                        (misbehavior.time.unix_timestamp_nanos() % 1_000_000_000)
                            .try_into()
                            .expect("failed to convert nanos to 0_37 format"),
                    )
                    .expect("failed to convert timestamp to 0_37 format"),
                    total_voting_power: tendermint_v0o34::vote::Power::try_from(
                        misbehavior.total_voting_power.value(),
                    )
                    .expect("failed to convert total voting power to tendermint 0_37 format"),
                })
                .collect(),
        };
        Ok(bb)
    }
}

/// Custom wrapper type for Tendermint's `EndBlock` type,
/// which simply stores an i64.
#[derive(Clone, Debug)]
pub struct EndBlock {
    pub height: i64,
}

/// Trivial conversion from compat type to v0.37 format.
impl From<EndBlock> for tendermint_v0o34::abci::request::EndBlock {
    fn from(val: EndBlock) -> Self {
        tendermint_v0o34::abci::request::EndBlock { height: val.height }
    }
}

/// Trivial conversion from compat type to v0.40 format.
impl From<EndBlock> for tendermint_v0o40::abci::request::EndBlock {
    fn from(val: EndBlock) -> Self {
        tendermint_v0o40::abci::request::EndBlock { height: val.height }
    }
}

/// Custom wrapper type for Tendermint's `DeliverTx` type.
/// Specifically, this is the *request* type of DeliverTx.
/// Stores raw bytes, suitable for conversion.
#[derive(Clone, Debug)]
pub struct DeliverTx {
    pub tx: Vec<u8>,
}

/// Trivial conversion from compat type to v0.34 format.
impl From<DeliverTx> for tendermint_v0o34::abci::request::DeliverTx {
    fn from(val: DeliverTx) -> Self {
        tendermint_v0o34::abci::request::DeliverTx { tx: val.tx.into() }
    }
}

/// Trivial conversion from compat type to v0.40 format.
impl From<DeliverTx> for tendermint_v0o40::abci::request::DeliverTx {
    fn from(val: DeliverTx) -> Self {
        tendermint_v0o40::abci::request::DeliverTx { tx: val.tx.into() }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ResponseDeliverTx {
    pub code: u32,
    pub data: Vec<u8>,
    pub log: String,
    pub info: String,
    pub gas_wanted: i64,
    pub gas_used: i64,
    pub events: Vec<Event>,
    pub codespace: String,
}

impl ResponseDeliverTx {
    pub fn with_defaults(events: anyhow::Result<Vec<Event>>) -> ResponseDeliverTx {
        // TODO: avoid copying this code from penumbra_app
        match events {
            Ok(events) => Self {
                events,
                ..Default::default()
            },
            Err(e) => {
                Self {
                    code: 1u32,
                    // Use the alternate format specifier to include the chain of error causes.
                    log: format!("{e:#}"),
                    ..Default::default()
                }
            }
        }
    }
}

impl ResponseDeliverTx {
    pub fn encode_to_latest_tx_result(self, height: i64, index: u32, tx: &[u8]) -> Vec<u8> {
        use prost::Message;
        use tendermint_proto::abci::{ExecTxResult, TxResult};

        let exec_result = ExecTxResult {
            code: self.code,
            data: self.data.into(),
            log: self.log,
            info: self.info,
            gas_wanted: self.gas_wanted,
            gas_used: self.gas_used,
            events: self.events.into_iter().map(|x| x.into()).collect(),
            codespace: self.codespace,
        };
        let tx_result = TxResult {
            height,
            index,
            tx: tx.to_vec().into(),
            result: Some(exec_result),
        };

        tx_result.encode_to_vec()
    }
}
