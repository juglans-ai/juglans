// src/registry/client.rs
//
// HTTP client for the Juglans package registry.
// Talks to the independent registry server to fetch package info and download archives.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Response from GET /api/v1/packages/{name}
#[derive(Debug, Deserialize)]
pub struct PackageResponse {
    #[serde(rename = "name")]
    pub _name: String,
    #[serde(rename = "slug")]
    pub _slug: String,
    #[serde(rename = "description")]
    pub _description: Option<String>,
    #[serde(rename = "author")]
    pub _author: Option<String>,
    #[serde(rename = "license")]
    pub _license: Option<String>,
    pub versions: Vec<VersionEntry>,
}

#[derive(Debug, Deserialize)]
pub struct VersionEntry {
    pub version: String,
    #[serde(rename = "entry")]
    pub _entry: Option<String>,
    #[serde(rename = "checksum")]
    pub _checksum: Option<String>,
    #[serde(rename = "published_at")]
    pub _published_at: Option<String>,
}

pub struct RegistryClient {
    base_url: String,
    http: reqwest::Client,
}

impl RegistryClient {
    pub fn new(registry_url: &str) -> Self {
        Self {
            base_url: registry_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Fetch all version info for a package
    pub async fn fetch_package_info(&self, name: &str) -> Result<PackageResponse> {
        let url = format!("{}/api/v1/packages/{}", self.base_url, name);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("Failed to reach registry at {}", url))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(anyhow!("Package '{}' not found in registry", name));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Registry error for '{}': {} — {}",
                name,
                status,
                body
            ));
        }

        resp.json::<PackageResponse>()
            .await
            .with_context(|| format!("Failed to parse registry response for '{}'", name))
    }

    /// Resolve a version constraint to an exact version.
    /// If version_req is None, returns the latest version.
    pub async fn resolve_version(&self, name: &str, version_req: Option<&str>) -> Result<String> {
        let info = self.fetch_package_info(name).await?;

        if info.versions.is_empty() {
            return Err(anyhow!("Package '{}' has no published versions", name));
        }

        match version_req {
            None | Some("latest") => {
                // Return the first version (registry returns newest first)
                Ok(info.versions[0].version.clone())
            }
            Some(req_str) => {
                let req = semver::VersionReq::parse(req_str)
                    .with_context(|| format!("Invalid version requirement: '{}'", req_str))?;

                // Find the best matching version (newest first)
                let mut candidates: Vec<(semver::Version, &VersionEntry)> = info
                    .versions
                    .iter()
                    .filter_map(|v| semver::Version::parse(&v.version).ok().map(|sv| (sv, v)))
                    .filter(|(sv, _)| req.matches(sv))
                    .collect();

                // Sort descending to pick newest match
                candidates.sort_by(|a, b| b.0.cmp(&a.0));

                candidates
                    .first()
                    .map(|(_, entry)| entry.version.clone())
                    .ok_or_else(|| {
                        anyhow!("No version of '{}' matches constraint '{}'", name, req_str)
                    })
            }
        }
    }

    /// Download a package archive to a temporary file.
    /// Returns the path to the downloaded .tar.gz.
    pub async fn download(&self, name: &str, version: &str, dest_dir: &Path) -> Result<PathBuf> {
        let url = format!(
            "{}/api/v1/packages/{}/{}/download",
            self.base_url, name, version
        );

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("Failed to download {}-{}", name, version))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Download failed for {}-{}: {} — {}",
                name,
                version,
                status,
                body
            ));
        }

        std::fs::create_dir_all(dest_dir)
            .with_context(|| format!("Failed to create dir {}", dest_dir.display()))?;

        let archive_path = dest_dir.join(format!("{}-{}.tar.gz", name, version));
        let bytes = resp.bytes().await?;
        std::fs::write(&archive_path, &bytes)
            .with_context(|| format!("Failed to write {}", archive_path.display()))?;

        Ok(archive_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_new() {
        let c = RegistryClient::new("https://jgr.juglans.ai/");
        assert_eq!(c.base_url, "https://jgr.juglans.ai");
    }

    #[test]
    fn test_client_url_no_trailing_slash() {
        let c = RegistryClient::new("https://jgr.juglans.ai");
        assert_eq!(c.base_url, "https://jgr.juglans.ai");
    }
}
