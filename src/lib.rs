mod cometbft;

/// This is a utility around re-indexing historical Penumbra events.
#[derive(clap::Parser)]
#[command(version)]
pub enum Opt {
    /// Test that we can call into Go.
    Test(Test),
}

impl Opt {
    /// Run this command.
    pub fn run(self) {
        match self {
            Opt::Test(x) => x.run(),
        }
    }
}

#[derive(clap::Parser)]
pub struct Test {}

impl Test {
    /// Print out a test message, calling into Go via FFI.
    pub fn run(self) {
        cometbft::print_hello();
        println!("Hello, world!");
    }
}
