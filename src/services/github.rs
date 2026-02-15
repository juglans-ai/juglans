// src/services/github.rs
//
// Fetch skill directories from GitHub repositories via the GitHub API.

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::fs;
use tracing::info;

/// A single file or directory entry from GitHub Contents API.
#[derive(Debug, Deserialize)]
struct GitHubContent {
    name: String,
    path: String,
    #[serde(rename = "type")]
    content_type: String,
    download_url: Option<String>,
}

/// A fetched skill directory with its files.
#[derive(Debug)]
pub struct FetchedSkill {
    pub name: String,
    pub local_dir: PathBuf,
}

/// Fetch skills from a GitHub repository.
///
/// `owner_repo`: e.g. "anthropics/skills"
/// `skill_names`: specific skill names to fetch, or empty for listing.
/// `output_dir`: local temp directory to save fetched files.
///
/// Skills are expected under `skills/` directory in the repo.
pub async fn fetch_skills(
    owner_repo: &str,
    skill_names: &[String],
    output_dir: &Path,
) -> Result<Vec<FetchedSkill>> {
    let (owner, repo) = owner_repo
        .split_once('/')
        .ok_or_else(|| anyhow!("Invalid repo format. Expected 'owner/repo', got '{}'", owner_repo))?;

    let client = Client::new();
    let mut results = Vec::new();

    if skill_names.is_empty() {
        return Err(anyhow!(
            "No skill specified. Use --skill <name> or --all to fetch skills.\n\
             Use 'juglans skills add {} --list' to see available skills.",
            owner_repo
        ));
    }

    for skill_name in skill_names {
        info!("Fetching skill '{}' from {}/{}...", skill_name, owner, repo);
        let skill_dir = output_dir.join(skill_name);
        fs::create_dir_all(&skill_dir)?;

        // Fetch the skill directory contents
        fetch_directory_recursive(
            &client,
            owner,
            repo,
            &format!("skills/{}", skill_name),
            &skill_dir,
        )
        .await
        .with_context(|| format!("Failed to fetch skill '{}'", skill_name))?;

        results.push(FetchedSkill {
            name: skill_name.clone(),
            local_dir: skill_dir,
        });
    }

    Ok(results)
}

/// List available skills in a GitHub repository.
pub async fn list_remote_skills(owner_repo: &str) -> Result<Vec<String>> {
    let (owner, repo) = owner_repo
        .split_once('/')
        .ok_or_else(|| anyhow!("Invalid repo format. Expected 'owner/repo'"))?;

    let client = Client::new();
    let url = format!(
        "https://api.github.com/repos/{}/{}/contents/skills",
        owner, repo
    );

    let resp = client
        .get(&url)
        .header("User-Agent", "juglans-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .with_context(|| format!("Failed to reach GitHub API: {}", url))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(
            "GitHub API returned {}: {}",
            status,
            body
        ));
    }

    let entries: Vec<GitHubContent> = resp.json().await?;

    let mut skill_names: Vec<String> = entries
        .into_iter()
        .filter(|e| e.content_type == "dir")
        .map(|e| e.name)
        .collect();

    skill_names.sort();
    Ok(skill_names)
}

/// Recursively fetch a directory from GitHub and save to local path.
async fn fetch_directory_recursive(
    client: &Client,
    owner: &str,
    repo: &str,
    remote_path: &str,
    local_dir: &Path,
) -> Result<()> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/contents/{}",
        owner, repo, remote_path
    );

    let resp = client
        .get(&url)
        .header("User-Agent", "juglans-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .with_context(|| format!("GitHub API request failed: {}", url))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(
            "GitHub API returned {} for {}: {}",
            status,
            remote_path,
            body
        ));
    }

    let entries: Vec<GitHubContent> = resp.json().await?;

    for entry in entries {
        if entry.content_type == "file" {
            if let Some(download_url) = &entry.download_url {
                let file_content = client
                    .get(download_url)
                    .header("User-Agent", "juglans-cli")
                    .send()
                    .await?
                    .text()
                    .await?;

                let file_path = local_dir.join(&entry.name);
                fs::write(&file_path, &file_content)?;
                info!("  Downloaded: {}", entry.name);
            }
        } else if entry.content_type == "dir" {
            let sub_dir = local_dir.join(&entry.name);
            fs::create_dir_all(&sub_dir)?;
            Box::pin(fetch_directory_recursive(
                client,
                owner,
                repo,
                &entry.path,
                &sub_dir,
            ))
            .await?;
        }
    }

    Ok(())
}
