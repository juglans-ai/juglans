// src/registry/cache.rs
//
// Global package cache at ~/.juglans/packages/{name}/{version}/
// Manages extraction of tar.gz archives and entry file resolution.

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use super::package::PackageManifest;

pub struct PackageCache {
    cache_dir: PathBuf,
}

impl PackageCache {
    /// Create a new cache rooted at ~/.juglans/packages/
    pub fn new() -> Result<Self> {
        let home = dirs_path()?;
        let cache_dir = home.join(".juglans").join("packages");
        fs::create_dir_all(&cache_dir)
            .with_context(|| format!("Failed to create cache dir {}", cache_dir.display()))?;
        Ok(Self { cache_dir })
    }

    /// Create a cache at a custom location (for testing)
    pub fn _with_dir(cache_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&cache_dir)
            .with_context(|| format!("Failed to create cache dir {}", cache_dir.display()))?;
        Ok(Self { cache_dir })
    }

    /// Check if a specific version of a package is cached
    pub fn is_cached(&self, name: &str, version: &str) -> bool {
        self.package_dir(name, version).exists()
    }

    /// Get the cache directory for a package version
    pub fn package_dir(&self, name: &str, version: &str) -> PathBuf {
        self.cache_dir.join(name).join(version)
    }

    /// Extract a tar.gz archive into the cache directory.
    /// Archives are expected to have a top-level directory prefix like {name}-{version}/
    /// Returns the path to the extracted package directory.
    pub fn extract(&self, name: &str, version: &str, archive: &Path) -> Result<PathBuf> {
        let target_dir = self.package_dir(name, version);

        // Remove existing if present (re-install)
        if target_dir.exists() {
            fs::remove_dir_all(&target_dir).with_context(|| {
                format!(
                    "Failed to remove existing cache at {}",
                    target_dir.display()
                )
            })?;
        }

        fs::create_dir_all(&target_dir)
            .with_context(|| format!("Failed to create cache dir {}", target_dir.display()))?;

        // Open and extract archive
        let file = fs::File::open(archive)
            .with_context(|| format!("Failed to open archive {}", archive.display()))?;
        let dec = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(dec);

        // Extract entries, stripping the top-level prefix directory
        for entry in tar.entries().context("Failed to read tar entries")? {
            let mut entry = entry.context("Failed to read tar entry")?;
            let path = entry.path().context("Invalid entry path")?.into_owned();

            // Strip the first component (e.g., "sqlite-tools-1.2.0/lib.jg" → "lib.jg")
            let stripped: PathBuf = path.components().skip(1).collect();
            if stripped.as_os_str().is_empty() {
                continue; // Skip the top-level directory entry itself
            }

            let dest = target_dir.join(&stripped);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            entry
                .unpack(&dest)
                .with_context(|| format!("Failed to extract {}", stripped.display()))?;
        }

        Ok(target_dir)
    }

    /// Get the entry file path for a cached package.
    /// Reads jgpackage.toml to determine the entry field (default: lib.jg).
    pub fn entry_path(&self, name: &str, version: &str) -> Result<PathBuf> {
        let pkg_dir = self.package_dir(name, version);
        find_entry_in_dir(&pkg_dir)
    }
}

/// Find the entry .jg file in a package directory.
/// Reads jgpackage.toml if present, otherwise defaults to lib.jg.
pub fn find_entry_in_dir(pkg_dir: &Path) -> Result<PathBuf> {
    let manifest_path = pkg_dir.join("jgpackage.toml");
    let entry_file = if manifest_path.exists() {
        let manifest = PackageManifest::load(&manifest_path)?;
        manifest.package.entry
    } else {
        "lib.jg".to_string()
    };

    let entry_path = pkg_dir.join(&entry_file);
    if !entry_path.exists() {
        return Err(anyhow!(
            "Entry file '{}' not found in package at {}",
            entry_file,
            pkg_dir.display()
        ));
    }

    Ok(entry_path)
}

/// Get the user's home directory
fn dirs_path() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("USERPROFILE").map(PathBuf::from))
        .map_err(|_| anyhow!("Cannot determine home directory"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_is_cached_false() {
        let dir = std::env::temp_dir().join("juglans_test_cache_empty");
        let _ = fs::remove_dir_all(&dir);
        let cache = PackageCache::_with_dir(dir.clone()).unwrap();
        assert!(!cache.is_cached("nonexistent", "1.0.0"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cache_package_dir() {
        let dir = std::env::temp_dir().join("juglans_test_cache_dir");
        let _ = fs::remove_dir_all(&dir);
        let cache = PackageCache::_with_dir(dir.clone()).unwrap();
        let pkg_dir = cache.package_dir("my-pkg", "1.2.3");
        assert_eq!(pkg_dir, dir.join("my-pkg").join("1.2.3"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cache_extract_and_entry() {
        let dir = std::env::temp_dir().join("juglans_test_cache_extract");
        let _ = fs::remove_dir_all(&dir);

        // Create a test archive with prefix directory
        let archive_dir = dir.join("archives");
        fs::create_dir_all(&archive_dir).unwrap();

        let archive_path = archive_dir.join("test-pkg-0.1.0.tar.gz");
        {
            let file = fs::File::create(&archive_path).unwrap();
            let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut tar = tar::Builder::new(enc);

            // Add jgpackage.toml under prefix
            let manifest = b"[package]\nname = \"test-pkg\"\nversion = \"0.1.0\"\n";
            let mut header = tar::Header::new_gnu();
            header.set_size(manifest.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, "test-pkg-0.1.0/jgpackage.toml", &manifest[..])
                .unwrap();

            // Add lib.jg under prefix
            let lib_content = b"name: \"test\"\nentry: [hello]\n[hello]: reply(message=\"hi\")\n";
            let mut header = tar::Header::new_gnu();
            header.set_size(lib_content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, "test-pkg-0.1.0/lib.jg", &lib_content[..])
                .unwrap();

            let enc = tar.into_inner().unwrap();
            enc.finish().unwrap();
        }

        // Extract
        let cache_dir = dir.join("cache");
        let cache = PackageCache::_with_dir(cache_dir).unwrap();
        let extracted = cache.extract("test-pkg", "0.1.0", &archive_path).unwrap();
        assert!(extracted.join("jgpackage.toml").exists());
        assert!(extracted.join("lib.jg").exists());
        assert!(cache.is_cached("test-pkg", "0.1.0"));

        // Entry path
        let entry = cache.entry_path("test-pkg", "0.1.0").unwrap();
        assert!(entry.ends_with("lib.jg"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_find_entry_in_dir_default() {
        let dir = std::env::temp_dir().join("juglans_test_find_entry");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // No jgpackage.toml, but lib.jg exists → should find it
        fs::write(dir.join("lib.jg"), "name: \"test\"\n").unwrap();
        let entry = find_entry_in_dir(&dir).unwrap();
        assert!(entry.ends_with("lib.jg"));

        let _ = fs::remove_dir_all(&dir);
    }
}
