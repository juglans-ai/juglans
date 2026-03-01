// src/registry/installer.rs
//
// Package installer: orchestrates download → cache → project linking.
// Implements the pnpm-style global cache + local symlink model.

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

use super::cache::PackageCache;
use super::client::RegistryClient;
use super::lock::{LockFile, LockedPackage};
use super::package::{parse_registry_import, PackageManifest};

/// Info about a successfully installed package
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    /// Absolute path to the package's entry .jg file
    pub entry_path: PathBuf,
    /// Absolute path to the package directory in the global cache
    pub package_dir: PathBuf,
}

pub struct PackageInstaller {
    client: RegistryClient,
    cache: PackageCache,
}

impl PackageInstaller {
    pub fn _new(client: RegistryClient, cache: PackageCache) -> Self {
        Self { client, cache }
    }

    /// Create an installer with default config (reads registry URL from config or uses default)
    pub fn with_defaults(registry_url: &str) -> Result<Self> {
        let client = RegistryClient::new(registry_url);
        let cache = PackageCache::new()?;
        Ok(Self { client, cache })
    }

    /// Install a single package: resolve version → download → extract → link.
    /// Returns info about the installed package.
    pub async fn install(
        &self,
        name: &str,
        version_req: Option<&str>,
        project_dir: &Path,
    ) -> Result<InstalledPackage> {
        // 1. Resolve version
        let version = self
            .client
            .resolve_version(name, version_req)
            .await
            .with_context(|| format!("Failed to resolve version for '{}'", name))?;

        info!("Installing {name}@{version} ...");

        // 2. Download if not cached
        if !self.cache.is_cached(name, &version) {
            let tmp_dir = std::env::temp_dir().join("juglans_downloads");
            let archive = self
                .client
                .download(name, &version, &tmp_dir)
                .await
                .with_context(|| format!("Failed to download {}-{}", name, version))?;

            // 3. Extract to global cache
            self.cache
                .extract(name, &version, &archive)
                .with_context(|| format!("Failed to extract {}-{}", name, version))?;

            // Clean up downloaded archive
            let _ = fs::remove_file(&archive);
        } else {
            info!("{name}@{version} already cached, skipping download");
        }

        // 4. Create local symlink
        self.link_to_project(name, &version, project_dir)?;

        let package_dir = self.cache.package_dir(name, &version);
        let entry_path = self
            .cache
            .entry_path(name, &version)
            .with_context(|| format!("Failed to find entry for {}-{}", name, version))?;

        Ok(InstalledPackage {
            name: name.to_string(),
            version,
            entry_path,
            package_dir,
        })
    }

    /// Ensure a package is installed (download if not cached, link if not linked).
    /// This is the "auto-install" path used by the resolver at runtime.
    pub async fn _ensure_installed(
        &self,
        name: &str,
        version_req: Option<&str>,
        project_dir: &Path,
    ) -> Result<InstalledPackage> {
        // Check if already linked in jg_modules
        let link_path = project_dir.join("jg_modules").join(name);
        if link_path.exists() {
            // Already linked — resolve entry from the linked directory
            let real_dir = link_path
                .canonicalize()
                .with_context(|| format!("Failed to resolve jg_modules/{} symlink", name))?;
            let entry_path = super::cache::find_entry_in_dir(&real_dir)?;
            // Extract version from the cache path (parent dir name)
            let version = real_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            return Ok(InstalledPackage {
                name: name.to_string(),
                version,
                entry_path,
                package_dir: real_dir,
            });
        }

        // Not linked → full install
        self.install(name, version_req, project_dir).await
    }

    /// Create jg_modules/{name} → ~/.juglans/packages/{name}/{version}/ symlink
    fn link_to_project(&self, name: &str, version: &str, project_dir: &Path) -> Result<()> {
        let jg_modules = project_dir.join("jg_modules");
        fs::create_dir_all(&jg_modules).with_context(|| {
            format!("Failed to create jg_modules/ in {}", project_dir.display())
        })?;

        let link_path = jg_modules.join(name);
        let target = self.cache.package_dir(name, version);

        // Remove existing link/dir if present
        if link_path.exists() || link_path.symlink_metadata().is_ok() {
            if link_path.is_dir()
                && !link_path
                    .symlink_metadata()
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
            {
                fs::remove_dir_all(&link_path)?;
            } else {
                fs::remove_file(&link_path)?;
            }
        }

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link_path).with_context(|| {
            format!(
                "Failed to create symlink {} → {}",
                link_path.display(),
                target.display()
            )
        })?;

        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target, &link_path).with_context(|| {
            format!(
                "Failed to create symlink {} → {}",
                link_path.display(),
                target.display()
            )
        })?;

        info!("Linked jg_modules/{} → {}", name, target.display());

        Ok(())
    }

    /// Install all dependencies from jgpackage.toml in the given project directory.
    /// Respects lock file for pinned versions and resolves transitive dependencies.
    pub async fn install_all(&self, project_dir: &Path) -> Result<Vec<InstalledPackage>> {
        let manifest_path = project_dir.join("jgpackage.toml");
        if !manifest_path.exists() {
            return Err(anyhow!(
                "jgpackage.toml not found in {}",
                project_dir.display()
            ));
        }

        let manifest = PackageManifest::load(&manifest_path)?;
        let mut lock = LockFile::load(project_dir)?;
        let locked_versions = lock.to_map();

        let mut installed = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // Collect all direct dependencies
        let mut queue: Vec<(String, String)> = manifest
            .dependencies
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        while let Some((dep_name, version_req)) = queue.pop() {
            if seen.contains(&dep_name) {
                continue;
            }
            seen.insert(dep_name.clone());

            // Use locked version if available, otherwise resolve from constraint
            let effective_req = if let Some(locked_ver) = locked_versions.get(&dep_name) {
                Some(format!("={}", locked_ver))
            } else {
                Some(version_req.clone())
            };

            let pkg = self
                .install(&dep_name, effective_req.as_deref(), project_dir)
                .await
                .with_context(|| format!("Failed to install dependency '{}'", dep_name))?;

            // Read installed package's own dependencies (transitive)
            let pkg_manifest_path = pkg.package_dir.join("jgpackage.toml");
            let transitive_deps = if pkg_manifest_path.exists() {
                let pkg_manifest = PackageManifest::load(&pkg_manifest_path)?;
                pkg_manifest.dependencies
            } else {
                std::collections::HashMap::new()
            };

            // Record to lock file
            lock.upsert(LockedPackage {
                name: dep_name.clone(),
                version: pkg.version.clone(),
                checksum: None,
                dependencies: transitive_deps
                    .iter()
                    .map(|(k, v)| format!("{}@{}", k, v))
                    .collect(),
            });

            // Enqueue transitive dependencies
            for (trans_name, trans_ver) in transitive_deps {
                if !seen.contains(&trans_name) {
                    queue.push((trans_name, trans_ver));
                }
            }

            installed.push(pkg);
        }

        // Save lock file
        lock.save(project_dir)?;

        Ok(installed)
    }

    /// Remove a package: delete symlink from jg_modules/ (does not remove global cache)
    pub fn unlink(&self, name: &str, project_dir: &Path) -> Result<()> {
        let link_path = project_dir.join("jg_modules").join(name);
        if link_path.exists() || link_path.symlink_metadata().is_ok() {
            if link_path.is_dir()
                && !link_path
                    .symlink_metadata()
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
            {
                fs::remove_dir_all(&link_path)?;
            } else {
                fs::remove_file(&link_path)?;
            }
            info!("Removed jg_modules/{}", name);
        }
        Ok(())
    }

    /// Parse a registry import string and install.
    /// Handles both "name" and "name@version" formats.
    /// Updates the lock file after installation.
    pub async fn install_from_import(
        &self,
        import: &str,
        project_dir: &Path,
    ) -> Result<InstalledPackage> {
        let (name, version_req) = parse_registry_import(import)?;
        let pkg = self
            .install(&name, version_req.as_deref(), project_dir)
            .await?;

        // Update lock file
        let mut lock = LockFile::load(project_dir).unwrap_or_default();

        // Read transitive dependencies from installed package
        let pkg_manifest_path = pkg.package_dir.join("jgpackage.toml");
        let deps = if pkg_manifest_path.exists() {
            PackageManifest::load(&pkg_manifest_path)
                .map(|m| m.dependencies)
                .unwrap_or_default()
        } else {
            std::collections::HashMap::new()
        };

        lock.upsert(LockedPackage {
            name: pkg.name.clone(),
            version: pkg.version.clone(),
            checksum: None,
            dependencies: deps.iter().map(|(k, v)| format!("{}@{}", k, v)).collect(),
        });
        let _ = lock.save(project_dir);

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_installed_package_struct() {
        let pkg = InstalledPackage {
            name: "test-pkg".to_string(),
            version: "1.0.0".to_string(),
            entry_path: PathBuf::from("/tmp/test/lib.jg"),
            package_dir: PathBuf::from("/tmp/test"),
        };
        assert_eq!(pkg.name, "test-pkg");
        assert_eq!(pkg.version, "1.0.0");
    }
}
