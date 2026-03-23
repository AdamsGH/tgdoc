mod config;
mod fetch;
mod convert;
mod anchor_index;
mod driver;
mod source;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::Config;

#[derive(Parser)]
#[command(name = "tgdoc", about = "Telegram API docs -> structured Markdown")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch and convert docs (all sources, or a single one by id)
    Fetch {
        /// Source id from sources.toml (omit to run all)
        source: Option<String>,
        /// Print heading tree only, don't write files
        #[arg(long)]
        dry: bool,
        /// Output directory
        #[arg(long, default_value = "docs")]
        out: String,
        /// Path to sources.toml
        #[arg(long, default_value = "sources.toml")]
        config: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Fetch { source, dry, out, config } => {
            let cfg = Config::load(&config)?;
            let sources: Vec<_> = match &source {
                Some(id) => vec![cfg.get(id)?],
                None => cfg.sources.iter().collect(),
            };
            for src in sources {
                println!("\n[source] {} (driver={}, parser={})", src.id, src.driver, src.parser);
                let raw = driver::fetch(src).await?;
                source::run_parser(src, raw, &out, dry).await?;
            }
        }
    }
    Ok(())
}
