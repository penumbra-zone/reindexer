use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = penumbra_reindexer::Opt::parse();
    penumbra_reindexer::Opt::init_console_tracing();
    opt.run().await
}
