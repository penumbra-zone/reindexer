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

// TODO: Rename the `tendermint` import in Cargo.toml to `tendermint_v0o34`, so it's
// always explicit which tendermint dep is being used.
/// Dependencies for `tendermint` crates at `0.34.x` versions.
pub mod v0o34 {
    pub use tendermint;
    pub use tendermint::{
        abci::Event,
        v0_37::abci::request::{BeginBlock, DeliverTx, EndBlock},
    };
}

/// Dependencies for `tendermint` crates at `0.40.x` versions.
pub mod v0o40 {
    pub use tendermint_v0o40 as tendermint;
    pub use tendermint_v0o40::{
        abci::Event,
        v0_37::abci::request::{BeginBlock, DeliverTx, EndBlock},
    };
}

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
impl TryFrom<v0o40::tendermint::abci::Event> for Event {
    type Error = anyhow::Error;
    fn try_from(event: v0o40::tendermint::abci::Event) -> anyhow::Result<Event> {
        Ok(
            Event {
                kind: event.kind,
                attributes: event.attributes.into_iter().map(|attribute| {
                    // Newer versions of the Tendermint crate wrap the EventAttribute in an Enum,
                    // for backwards-compat. In the context of Penumbra chain data, we only expect
                    // the newer of the two formats.
                    match attribute {
                        tendermint_v0o40::abci::EventAttribute::V037(x) => {
                            let a = x.key.as_bytes().to_vec();
                            let b = x.value.as_bytes().to_vec();
                            let c = x.index;
                            (a, b, c)
                        },
                        tendermint_v0o40::abci::EventAttribute::V034(_x) => {
                            let msg = "unexpectedly encountered an ABCI Event formatted for Tendermint 0.34.x";
                            tracing::error!(msg);
                            // TODO: saner error handling
                            panic!("{}", msg);
                            // anyhow::bail!(msg);
                        }
                    }
                }
                ).collect(),
            }
        )
    }
}

impl TryFrom<tendermint::abci::Event> for Event {
    type Error = anyhow::Error;
    fn try_from(event: v0o34::tendermint::abci::Event) -> anyhow::Result<Event> {
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
pub struct Block(v0o40::tendermint::Block);

/// Provide for conversions from 0.40.x tendermint block types.
impl From<v0o40::tendermint::Block> for Block {
    fn from(block: v0o40::tendermint::Block) -> Block {
        Block(block)
    }
}

/// Provide for conversions from 0.40.x tendermint block types.
impl Into<v0o40::tendermint::Block> for Block {
    fn into(self: Block) -> v0o40::tendermint::Block {
        self.0
    }
}

impl TryFrom<v0o34::tendermint::Block> for Block {
    type Error = anyhow::Error;
    fn try_from(block: v0o34::tendermint::Block) -> anyhow::Result<Block> {
        let block = Block(v0o40::tendermint::Block::new(
            v0o40::tendermint::block::Header {
                version: v0o40::tendermint::block::header::Version {
                    // Version is a tuple of u64s, so it's easy to unpack.
                    block: block.header.version.block,
                    app: block.header.version.app,
                },
                // chain_id is just a string
                chain_id: v0o40::tendermint::chain::id::Id::try_from(
                    block.header.chain_id.as_str(),
                )?,
                // Height is a u64 inside, so easy enough
                height: v0o40::tendermint::block::Height::try_from(block.header.height.value())?,
                // TODO: should the nanos be 0? am i doubling the evaluted time by summing (time +
                // time-in-nanos)?
                time: v0o40::tendermint::time::Time::from_unix_timestamp(
                    block.header.time.unix_timestamp(),
                    block.header.time.unix_timestamp_nanos().try_into()?,
                )?,
                last_block_id: match block.header.last_block_id {
                    Some(last_block_id) => Some(v0o40::tendermint::block::Id {
                        hash: v0o40::tendermint::hash::Hash::Sha256(
                            last_block_id.hash.as_bytes().try_into()?,
                        ),
                        part_set_header: v0o40::tendermint::block::parts::Header::new(
                            last_block_id.part_set_header.total,
                            v0o40::tendermint::hash::Hash::Sha256(
                                last_block_id.hash.as_bytes().try_into()?,
                            ),
                        )?,
                    }),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                last_commit_hash: match block.header.last_commit_hash {
                    Some(last_commit_hash) => Some(v0o40::tendermint::hash::Hash::Sha256(
                        last_commit_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                data_hash: match block.header.data_hash {
                    Some(data_hash) => Some(v0o40::tendermint::hash::Hash::Sha256(
                        data_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Round-trip as bytes
                validators_hash: v0o40::tendermint::hash::Hash::Sha256(
                    block.header.validators_hash.as_bytes().try_into()?,
                ),
                // Round-trip as bytes
                next_validators_hash: v0o40::tendermint::hash::Hash::Sha256(
                    block.header.next_validators_hash.as_bytes().try_into()?,
                ),
                // Round-trip as bytes
                consensus_hash: v0o40::tendermint::hash::Hash::Sha256(
                    block.header.consensus_hash.as_bytes().try_into()?,
                ),
                // Round-trip as bytes
                app_hash: v0o40::tendermint::hash::AppHash::try_from(
                    block.header.app_hash.as_bytes().to_vec(),
                )?,
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                last_results_hash: match block.header.last_results_hash {
                    Some(last_results_hash) => Some(v0o40::tendermint::hash::Hash::Sha256(
                        last_results_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                evidence_hash: match block.header.evidence_hash {
                    Some(evidence_hash) => Some(v0o40::tendermint::hash::Hash::Sha256(
                        evidence_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Round-trip as bytes.
                proposer_address: v0o40::tendermint::account::Id::new(
                    block.header.proposer_address.as_bytes().try_into()?,
                ),
            },
            // data
            block.data.into_iter().collect(),
            // TODO: need to unpack a compcliated evidence List and match its enums
            v0o40::tendermint::evidence::List::new(block.evidence.iter().flat_map(|e| {
                match e {
                    v0o34::tendermint::evidence::Evidence::DuplicateVote(bad) => vec![v0o40::tendermint::abci::types::Misbehavior {
                        kind: v0o40::tendermint::abci::types::MisbehaviorKind::DuplicateVote,
                        validator: Validator { address: bad.vote_a.validator_address.as_bytes().to_vec().try_into().unwrap(), power: bad.validator_power.into() }.try_into().unwrap(),
                        height: bad.vote_a.height.value().try_into().unwrap(),
                        time: v0o40::tendermint::time::Time::from_unix_timestamp(
                            bad.timestamp.unix_timestamp(),
                            bad.timestamp.unix_timestamp_nanos().try_into().unwrap(),
                        ).unwrap(),
                        total_voting_power: bad.total_voting_power.value().try_into().unwrap(),
                    }],

                    v0o34::tendermint::evidence::Evidence::LightClientAttack(bad) => bad
                        .byzantine_validators
                        .iter()
                        .map(|v| v0o40::tendermint::abci::types::Misbehavior {
                            kind: v0o40::tendermint::abci::types::MisbehaviorKind::LightClientAttack,
                            validator: Validator { address: v.address.as_bytes().to_vec().try_into().unwrap(), power: v.power.into() }.try_into().unwrap(),
                            height: bad.common_height.value().try_into().unwrap(),
                            time: v0o40::tendermint::time::Time::from_unix_timestamp(
                                bad.timestamp.unix_timestamp(),
                                bad.timestamp.unix_timestamp_nanos().try_into().unwrap(),
                            ).unwrap(),
                            total_voting_power: bad.total_voting_power.value().try_into().unwrap(),
                        })
                        .collect(),
                }
            }
            ).collect()),

            match block.last_commit {
                None => None,
                Some(last_commit) => Some(v0o40::tendermint::block::Commit {
                    height: v0o40::tendermint::block::Height::try_from(last_commit.height.value())?,
                    round: v0o40::tendermint::block::Round::try_from(last_commit.round.value())?,
                    block_id: v0o40::tendermint::block::Id {
                        hash: match last_commit.block_id.hash {
                            v0o34::tendermint::Hash::Sha256(h) => {
                                v0o40::tendermint::Hash::Sha256(h)
                            }
                            v0o34::tendermint::Hash::None => v0o40::tendermint::Hash::None,
                        },

                        part_set_header: v0o40::tendermint::block::parts::Header::new(
                            last_commit.block_id.part_set_header.total,
                            //                            c.block_id.part_set_header.hash.into(),
                            match last_commit.block_id.part_set_header.hash {
                                v0o34::tendermint::Hash::Sha256(h) => {
                                    v0o40::tendermint::Hash::Sha256(h)
                                }
                                v0o34::tendermint::Hash::None => v0o40::tendermint::Hash::None,
                            },
                        )?,
                    },
                    signatures: last_commit
                        .signatures
                        .iter()
                        .map(|s| match s {
                            v0o34::tendermint::block::commit_sig::CommitSig::BlockIdFlagAbsent => {
                                v0o40::tendermint::block::commit_sig::CommitSig::BlockIdFlagAbsent
                            }
                            v0o34::tendermint::block::commit_sig::CommitSig::BlockIdFlagCommit { ref validator_address, ref timestamp, ref signature }  => 
                                v0o40::tendermint::block::commit_sig::CommitSig::BlockIdFlagCommit {
                                    validator_address: v0o40::tendermint::account::Id::new(
                                        validator_address.as_bytes().try_into().unwrap(),
                                    ),
                                    timestamp: v0o40::tendermint::time::Time::from_unix_timestamp(
                                        timestamp.unix_timestamp(),
                                        timestamp
                                            .unix_timestamp_nanos()
                                            .try_into()
                                            .expect("failed to convert timestamp"),
                                    )
                                    .unwrap(),
                                    signature: match signature {
                                        Some(s2) => 
                                            v0o40::tendermint::signature::Signature::new(
                                                s2.as_bytes())
                                            .unwrap(),
                                        None => None,
                                    },
                                },
                            v0o34::tendermint::block::commit_sig::CommitSig::BlockIdFlagNil { ref validator_address, ref timestamp, ref signature } =>
                                v0o40::tendermint::block::commit_sig::CommitSig::BlockIdFlagNil {
                                    validator_address: v0o40::tendermint::account::Id::new(
                                        validator_address.as_bytes().try_into().unwrap(),
                                    ),
                                    timestamp: v0o40::tendermint::time::Time::from_unix_timestamp(
                                        timestamp.unix_timestamp(),
                                        timestamp
                                            .unix_timestamp_nanos()
                                            .try_into()
                                            .expect("failed to convert timestamp"),
                                    )
                                    .unwrap(),
                                    signature: match signature {
                                        Some(s2) => 
                                            v0o40::tendermint::signature::Signature::new(
                                                s2.as_bytes())
                                            .unwrap(),
                                        None => None,
                                    },
                                },
                            }
                        )
                        .collect(),
                }),
            },
        ));
        Ok(block)
    }
}
/// Wrapper type for handling conversions between incompatible versions of Tendermint `BeginBlock`
/// types. Stores the most recent Tendermint version as a singleton, and defers conversions to
/// TryInto impls.
#[derive(Clone, Debug)]
pub struct BeginBlock(v0o40::BeginBlock);

/// Convenience conversion from `Block` to `BeginBlock`
impl Into<BeginBlock> for Block {
    fn into(self: Block) -> BeginBlock {
        BeginBlock(v0o40::BeginBlock {
            hash: self.0.header.hash(),
            header: self.0.header.clone(),
            // last_commit_info: commit_to_info(self.0.last_commit.as_ref()),
            last_commit_info: match self.0.last_commit {
                None => v0o40::tendermint::abci::types::CommitInfo {
                    round: Default::default(),
                    votes: Default::default(),
                },
                Some(commit) =>
                    v0o40::tendermint::abci::types::CommitInfo {
                        round: commit.round,
                        votes: commit.signatures.iter().filter_map(|s|  match s {
                            v0o40::tendermint::block::commit_sig::CommitSig::BlockIdFlagAbsent => None,
                            v0o40::tendermint::block::commit_sig::CommitSig::BlockIdFlagCommit {
                                validator_address, ..
                            } => Some(v0o40::tendermint::abci::types::VoteInfo {
                                // DRAGON: we assume that the penumbra logic will not care about the power
                                // we declare here.
                                // validator: make_validator(*validator_address, Default::default()),
                                validator: Validator { address: validator_address.as_bytes().to_vec().try_into().ok()?, power: 1 }.try_into().ok()?,
                                sig_info: v0o40::tendermint::abci::types::BlockSignatureInfo::Flag(v0o40::tendermint::block::BlockIdFlag::Commit),
                            }),
                            v0o40::tendermint::block::commit_sig::CommitSig::BlockIdFlagNil {
                                validator_address, ..
                            } => Some(v0o40::tendermint::abci::types::VoteInfo {
                                // DRAGON: we assume that the penumbra logic will not care about the power
                                // we declare here.
                                // validator: make_validator(*validator_address, Default::default()),
                                validator: Validator { address: validator_address.as_bytes().to_vec().try_into().ok()?, power: 1 }.try_into().ok()?,
                                sig_info: v0o40::tendermint::abci::types::BlockSignatureInfo::Flag(v0o40::tendermint::block::BlockIdFlag::Nil),
                            }),
                        }).collect(),
                    },
                },
            byzantine_validators: self
                .0
                .evidence
                .iter()
                .flat_map(evidence_to_misbehavior)
                .collect()
        })
    }
}

/// Convenience conversion for extracting the inner value.
impl Into<v0o40::BeginBlock> for BeginBlock {
    fn into(self) -> v0o40::BeginBlock {
        self.0
    }
}

/// Fallible conversion from the current BeginBlock spec to an older version.
/// Unsure if this is actually useful in reindexer: do we only need TryFrom older blocks?
impl TryInto<v0o34::BeginBlock> for BeginBlock {
    type Error = anyhow::Error;
    fn try_into(self) -> anyhow::Result<v0o34::BeginBlock> {
        let bb: v0o34::BeginBlock = v0o34::BeginBlock {
            hash: v0o34::tendermint::hash::Hash::Sha256(self.0.hash.as_bytes().try_into()?),
            header: v0o34::tendermint::block::Header {
                version: v0o34::tendermint::block::header::Version {
                    // Version is a tuple of u64s, so it's easy to unpack.
                    block: self.0.header.version.block,
                    app: self.0.header.version.app,
                },
                // chain_id is just a string
                chain_id: v0o34::tendermint::chain::id::Id::try_from(
                    self.0.header.chain_id.as_str(),
                )?,
                // Height is a u64 inside, so easy enough
                height: v0o34::tendermint::block::Height::try_from(self.0.header.height.value())?,
                // TODO: should the nanos be 0? am i doubling the evaluted time by summing (time +
                // time-in-nanos)?
                time: v0o34::tendermint::time::Time::from_unix_timestamp(
                    self.0.header.time.unix_timestamp(),
                    self.0.header.time.unix_timestamp_nanos().try_into()?,
                )?,
                last_block_id: match self.0.header.last_block_id {
                    Some(last_block_id) => Some(v0o34::tendermint::block::Id {
                        hash: v0o34::tendermint::hash::Hash::Sha256(
                            last_block_id.hash.as_bytes().try_into()?,
                        ),
                        part_set_header: v0o34::tendermint::block::parts::Header::new(
                            last_block_id.part_set_header.total,
                            v0o34::tendermint::hash::Hash::Sha256(
                                last_block_id.hash.as_bytes().try_into()?,
                            ),
                        )?,
                    }),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                last_commit_hash: match self.0.header.last_commit_hash {
                    Some(last_commit_hash) => Some(v0o34::tendermint::hash::Hash::Sha256(
                        last_commit_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                data_hash: match self.0.header.data_hash {
                    Some(data_hash) => Some(v0o34::tendermint::hash::Hash::Sha256(
                        data_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Round-trip as bytes
                validators_hash: v0o34::tendermint::hash::Hash::Sha256(
                    self.0.header.validators_hash.as_bytes().try_into()?,
                ),
                // Round-trip as bytes
                next_validators_hash: v0o34::tendermint::hash::Hash::Sha256(
                    self.0.header.next_validators_hash.as_bytes().try_into()?,
                ),
                // Round-trip as bytes
                consensus_hash: v0o34::tendermint::hash::Hash::Sha256(
                    self.0.header.consensus_hash.as_bytes().try_into()?,
                ),
                // Round-trip as bytes
                app_hash: v0o34::tendermint::hash::AppHash::try_from(
                    self.0.header.app_hash.as_bytes().to_vec(),
                )?,
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                last_results_hash: match self.0.header.last_results_hash {
                    Some(last_results_hash) => Some(v0o34::tendermint::hash::Hash::Sha256(
                        last_results_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Easy enough to round-trip the bytes representation, and retain the Option value.
                evidence_hash: match self.0.header.evidence_hash {
                    Some(evidence_hash) => Some(v0o34::tendermint::hash::Hash::Sha256(
                        evidence_hash.as_bytes().try_into()?,
                    )),
                    None => None,
                },
                // Round-trip as bytes.
                proposer_address: v0o34::tendermint::account::Id::new(
                    self.0.header.proposer_address.as_bytes().try_into()?,
                ),
            },
            last_commit_info: v0o34::tendermint::abci::types::CommitInfo {
                // Round is a u32, simple to convert.
                round: self.0.last_commit_info.round.value().try_into()?,
                votes: self
                    .0
                    .last_commit_info
                    .votes
                    .iter()
                    .map(|vote_info| v0o34::tendermint::abci::types::VoteInfo {
                        validator: v0o34::tendermint::abci::types::Validator {
                            address: vote_info.validator.address.try_into().expect(
                                "failed to convert validator address to tendermint v0_37 format",
                            ),
                            power: vote_info.validator.power.value().try_into().expect(
                                "failed to convert validator power to tendermint v0_37 format",
                            ),
                        },
                        sig_info: match vote_info.sig_info {
                            v0o40::tendermint::abci::types::BlockSignatureInfo::Flag(
                                block_id_flag,
                            ) => match block_id_flag {
                                v0o40::tendermint::block::BlockIdFlag::Absent => {
                                    v0o34::tendermint::abci::types::BlockSignatureInfo::Flag(
                                        v0o34::tendermint::block::BlockIdFlag::Absent,
                                    )
                                }
                                v0o40::tendermint::block::BlockIdFlag::Commit => {
                                    v0o34::tendermint::abci::types::BlockSignatureInfo::Flag(
                                        v0o34::tendermint::block::BlockIdFlag::Commit,
                                    )
                                }
                                v0o40::tendermint::block::BlockIdFlag::Nil => {
                                    v0o34::tendermint::abci::types::BlockSignatureInfo::Flag(
                                        v0o34::tendermint::block::BlockIdFlag::Nil,
                                    )
                                }
                            },
                            v0o40::tendermint::abci::types::BlockSignatureInfo::LegacySigned => {
                                v0o34::tendermint::abci::types::BlockSignatureInfo::LegacySigned
                            }
                        },
                    })
                    .collect(),
            },
            byzantine_validators: self
                .0
                .byzantine_validators
                .iter()
                .map(|misbehavior| v0o34::tendermint::abci::types::Misbehavior {
                    kind: match misbehavior.kind {
                        v0o40::tendermint::abci::types::MisbehaviorKind::Unknown => {
                            v0o34::tendermint::abci::types::MisbehaviorKind::Unknown
                        }
                        v0o40::tendermint::abci::types::MisbehaviorKind::DuplicateVote => {
                            v0o34::tendermint::abci::types::MisbehaviorKind::DuplicateVote
                        }
                        v0o40::tendermint::abci::types::MisbehaviorKind::LightClientAttack => {
                            v0o34::tendermint::abci::types::MisbehaviorKind::LightClientAttack
                        }
                    },
                    validator: v0o34::tendermint::abci::types::Validator {
                        address: misbehavior.validator.address.try_into().expect(
                            "failed to convert validator address to tendermint 0_37 format",
                        ),
                        power: misbehavior
                            .validator
                            .power
                            .value()
                            .try_into()
                            .expect("failed to convert validator power to tendermint v0_37 format"),
                    },
                    // Height is a u64 inside, so easy enough
                    height: v0o34::tendermint::block::Height::try_from(misbehavior.height.value())
                        .expect("failed to convert height to tendermint 0_37 format"),
                    // TODO: should the nanos be 0? am i doubling the evaluted time by summing (time +
                    // time-in-nanos)?
                    time: v0o34::tendermint::time::Time::from_unix_timestamp(
                        misbehavior.time.unix_timestamp(),
                        misbehavior
                            .time
                            .unix_timestamp_nanos()
                            .try_into()
                            .expect("failed to convert nanos to 0_37 format"),
                    )
                    .expect("failed to convert timestamp to 0_37 format"),
                    total_voting_power: v0o34::tendermint::vote::Power::try_from(
                        misbehavior.total_voting_power.value(),
                    )
                    .expect("failed to convert total voting power to tendermint 0_37 format"),
                })
                .collect(),
        };
        Ok(bb)
    }
}

/// Custom wrapper type for Tendermint concept of `Header`.
#[derive(Clone, Debug)]
pub struct Header {}

// Can't use this yet, should define custom `Block` first.
// impl From<v0o34::tendermint::Block> for BeginBlock {
//     fn from(block: v0o34::tendermint::Block) -> BeginBlock {
//         let bb = v0o40::tendermint::v0_37::abci::request::BeginBlock {
//             hash: block.header.hash(),
//             header: block.header.clone(),
//             last_commit_info: commit_to_info(block.last_commit.as_ref()),
//             byzantine_validators: block
//                 .evidence
//                 .iter()
//                 .flat_map(evidence_to_misbehavior)
//                 .collect(),
//         };
//         bb
//     }
// }

/// Custom wrapper type for Tendermint's `EndBlock` type,
/// which simply stores an i64.
#[derive(Clone, Debug)]
pub struct EndBlock {
    pub height: i64,
}

/// Trivial conversion from compat type to v0.37 format.
impl Into<v0o34::tendermint::abci::request::EndBlock> for EndBlock {
    fn into(self) -> v0o34::tendermint::abci::request::EndBlock {
        v0o34::tendermint::abci::request::EndBlock {
            height: self.height,
        }
    }
}

/// Trivial conversion from compat type to v0.40 format.
impl Into<v0o40::tendermint::abci::request::EndBlock> for EndBlock {
    fn into(self) -> v0o40::tendermint::abci::request::EndBlock {
        v0o40::tendermint::abci::request::EndBlock {
            height: self.height,
        }
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
impl Into<v0o34::tendermint::abci::request::DeliverTx> for DeliverTx {
    fn into(self) -> v0o34::tendermint::abci::request::DeliverTx {
        v0o34::tendermint::abci::request::DeliverTx { tx: self.tx.into() }
    }
}

/// Trivial conversion from compat type to v0.40 format.
impl Into<v0o40::tendermint::abci::request::DeliverTx> for DeliverTx {
    fn into(self) -> v0o40::tendermint::abci::request::DeliverTx {
        v0o40::tendermint::abci::request::DeliverTx { tx: self.tx.into() }
    }
}

/// Trivial conversion from v0.34 format to compat type.
impl From<v0o34::tendermint::abci::request::DeliverTx> for DeliverTx {
    fn from(tx: v0o34::tendermint::abci::request::DeliverTx) -> DeliverTx {
        DeliverTx { tx: tx.tx.into() }
    }
}

/// Trivial conversion from v0.40 format to compat type.
impl From<v0o40::tendermint::abci::request::DeliverTx> for DeliverTx {
    fn from(tx: v0o40::tendermint::abci::request::DeliverTx) -> DeliverTx {
        DeliverTx { tx: tx.tx.into() }
    }
}

/// Custom wrapper type for Tendermint's notion of a Validator.
pub struct Validator {
    /// Address is stored as raw bytes; should be 20 long.
    pub address: [u8; 20],
    /// Power is internally represented as a u64, so we'll just store that.
    pub power: u64,
}

impl TryFrom<v0o34::tendermint::abci::types::Validator> for Validator {
    type Error = anyhow::Error;
    fn try_from(validator: v0o34::tendermint::abci::types::Validator) -> anyhow::Result<Validator> {
        Ok(Validator {
            address: validator.address,
            power: validator.power.try_into()?,
        })
    }
}

impl TryFrom<v0o40::tendermint::abci::types::Validator> for Validator {
    type Error = anyhow::Error;
    fn try_from(validator: v0o40::tendermint::abci::types::Validator) -> anyhow::Result<Validator> {
        Ok(Validator {
            address: validator.address,
            power: validator.power.try_into()?,
        })
    }
}

impl TryInto<v0o40::tendermint::abci::types::Validator> for Validator {
    type Error = anyhow::Error;
    fn try_into(self) -> anyhow::Result<v0o40::tendermint::abci::types::Validator> {
        Ok(v0o40::tendermint::abci::types::Validator {
            address: self.address,
            power: self.power.try_into()?,
        })
    }
}

impl TryInto<v0o34::tendermint::abci::types::Validator> for Validator {
    type Error = anyhow::Error;
    fn try_into(self) -> anyhow::Result<v0o34::tendermint::abci::types::Validator> {
        Ok(v0o34::tendermint::abci::types::Validator {
            address: self.address,
            power: self.power.try_into()?,
        })
    }
}
