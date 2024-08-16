use clap::Parser;

fn main() {
    penumbra_reindexer::Opt::parse().run()
}
