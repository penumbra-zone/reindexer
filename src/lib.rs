mod cometbft;

/// This is a utility around re-indexing historical Penumbra events.
#[derive(clap::Parser)]
#[command(version)]
pub enum Opt {
    /// Test that we can call into Go.
    Test(Test),
    /// Create or add to our full historical archive of blocks.
    Archive(Archive),
}

impl Opt {
    /// Run this command.
    pub fn run(self) {
        match self {
            Opt::Test(x) => x.run(),
            Opt::Archive(x) => x.run(),
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

#[derive(clap::Parser)]
pub struct Archive {
    /// A starting point for reading and writing penumbra data.
    ///
    /// The equivalent of pd's --network-dir.
    ///
    /// Read usage can be overriden with --cometbft-data-dir.
    ///
    /// Write usage can be overriden with --archive-file.
    ///
    /// In this directory we expect there to be:
    ///   - ./cometbft/config/config.toml, for reading cometbft configuration,
    ///   - ./cometbft/data/, for reading cometbft data,
    ///   - (maybe) ./archive.bin, for existing archive data to append to.
    ///
    /// If unset, defaults to ~/.penumbra/network_data/node0.
    #[clap(long)]
    home: Option<String>,
    /// If set, use this directory for cometbft, instead of HOME/cometbft/.
    #[clap(long)]
    cometbft_dir: Option<String>,
    /// If set, use this file for archive data, instead of HOME/archive.bin.
    #[clap(long)]
    archive_file: Option<String>,
}

impl Archive {
    /// Create or add to our full historical archive of blocks.
    pub fn run(self) {
        todo!()
    }
}
