#[link(name = "cometbft", kind="static")]
extern "C" {
    fn printHello();
}

fn print_hello() {
    unsafe { printHello() }
}

fn main() {
    print_hello();
    println!("Hello, world!");
}
