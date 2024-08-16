use std::os::raw::c_void;

#[link(name = "cometbft", kind = "static")]
extern "C" {
    fn c_store_new() -> *const c_void;
    fn c_store_message_a(ptr: *const c_void);
    fn c_store_message_b(ptr: *const c_void);
    fn c_store_delete(ptr: *const c_void);
}

/// Print a hello world message.
///
/// This function exists only to test integration with our Go library.
pub fn print_hello() {
    unsafe {
        let handle = c_store_new();
        c_store_message_a(handle);
        c_store_message_b(handle);
        c_store_delete(handle);
    }
}
