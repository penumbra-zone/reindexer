use clap::Parser;

fn main() -> anyhow::Result<()> {
    penumbra_reindexer::Opt::parse().run()
}
