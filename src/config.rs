use anyhow::{bail, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(rename = "source")]
    pub sources: Vec<SourceConfig>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct SourceConfig {
    pub id: String,
    pub driver: String,
    pub parser: String,
    pub out: String,
    pub http: Option<HttpConfig>,
    pub git: Option<GitConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HttpConfig {
    pub base_url: String,
    pub proxy: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GitConfig {
    pub repo: String,
    #[serde(rename = "ref", default = "default_ref")]
    pub git_ref: String,
    /// Local path where the repo is cloned (relative to project root).
    /// Defaults to "repos/<id>".
    pub clone_dir: Option<String>,
}

fn default_ref() -> String {
    "master".to_string()
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&text)?;
        for src in &cfg.sources {
            src.validate()?;
        }
        Ok(cfg)
    }

    /// Return a single source by id, or error.
    pub fn get(&self, id: &str) -> Result<&SourceConfig> {
        self.sources.iter().find(|s| s.id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown source id: {}", id))
    }
}

impl SourceConfig {
    pub fn validate(&self) -> Result<()> {
        match self.driver.as_str() {
            "http" => {
                if self.http.is_none() {
                    bail!("source '{}': driver=http requires [source.http]", self.id);
                }
            }
            "git" => {
                if self.git.is_none() {
                    bail!("source '{}': driver=git requires [source.git]", self.id);
                }
            }
            other => bail!("source '{}': unknown driver '{}'", self.id, other),
        }
        Ok(())
    }

    /// Resolve the clone directory for a git source.
    pub fn clone_dir(&self) -> String {
        if let Some(g) = &self.git {
            g.clone_dir.clone()
                .unwrap_or_else(|| format!("repos/{}", self.id))
        } else {
            format!("repos/{}", self.id)
        }
    }
}

impl Config {

}
