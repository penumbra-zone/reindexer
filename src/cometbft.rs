//! This module contains utilities for reading cometbft data.
//!
//! This contains the actual FFI shim and what not.
use anyhow::{anyhow, Context};
use penumbra_proto::{tendermint::types as pb, Message};
use std::{
    os::raw::c_void,
    path::{Path, PathBuf},
};

#[link(name = "cometbft", kind = "static")]
extern "C" {
    fn c_store_new(
        dir_ptr: *const u8,
        dir_len: i32,
        backend_ptr: *const u8,
        backend_len: i32,
    ) -> *const c_void;
    fn c_store_first_height(ptr: *const c_void) -> i64;
    fn c_store_last_height(ptr: *const c_void) -> i64;
    fn c_store_block_by_height(
        ptr: *const c_void,
        height: i64,
        out_ptr: *mut u8,
        out_cap: i32,
    ) -> i32;
    fn c_store_delete(ptr: *const c_void);
}

/// How many bytes we expect an encoded block to be.
///
/// About 1 MiB seems fine, maybe a bit small in extreme cases.
const EXPECTED_BLOCK_PROTO_SIZE: usize = 1 << 20;

/// A wrapper around the FFI for the cometbft store.
///
/// This uses unsafe internally, but presents a safe interface.
struct RawStore {
    handle: *const c_void,
    buf: Vec<u8>,
}

impl RawStore {
    pub fn new(backend: &str, dir: &Path) -> anyhow::Result<Self> {
        let dir_bytes = dir.as_os_str().as_encoded_bytes();
        let handle = unsafe {
            // Safety: the Go side of things will immediately copy the data, and not write into it,
            // or read past the provided bounds.
            c_store_new(
                dir_bytes.as_ptr(),
                i32::try_from(dir_bytes.len())
                    .context("directory length should fit into an i32")?,
                backend.as_ptr(),
                i32::try_from(backend.len()).context("backend type should fit into an i32")?,
            )
        };
        Ok(Self {
            handle,
            buf: Vec::with_capacity(EXPECTED_BLOCK_PROTO_SIZE),
        })
    }

    pub fn first_height(&mut self) -> i64 {
        unsafe {
            // Safety: because we take mutable ownership, we avoid any shenanigans on the Go side.
            c_store_first_height(self.handle)
        }
    }

    pub fn last_height(&mut self) -> i64 {
        unsafe {
            // Safety: because we take mutable ownership, we avoid any shenanigans on the Go side.
            c_store_last_height(self.handle)
        }
    }

    pub fn block_by_height(&mut self, height: i64) -> Option<&[u8]> {
        // Try reading the block, growing our buffer as necessary
        let mut res;
        while {
            res = unsafe {
                // Safety: the Go side will not write past the capacity we give it here,
                // and we've allocated the appropriate amount of capacity on the rust side.
                let out_ptr = self.buf.as_mut_ptr();
                let out_cap = i32::try_from(self.buf.capacity())
                    .expect("capacity should not have exceeded i32");
                c_store_block_by_height(self.handle, height, out_ptr, out_cap)
            };
            if res == -1 {
                return None;
            }
            res < 0
        } {
            // Increase the buffer by another block size's worth.
            self.buf.reserve(EXPECTED_BLOCK_PROTO_SIZE);
        }
        unsafe {
            // Safety: res will be positive here, and be the length that Go
            // actually wrote bytes into on the other side.
            self.buf.set_len(res as usize);
        }
        Some(self.buf.as_slice())
    }
}

impl Drop for RawStore {
    fn drop(&mut self) {
        unsafe {
            // Safety: the existence of this method ensures we don't leak memory,
            // and the &mut avoids other shenanigans.
            c_store_delete(self.handle);
        }
    }
}

// Safety: a [RawStore] will always contain a unique handle to the Go object.
unsafe impl Send for RawStore {}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    inner: pb::Block,
}

impl Block {
    /// Encode Self into a vector of bytes.
    #[allow(dead_code)]
    pub fn encode(self) -> Vec<u8> {
        self.inner.encode_to_vec()
    }

    /// Attempt to decode data producing Self.
    pub fn decode(data: &[u8]) -> anyhow::Result<Self> {
        Ok(Self {
            inner: Message::decode(data)?,
        })
    }
}

/// The parts of the cometbft config that we care about.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    db_backend: String,
    db_dir: PathBuf,
}

impl Config {
    /// Read this from a cometbft directory.
    ///
    /// This assumes that the config file is in the usual ./config/config.toml location.
    ///
    /// Use [Self::read_file] if you want to use a different file.
    pub fn read_dir(cometbft_dir: &Path) -> anyhow::Result<Self> {
        Self::read_file(&cometbft_dir.join("config/config.toml"))
    }

    /// Read this from a specific file.
    ///
    /// Use [Self::from_toml] if you want to read from the contents directly.
    pub fn read_file(file: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(file)?;
        let string = String::from_utf8(bytes)?;
        Self::from_toml(&string)
    }

    /// Attempt to read this config from a TOML string.
    pub fn from_toml(data: &str) -> anyhow::Result<Self> {
        let value: toml::Value = toml::from_str(data)?;
        let db_backend = value
            .get("db_backend")
            .and_then(|x| Some(x.as_str()?.to_owned()))
            .ok_or(anyhow!("no `db_backend` field"))?;
        let db_dir: PathBuf = value
            .get("db_dir")
            .and_then(|x| x.as_str())
            .ok_or(anyhow!("no `db_dir` field"))?
            .try_into()?;
        Ok(Self { db_backend, db_dir })
    }
}

/// A store over cometbft data.
///
/// This can be used to retrieve blocks, among other things.
pub struct Store {
    raw: RawStore,
}

impl Store {
    /// Create a new store given the location of cometbft data.
    ///
    /// `backend` should be the type of the cometbft database.
    /// `dir` should be the path of the cometbft data store.
    pub fn new(cometbft_dir: &Path) -> anyhow::Result<Self> {
        let config = Config::read_dir(cometbft_dir)?;
        Ok(Self {
            raw: RawStore::new(&config.db_backend, &cometbft_dir.join(&config.db_dir))?,
        })
    }

    /// Retrieve the height of the last block in the store.
    pub fn first_height(&mut self) -> Option<i64> {
        // Heights of 0 are indicative of an empty block store, so we can wrap this nicely.
        match self.raw.first_height() {
            x if x <= 0 => None,
            x => Some(x),
        }
    }

    /// Retrieve the height of the last block in the store.
    pub fn last_height(&mut self) -> i64 {
        self.raw.last_height()
    }

    /// Attempt to retrieve a block at a given height.
    ///
    /// This will return `None` if there's no such block.
    pub fn block_by_height(&mut self, height: i64) -> anyhow::Result<Option<Block>> {
        self.raw
            .block_by_height(height)
            .map(Block::decode)
            .transpose()
    }
}

#[cfg(test)]
mod test {
    use super::Config;

    #[test]
    fn test_config_parsing() -> anyhow::Result<()> {
        let toml = r#"
db_backend = "goleveldb"
db_dir = "data"
        "#;
        let config = Config::from_toml(toml)?;
        assert_eq!(
            config,
            Config {
                db_dir: "data".into(),
                db_backend: "goleveldb".into()
            }
        );
        Ok(())
    }
}
