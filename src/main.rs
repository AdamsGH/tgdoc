mod fetch;
mod convert;
mod pages;
mod anchor_index;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tgdoc", about = "Telegram API docs -> structured Markdown")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch all pages and write docs/
    Fetch {
        /// Print heading tree only, don't write files
        #[arg(long)]
        dry: bool,
        /// Proxy URL (e.g. http://127.0.0.1:8580)
        #[arg(long, default_value = "http://127.0.0.1:8580")]
        proxy: String,
        /// Output directory
        #[arg(long, default_value = "docs")]
        out: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Fetch { dry, proxy, out } => {
            pages::run(&proxy, &out, dry).await?;
        }
    }
    Ok(())
}
