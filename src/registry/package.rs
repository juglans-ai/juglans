// src/registry/package.rs
//
// jgpackage.toml parsing + packaging logic

use anyhow::{anyhow, Context, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Package manifest parsed from jgpackage.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: PackageInfo,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default = "default_entry")]
    pub entry: String,
}

fn default_entry() -> String {
    "lib.jg".to_string()
}

impl PackageManifest {
    /// Load from a jgpackage.toml file
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        Self::parse(&content)
    }

    /// Parse from TOML string
    pub fn parse(content: &str) -> Result<Self> {
        let manifest: PackageManifest =
            toml::from_str(content).context("Failed to parse jgpackage.toml")?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate required fields and constraints
    fn validate(&self) -> Result<()> {
        let pkg = &self.package;

        if pkg.name.is_empty() {
            return Err(anyhow!("package.name must not be empty"));
        }

        // Name must be lowercase alphanumeric + hyphens
        if !pkg
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(anyhow!(
                "package.name '{}' must contain only lowercase letters, digits, and hyphens",
                pkg.name
            ));
        }

        // Must be valid semver
        semver::Version::parse(&pkg.version)
            .with_context(|| format!("package.version '{}' is not valid semver", pkg.version))?;

        // Validate slug if provided
        if let Some(slug) = &pkg.slug {
            if !slug
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
            {
                return Err(anyhow!(
                    "package.slug '{}' must contain only lowercase letters, digits, hyphens, and underscores",
                    slug
                ));
            }
        }

        // Validate dependency version constraints
        for (dep_name, version_req) in &self.dependencies {
            parse_version_req(version_req).with_context(|| {
                format!(
                    "Invalid version requirement '{}' for dependency '{}'",
                    version_req, dep_name
                )
            })?;
        }

        Ok(())
    }

    /// Get the effective slug (falls back to package name)
    pub fn slug(&self) -> &str {
        self.package.slug.as_deref().unwrap_or(&self.package.name)
    }

    /// Get parsed semver version
    pub fn _version(&self) -> Result<semver::Version> {
        semver::Version::parse(&self.package.version).context("Invalid version")
    }
}

/// Parse a version requirement string (^1.2, ~1.0, >=1.0.0, =1.0.0, 1.0.0)
pub fn parse_version_req(req: &str) -> Result<semver::VersionReq> {
    semver::VersionReq::parse(req)
        .with_context(|| format!("Invalid version requirement: '{}'", req))
}

/// Check if a lib import string is a registry package (not a local path)
pub fn is_registry_import(import: &str) -> bool {
    // Local paths start with ./ or / or @/
    !(import.starts_with("./")
        || import.starts_with('/')
        || import.starts_with("@/")
        || import.ends_with(".jg")
        || import.ends_with(".jgflow"))
}

/// Parse a registry import string like "sqlite@^1.2.0" into (name, version_req)
pub fn parse_registry_import(import: &str) -> Result<(String, Option<String>)> {
    if let Some(at_pos) = import.find('@') {
        let name = &import[..at_pos];
        let version = &import[at_pos + 1..];
        if name.is_empty() {
            return Err(anyhow!("Empty package name in '{}'", import));
        }
        if version.is_empty() {
            return Err(anyhow!("Empty version in '{}'", import));
        }
        Ok((name.to_string(), Some(version.to_string())))
    } else {
        // No version specified → latest
        Ok((import.to_string(), None))
    }
}

/// Collect all files that should be included in the package archive.
/// Includes .jg/.jgflow, .jgagent, .jgprompt, jgpackage.toml, and README/LICENSE.
pub fn collect_package_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    let manifest_path = dir.join("jgpackage.toml");
    if !manifest_path.exists() {
        return Err(anyhow!("jgpackage.toml not found in {}", dir.display()));
    }
    files.push(manifest_path);

    collect_recursive(dir, dir, &mut files)?;
    Ok(files)
}

fn collect_recursive(_root: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        fs::read_dir(dir).with_context(|| format!("Failed to read directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        // Skip hidden dirs and common non-source dirs
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }

        if path.is_dir() {
            collect_recursive(_root, &path, files)?;
        } else {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let is_source = matches!(ext, "jg" | "jgflow" | "jgagent" | "jgprompt");
            let is_meta = matches!(
                name.as_ref(),
                "README.md"
                    | "readme.md"
                    | "LICENSE"
                    | "license"
                    | "LICENSE-MIT"
                    | "LICENSE-APACHE"
            );
            // jgpackage.toml already added above
            if is_source || is_meta {
                // Avoid duplicate jgpackage.toml
                if name != "jgpackage.toml" {
                    files.push(path);
                }
            }
        }
    }
    Ok(())
}

/// Pack a directory into a .tar.gz archive.
/// Returns the output path of the created archive.
pub fn pack(dir: &Path, output_dir: Option<&Path>) -> Result<PathBuf> {
    let manifest = PackageManifest::load(&dir.join("jgpackage.toml"))?;
    let files = collect_package_files(dir)?;

    // Validate entry file exists
    let entry_path = dir.join(&manifest.package.entry);
    if !entry_path.exists() {
        return Err(anyhow!(
            "Entry file '{}' not found in {}",
            manifest.package.entry,
            dir.display()
        ));
    }

    let archive_name = format!(
        "{}-{}.tar.gz",
        manifest.package.name, manifest.package.version
    );
    let out_dir = output_dir.unwrap_or(dir);
    let archive_path = out_dir.join(&archive_name);

    let file = fs::File::create(&archive_path)
        .with_context(|| format!("Failed to create {}", archive_path.display()))?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = tar::Builder::new(enc);

    let prefix = format!("{}-{}", manifest.package.name, manifest.package.version);

    for file_path in &files {
        let rel = file_path.strip_prefix(dir).unwrap_or(file_path);
        let archive_entry_path = format!("{}/{}", prefix, rel.display());
        tar.append_path_with_name(file_path, &archive_entry_path)
            .with_context(|| format!("Failed to add {} to archive", file_path.display()))?;
    }

    let enc = tar.into_inner().context("Failed to finalize tar")?;
    enc.finish().context("Failed to finalize gzip")?;

    Ok(archive_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_manifest_basic() {
        let toml = r#"
[package]
name = "sqlite-tools"
version = "1.2.0"
slug = "sqlite"
description = "SQLite utilities"
author = "ops"
license = "MIT"
entry = "lib.jgflow"

[dependencies]
http-tools = "^2.0"
"#;
        let m = PackageManifest::parse(toml).unwrap();
        assert_eq!(m.package.name, "sqlite-tools");
        assert_eq!(m.package.version, "1.2.0");
        assert_eq!(m.slug(), "sqlite");
        assert_eq!(m.package.entry, "lib.jgflow");
        assert_eq!(m.dependencies.get("http-tools").unwrap(), "^2.0");
    }

    #[test]
    fn test_parse_manifest_minimal() {
        let toml = r#"
[package]
name = "my-lib"
version = "0.1.0"
"#;
        let m = PackageManifest::parse(toml).unwrap();
        assert_eq!(m.package.name, "my-lib");
        assert_eq!(m.slug(), "my-lib"); // falls back to name
        assert_eq!(m.package.entry, "lib.jg"); // default
        assert!(m.dependencies.is_empty());
    }

    #[test]
    fn test_invalid_name_uppercase() {
        let toml = r#"
[package]
name = "MyLib"
version = "1.0.0"
"#;
        assert!(PackageManifest::parse(toml).is_err());
    }

    #[test]
    fn test_invalid_version() {
        let toml = r#"
[package]
name = "my-lib"
version = "not-semver"
"#;
        assert!(PackageManifest::parse(toml).is_err());
    }

    #[test]
    fn test_invalid_dependency_version() {
        let toml = r#"
[package]
name = "my-lib"
version = "1.0.0"

[dependencies]
bad-dep = ">>>invalid<<<"
"#;
        assert!(PackageManifest::parse(toml).is_err());
    }

    #[test]
    fn test_is_registry_import() {
        assert!(is_registry_import("sqlite@^1.2.0"));
        assert!(is_registry_import("http-tools@~2.0"));
        assert!(is_registry_import("my-lib"));
        assert!(is_registry_import("my-lib@latest"));

        assert!(!is_registry_import("./libs/sqlite.jgflow"));
        assert!(!is_registry_import("/absolute/path.jgflow"));
        assert!(!is_registry_import("@/tools/http.jgflow"));
    }

    #[test]
    fn test_parse_registry_import() {
        let (name, ver) = parse_registry_import("sqlite@^1.2.0").unwrap();
        assert_eq!(name, "sqlite");
        assert_eq!(ver, Some("^1.2.0".to_string()));

        let (name, ver) = parse_registry_import("my-lib").unwrap();
        assert_eq!(name, "my-lib");
        assert_eq!(ver, None);
    }

    #[test]
    fn test_parse_version_req_variants() {
        assert!(parse_version_req("^1.2.0").is_ok());
        assert!(parse_version_req("~1.0").is_ok());
        assert!(parse_version_req(">=1.0.0").is_ok());
        assert!(parse_version_req("=1.0.0").is_ok());
        assert!(parse_version_req("1.0.0").is_ok());
        assert!(parse_version_req("*").is_ok());
    }

    #[test]
    fn test_pack_creates_archive() {
        let dir = std::env::temp_dir().join("juglans_test_pack");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // Create jgpackage.toml
        fs::write(
            dir.join("jgpackage.toml"),
            r#"
[package]
name = "test-pkg"
version = "0.1.0"
entry = "lib.jgflow"
"#,
        )
        .unwrap();

        // Create entry file
        fs::write(
            dir.join("lib.jgflow"),
            "fn hello(): string\n  [greet]: reply(message=\"hi\")\n",
        )
        .unwrap();

        let archive_path = pack(&dir, None).unwrap();
        assert!(archive_path.exists());
        assert_eq!(
            archive_path.file_name().unwrap().to_str().unwrap(),
            "test-pkg-0.1.0.tar.gz"
        );

        // Verify it's a valid gzip
        let file = fs::File::open(&archive_path).unwrap();
        let dec = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(dec);
        let entries: Vec<_> = tar.entries().unwrap().collect();
        assert!(entries.len() >= 2); // at least jgpackage.toml + lib.jgflow

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_pack_missing_entry_file() {
        let dir = std::env::temp_dir().join("juglans_test_pack_no_entry");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(
            dir.join("jgpackage.toml"),
            r#"
[package]
name = "test-pkg"
version = "0.1.0"
entry = "lib.jgflow"
"#,
        )
        .unwrap();
        // No lib.jgflow created

        assert!(pack(&dir, None).is_err());

        let _ = fs::remove_dir_all(&dir);
    }
}
