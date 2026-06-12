use anyhow::Result;
use clap::Parser;
use docref::cli::{Cli, run};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    run(cli)
}
