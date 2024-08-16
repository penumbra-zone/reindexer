use std::{os::raw::c_void, path::Path};

#[link(name = "cometbft", kind = "static")]
extern "C" {
    fn c_store_new(dir_ptr: *const u8, dir_len: i32) -> *const c_void;
    fn c_store_message_a(ptr: *const c_void);
    fn c_store_message_b(ptr: *const c_void);
    fn c_store_delete(ptr: *const c_void);
}

struct RawStore {
    handle: *const c_void,
}

impl RawStore {
    fn new(dir: &Path) -> Self {
        let os_str_bytes = dir.as_os_str().as_encoded_bytes();
        let handle = unsafe {
            // Safety: the Go side of things will immediately copy the data, and not write into it,
            // or read past the provided bounds.
            c_store_new(
                os_str_bytes.as_ptr(),
                i32::try_from(os_str_bytes.len()).expect("directory should fit into an i32"),
            )
        };
        Self { handle }
    }

    fn message_a(&mut self) {
        unsafe {
            // Safety: because we take mutable ownership, we avoid any shenanigans on the Go side.
            c_store_message_a(self.handle);
        }
    }

    fn message_b(&mut self) {
        unsafe {
            // Safety: because we take mutable ownership, we avoid any shenanigans on the Go side.
            c_store_message_b(self.handle);
        }
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
    pub fn new(dir: &Path) -> Self {
        Self {
            raw: RawStore::new(dir),
        }
    }

    pub fn message_a(&mut self) {
        self.raw.message_a()
    }

    pub fn message_b(&mut self) {
        self.raw.message_b()
    }
}
