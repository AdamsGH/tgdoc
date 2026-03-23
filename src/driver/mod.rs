pub mod http;
pub mod git;

use anyhow::Result;
use crate::config::SourceConfig;

/// Raw data returned by a driver before parsing.
#[allow(dead_code)]
pub enum RawData {
    /// Map of URL path -> HTML text (http driver).
    Html(std::collections::HashMap<String, String>),
    /// Path to a local git clone (git driver).
    Repo(std::path::PathBuf),
}

pub async fn fetch(cfg: &SourceConfig) -> Result<RawData> {
    match cfg.driver.as_str() {
        "http" => http::fetch(cfg).await,
        "git"  => git::fetch(cfg).await,
        other  => anyhow::bail!("unknown driver: {}", other),
    }
}
