//! This module contains utilities for reading cometbft data.
//!
//! This contains the actual FFI shim and what not.
use anyhow::{anyhow, Context};
use async_stream::try_stream;
use async_trait::async_trait;
use futures_core::Stream;
use penumbra_proto::{
    tendermint::types::{self as pb},
    Message,
};
use std::{
    os::raw::c_void,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
};
use tendermint_proto::v0_37::types::{Block as ProtoBlock, BlockId as ProtoBlockId};
use tendermint_v0o40::{
    block::Id as TendermintBlockId, Block as TendermintBlock, Genesis as TendermintGenesis,
};
use tokio::sync::Mutex;

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
    /// Cached fields
    height: u64,
}

impl Block {
    /// Encode Self into a vector of bytes.
    pub fn encode(&self) -> Vec<u8> {
        self.inner.encode_to_vec()
    }

    /// Get the height of this block.
    pub fn height(&self) -> u64 {
        self.height
    }

    /// Attempt to decode data producing Self.
    pub fn decode(data: &[u8]) -> anyhow::Result<Self> {
        let inner = pb::Block::decode(data)?;
        let height = inner
            .header
            .as_ref()
            .ok_or(anyhow!("block should have header"))?
            .height
            .try_into()?;
        Ok(Self { inner, height })
    }

    /// Calculate tendermint's view of this block
    pub fn tendermint(&self) -> anyhow::Result<TendermintBlock> {
        // We skip validation logic by temporarily setting the height to 1
        let height = self.height();
        let mut out = self.inner.clone();
        let last_block_id = out.header.as_ref().and_then(|x| x.last_block_id.clone());
        out.header = out.header.map(|x| {
            let mut out = x.clone();
            out.height = 1;
            out.last_block_id = None;
            out
        });
        let data = out.encode_to_vec();
        let mut block =
            <TendermintBlock as tendermint_proto::Protobuf<ProtoBlock>>::decode_vec(&data)?;
        block.header.height = height.try_into()?;
        block.header.last_block_id = last_block_id
            .map(|x| -> anyhow::Result<_> {
                let data = x.encode_to_vec();
                Ok(<TendermintBlockId as tendermint_proto::Protobuf<
                    ProtoBlockId,
                >>::decode_vec(&data)?)
            })
            .transpose()?;
        Ok(block)
    }

    #[cfg(test)]
    pub fn test_value() -> Self {
        Self::decode(include_bytes!("../test_data/block.bin"))
            .expect("test data should be a valid block")
    }
}

/// The parts of the cometbft config that we care about.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    db_backend: String,
    db_dir: PathBuf,
    genesis_file: PathBuf,
}

impl Config {
    /// Read this from a cometbft directory.
    ///
    /// This assumes that the config file is in the usual ./config/config.toml location.
    ///
    /// Use [Self::read_file] if you want to use a different file.
    pub fn read_dir(cometbft_dir: &Path) -> anyhow::Result<Self> {
        let f = cometbft_dir.join("config/config.toml");
        Self::read_file(&f).context(format!(
            "failed to read cometbft config file at '{}'",
            f.display()
        ))
    }

    /// Read this from a specific file.
    ///
    /// Use [Self::from_toml] if you want to read from the contents directly.
    fn read_file(file: &Path) -> anyhow::Result<Self> {
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
            .into();
        let genesis_file: PathBuf = value
            .get("genesis_file")
            .and_then(|x| x.as_str())
            .ok_or(anyhow!("no `genesis_file` field"))?
            .into();
        Ok(Self {
            db_backend,
            db_dir,
            genesis_file,
        })
    }
}

/// Represent cometbft's view of genesis data.
///
/// This is generic, and doesn't know anything about what Penumbra needs.
#[derive(Debug, Clone)]
pub struct Genesis {
    inner: TendermintGenesis,
}

impl Genesis {
    /// Read a genesis file based on a cometbft directory, and a parsed cometbft [Config].
    ///
    /// We need a directory because the config file will contain the location of the
    /// genesis file relative to this directory.
    pub fn read_cometbft_dir(cometbft_dir: &Path, config: &Config) -> anyhow::Result<Self> {
        let file = cometbft_dir.join(&config.genesis_file);
        tracing::debug!("reading genesis file: {}", file.display());
        Self::read_file(&file)
    }

    /// Read genesis data from a file.
    pub fn read_file(path: &Path) -> anyhow::Result<Self> {
        let inner = serde_json::from_slice(&std::fs::read(path)?)?;
        Ok(Self { inner })
    }

    /// The initial height of the chain.
    pub fn initial_height(&self) -> u64 {
        self.inner
            .initial_height
            .try_into()
            .expect("initial height should fit into u64")
    }

    /// The identifier of the chain this genesis is for.
    pub fn chain_id(&self) -> String {
        self.inner.chain_id.to_string()
    }

    /// The app state embedded in this genesis file.
    ///
    /// This will be an opaque value we need to then parse.
    pub fn app_state(&self) -> &serde_json::Value {
        &self.inner.app_state
    }

    pub fn encode(&self) -> anyhow::Result<Vec<u8>> {
        serde_json::to_vec(&self.inner).map_err(Into::into)
    }

    pub fn decode(data: &[u8]) -> anyhow::Result<Self> {
        let inner = serde_json::from_slice(data)?;
        Ok(Self { inner })
    }

    #[cfg(test)]
    pub fn test_value() -> Self {
        Self::decode(include_bytes!("../test_data/genesis.json"))
            .expect("test genesis should parse")
    }
}

/// A stream of blocks, with their heights, allowing for errors.
pub type BlockStream<'a> = Pin<Box<dyn Stream<Item = anyhow::Result<(u64, Block)>> + 'a>>;

/// A trait abstracting over a store of cometbft data.
///
/// This unifies stores that read local data, and stores that read remote data.
#[async_trait]
pub trait Store: Send + 'static {
    /// Get the genesis file for this Generation.
    async fn get_genesis(&self) -> anyhow::Result<Genesis>;
    /// Get the height bounds for this Generation.
    ///
    /// If present, this is expected to be the first block, and last block present in the store.
    async fn get_height_bounds(&self) -> anyhow::Result<Option<(u64, u64)>>;
    /// Get a specific block.
    async fn get_block(&self, height: u64) -> anyhow::Result<Option<Block>>;
    /// Stream blocks between optional bounds.
    ///
    /// This has a default implementation which will:
    /// - truncate the bounds to be inside the result of [`Self::get_height_bounds`],
    /// - get each block in sequence.
    ///
    /// This can be inefficient in general, so this method can be overriden.
    /// (The use-case in mind here is a remote store, where we want to fetch several blocks at once).
    fn stream_blocks(&self, start: Option<u64>, end: Option<u64>) -> BlockStream<'_> {
        Box::pin(try_stream! {
            let bounds = {
                let mut internal = self.get_height_bounds().await?.ok_or(anyhow!("stream_blocks expects height bounds to exist"))?;
                if let Some(x) = start {
                    internal.0 = internal.0.max(x);
                }
                if let Some(x) = end {
                    internal.1 = internal.1.min(x);
                }
                internal
            };
            for height in bounds.0..=bounds.1 {
                let block = self.get_block(height).await?.ok_or(anyhow!("expected block at height {}", height))?;
                yield (height, block);
            }
        })
    }
}

/// A store over cometbft data, using the filesystem.
///
/// This can be used to retrieve blocks, among other things.
struct FileStore {
    raw: RawStore,
}

impl FileStore {
    /// Create a new store given the location of cometbft data.
    ///
    /// `backend` should be the type of the cometbft database.
    /// `dir` should be the path of the cometbft data store.
    fn new(cometbft_dir: &Path, config: &Config) -> anyhow::Result<Self> {
        Ok(Self {
            raw: RawStore::new(&config.db_backend, &cometbft_dir.join(&config.db_dir))?,
        })
    }

    /// Retrieve the height of the last block in the store.
    fn first_height(&mut self) -> Option<u64> {
        // Heights of 0 are indicative of an empty block store, so we can wrap this nicely.
        match self.raw.first_height() {
            x if x <= 0 => None,
            x => Some(x.try_into().expect("height should fit into u64")),
        }
    }

    /// Retrieve the height of the last block in the store.
    fn last_height(&mut self) -> Option<u64> {
        // Heights of 0 are indicative of an empty block store, so we can wrap this nicely.
        match self.raw.last_height() {
            x if x <= 0 => None,
            x => Some(x.try_into().expect("height should fit into u64")),
        }
    }

    /// Attempt to retrieve a block at a given height.
    ///
    /// This will return `None` if there's no such block.
    fn block_by_height(&mut self, height: u64) -> anyhow::Result<Option<Block>> {
        self.raw
            .block_by_height(height.try_into()?)
            .map(Block::decode)
            .transpose()
    }
}

pub enum LocalStoreGenesisLocation<'p> {
    #[allow(dead_code)]
    DirectFile(&'p Path),
    FromConfig,
}

/// A store which accesses data locally.
pub struct LocalStore {
    file_store: Arc<Mutex<FileStore>>,
    genesis: Genesis,
}

impl LocalStore {
    /// Initialize a new store from a cometbft home directory, and a way to locate the genesis.
    ///
    /// This genesis location indicates a direct path to read from, or that the config file
    /// assumed to be in this directory should be used to locate it instead.
    pub fn init(
        cometbft_dir: &Path,
        genesis: LocalStoreGenesisLocation<'_>,
    ) -> anyhow::Result<Self> {
        let config = Config::read_dir(cometbft_dir)?;
        let file_store = FileStore::new(cometbft_dir, &config)?;
        let genesis = match genesis {
            LocalStoreGenesisLocation::FromConfig => {
                Genesis::read_cometbft_dir(cometbft_dir, &config)?
            }
            LocalStoreGenesisLocation::DirectFile(path) => Genesis::read_file(path)?,
        };
        Ok(Self {
            genesis,
            file_store: Arc::new(Mutex::new(file_store)),
        })
    }
}

#[async_trait]
impl Store for LocalStore {
    async fn get_genesis(&self) -> anyhow::Result<Genesis> {
        Ok(self.genesis.clone())
    }

    async fn get_height_bounds(&self) -> anyhow::Result<Option<(u64, u64)>> {
        let mut file_store = self.file_store.lock().await;
        let start = file_store.first_height();
        let end = file_store.last_height();
        let out = match (start, end) {
            (None, _) => None,
            (_, None) => None,
            (Some(x), Some(y)) => Some((x, y)),
        };
        Ok(out)
    }

    async fn get_block(&self, height: u64) -> anyhow::Result<Option<Block>> {
        self.file_store.lock().await.block_by_height(height)
    }
}

mod remote;
pub use remote::RemoteStore;

#[cfg(test)]
mod test {
    use super::Config;

    #[test]
    fn test_config_parsing() -> anyhow::Result<()> {
        let toml = r#"
db_backend = "goleveldb"
db_dir = "data"
genesis_file = "config/genesis.json"
        "#;
        let config = Config::from_toml(toml)?;
        assert_eq!(
            config,
            Config {
                db_dir: "data".into(),
                db_backend: "goleveldb".into(),
                genesis_file: "config/genesis.json".into()
            }
        );
        Ok(())
    }
}
