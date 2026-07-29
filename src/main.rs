use anyhow::Result;
use clap::Parser;
use cratectx::cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    cratectx::run(cli)?;
    Ok(())
}
