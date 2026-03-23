use std::path::PathBuf;
use anyhow::{Context, Result};
use crate::config::SourceConfig;
use crate::driver::RawData;

pub async fn fetch(cfg: &SourceConfig) -> Result<RawData> {
    let git = cfg.git.as_ref().expect("git config missing");
    let clone_dir = cfg.clone_dir();
    let path = PathBuf::from(&clone_dir);

    if path.join(".git").exists() {
        println!("[git] pull {} -> {}", git.repo, clone_dir);
        pull(&path, &git.git_ref)?;
    } else {
        println!("[git] clone {} -> {}", git.repo, clone_dir);
        clone(&git.repo, &path, &git.git_ref)?;
    }

    Ok(RawData::Repo(path))
}

fn clone(repo: &str, dest: &PathBuf, git_ref: &str) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let status = std::process::Command::new("git")
        .args([
            "clone",
            "--depth", "1",
            "--branch", git_ref,
            "--single-branch",
            repo,
            dest.to_str().unwrap_or("."),
        ])
        .status()
        .context("git clone failed")?;

    if !status.success() {
        anyhow::bail!("git clone exited with {}", status);
    }
    Ok(())
}

fn pull(repo_path: &PathBuf, _git_ref: &str) -> Result<()> {
    let status = std::process::Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(repo_path)
        .status()
        .context("git pull failed")?;

    if !status.success() {
        anyhow::bail!("git pull exited with {}", status);
    }
    Ok(())
}
