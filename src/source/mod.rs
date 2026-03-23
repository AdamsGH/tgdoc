pub mod tg_bot_api;

use anyhow::Result;
use crate::config::SourceConfig;
use crate::driver::RawData;

pub async fn run_parser(cfg: &SourceConfig, raw: RawData, out_dir: &str, dry: bool) -> Result<()> {
    match cfg.parser.as_str() {
        "tg-html" => tg_bot_api::run(cfg, raw, out_dir, dry).await,
        other => anyhow::bail!("unknown parser: {}", other),
    }
}
