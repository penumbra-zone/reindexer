#[link(name = "cometbft", kind = "static")]
extern "C" {
    fn printHello();
}

/// Print a hello world message.
///
/// This function exists only to test integration with our Go library.
pub fn print_hello() {
    unsafe { printHello() }
}
