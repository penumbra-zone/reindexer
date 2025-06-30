#![cfg(feature = "network-integration")]
//! Integration tests the `penumbra-reindexer`, wrangling archives for the public Penumbra testnet.
//! These tests are off by default, given that they contact remote services,
//! and require a *significant* amount of disk space and bandwidth.
//!
//! These tests are intended to constitute a soup-to-nuts verification that given node snapshots,
//! fetched from remote URLs and stored locally, the entirety of a chain's events
//! can be reconstructed, across any and all upgrade boundaries.
//!
//! Right now, however, only the `penumbra-reindexer archive` step is exercised.
//! Further work should confirm that `penumbra-reindexer regen-step` is exercised,
//! and assertions made on the database contents.

mod common;
