use std::{os::raw::c_void, path::Path};
use tendermint::Block;
use tendermint_proto::Protobuf;

#[link(name = "cometbft", kind = "static")]
extern "C" {
    fn c_store_new(
        dir_ptr: *const u8,
        dir_len: i32,
        backend_ptr: *const u8,
        backend_len: i32,
    ) -> *const c_void;
    fn c_store_height(ptr: *const c_void) -> i64;
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

struct RawStore {
    handle: *const c_void,
    buf: Vec<u8>,
}

impl RawStore {
    fn new(backend: &str, dir: &Path) -> Self {
        let os_str_bytes = dir.as_os_str().as_encoded_bytes();
        let handle = unsafe {
            // Safety: the Go side of things will immediately copy the data, and not write into it,
            // or read past the provided bounds.
            c_store_new(
                os_str_bytes.as_ptr(),
                i32::try_from(os_str_bytes.len()).expect("directory should fit into an i32"),
                backend.as_ptr(),
                i32::try_from(backend.len()).expect("backend type should fit into an i32"),
            )
        };
        Self {
            handle,
            buf: Vec::with_capacity(EXPECTED_BLOCK_PROTO_SIZE),
        }
    }

    fn height(&mut self) -> i64 {
        unsafe {
            // Safety: because we take mutable ownership, we avoid any shenanigans on the Go side.
            c_store_height(self.handle)
        }
    }

    fn block_by_height(&mut self, height: i64) -> Option<&[u8]> {
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

pub struct Store {
    raw: RawStore,
}

impl Store {
    pub fn new(backend: &str, dir: &Path) -> Self {
        Self {
            raw: RawStore::new(backend, dir),
        }
    }

    pub fn height(&mut self) -> i64 {
        self.raw.height()
    }

    pub fn block_by_height(&mut self, height: i64) -> Option<Block> {
        self.raw.block_by_height(height).map(|block_data| {
            <Block as Protobuf<tendermint_proto::types::Block>>::decode_vec(block_data).unwrap()
        })
    }
}
