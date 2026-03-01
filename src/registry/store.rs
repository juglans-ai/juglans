// src/registry/store.rs
//
// SQLite + filesystem storage layer for the package registry.

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageRecord {
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionRecord {
    pub package_name: String,
    pub version: String,
    pub entry: String,
    pub dependencies: String, // JSON string
    pub checksum: String,     // SHA-256 hex
    pub published_at: String,
}

pub struct RegistryStore {
    conn: Mutex<Connection>,
    data_dir: PathBuf,
}

impl RegistryStore {
    /// Open or create the registry store at the given data directory.
    pub fn open(data_dir: &Path) -> Result<Self> {
        fs::create_dir_all(data_dir)
            .with_context(|| format!("Failed to create data dir: {}", data_dir.display()))?;
        fs::create_dir_all(data_dir.join("packages"))
            .context("Failed to create packages dir")?;

        let db_path = data_dir.join("registry.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open database: {}", db_path.display()))?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS packages (
                name        TEXT PRIMARY KEY,
                slug        TEXT NOT NULL,
                description TEXT,
                author      TEXT,
                license     TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS versions (
                package_name TEXT NOT NULL,
                version      TEXT NOT NULL,
                entry        TEXT NOT NULL DEFAULT 'lib.jg',
                dependencies TEXT NOT NULL DEFAULT '{}',
                checksum     TEXT NOT NULL,
                published_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (package_name, version),
                FOREIGN KEY (package_name) REFERENCES packages(name)
            );

            CREATE INDEX IF NOT EXISTS idx_versions_package ON versions(package_name);
            ",
        )
        .context("Failed to initialize database schema")?;

        Ok(Self {
            conn: Mutex::new(conn),
            data_dir: data_dir.to_path_buf(),
        })
    }

    /// Publish a new package version.
    pub fn publish(
        &self,
        name: &str,
        version: &str,
        slug: &str,
        description: Option<&str>,
        author: Option<&str>,
        license: Option<&str>,
        entry: &str,
        dependencies: &serde_json::Value,
        archive_bytes: &[u8],
    ) -> Result<()> {
        // Validate semver
        semver::Version::parse(version)
            .with_context(|| format!("Invalid semver: {}", version))?;

        // Compute SHA-256
        let mut hasher = Sha256::new();
        hasher.update(archive_bytes);
        let checksum = format!("{:x}", hasher.finalize());

        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        // Check version doesn't already exist
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM versions WHERE package_name = ?1 AND version = ?2",
            params![name, version],
            |row| row.get(0),
        )?;
        if exists {
            return Err(anyhow!(
                "Version {} of package '{}' already exists",
                version,
                name
            ));
        }

        // Upsert package
        conn.execute(
            "INSERT INTO packages (name, slug, description, author, license)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(name) DO UPDATE SET
                 slug = ?2,
                 description = COALESCE(?3, description),
                 author = COALESCE(?4, author),
                 license = COALESCE(?5, license),
                 updated_at = datetime('now')",
            params![name, slug, description, author, license],
        )?;

        // Insert version
        conn.execute(
            "INSERT INTO versions (package_name, version, entry, dependencies, checksum)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, version, entry, dependencies.to_string(), checksum],
        )?;

        // Write archive to filesystem
        let archive_path = self.archive_path(name, version);
        if let Some(parent) = archive_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&archive_path, archive_bytes)
            .with_context(|| format!("Failed to write archive: {}", archive_path.display()))?;

        Ok(())
    }

    /// Get package record by name.
    pub fn get_package(&self, name: &str) -> Result<Option<PackageRecord>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT name, slug, description, author, license, created_at, updated_at
             FROM packages WHERE name = ?1",
        )?;
        let mut rows = stmt.query_map(params![name], |row| {
            Ok(PackageRecord {
                name: row.get(0)?,
                slug: row.get(1)?,
                description: row.get(2)?,
                author: row.get(3)?,
                license: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    /// Get all versions of a package, newest first.
    pub fn get_versions(&self, name: &str) -> Result<Vec<VersionRecord>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT package_name, version, entry, dependencies, checksum, published_at
             FROM versions WHERE package_name = ?1
             ORDER BY published_at DESC",
        )?;
        let rows = stmt.query_map(params![name], |row| {
            Ok(VersionRecord {
                package_name: row.get(0)?,
                version: row.get(1)?,
                entry: row.get(2)?,
                dependencies: row.get(3)?,
                checksum: row.get(4)?,
                published_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get a specific version record.
    pub fn get_version(&self, name: &str, version: &str) -> Result<Option<VersionRecord>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT package_name, version, entry, dependencies, checksum, published_at
             FROM versions WHERE package_name = ?1 AND version = ?2",
        )?;
        let mut rows = stmt.query_map(params![name, version], |row| {
            Ok(VersionRecord {
                package_name: row.get(0)?,
                version: row.get(1)?,
                entry: row.get(2)?,
                dependencies: row.get(3)?,
                checksum: row.get(4)?,
                published_at: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    /// Get the filesystem path for a version's archive.
    pub fn archive_path(&self, name: &str, version: &str) -> PathBuf {
        self.data_dir
            .join("packages")
            .join(name)
            .join(format!("{}.tar.gz", version))
    }

    /// Search packages by name, slug, or description.
    pub fn search(&self, query: &str) -> Result<Vec<PackageRecord>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT name, slug, description, author, license, created_at, updated_at
             FROM packages
             WHERE name LIKE ?1 OR slug LIKE ?1 OR description LIKE ?1
             ORDER BY updated_at DESC
             LIMIT 50",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok(PackageRecord {
                name: row.get(0)?,
                slug: row.get(1)?,
                description: row.get(2)?,
                author: row.get(3)?,
                license: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(suffix: &str) -> (RegistryStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("juglans_reg_test_{}", suffix));
        let _ = fs::remove_dir_all(&dir);
        let store = RegistryStore::open(&dir).unwrap();
        (store, dir)
    }

    #[test]
    fn test_open_creates_dirs_and_db() {
        let (_, dir) = temp_store("open");
        assert!(dir.join("registry.db").exists());
        assert!(dir.join("packages").is_dir());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_publish_and_get() {
        let (store, dir) = temp_store("pub_get");
        let deps = serde_json::json!({"http-tools": "^2.0"});
        store
            .publish(
                "sqlite-tools",
                "1.0.0",
                "sqlite",
                Some("SQLite utilities"),
                Some("ops"),
                Some("MIT"),
                "lib.jgflow",
                &deps,
                b"fake-archive-bytes",
            )
            .unwrap();

        let pkg = store.get_package("sqlite-tools").unwrap().unwrap();
        assert_eq!(pkg.name, "sqlite-tools");
        assert_eq!(pkg.slug, "sqlite");
        assert_eq!(pkg.description.as_deref(), Some("SQLite utilities"));

        let versions = store.get_versions("sqlite-tools").unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version, "1.0.0");
        assert!(!versions[0].checksum.is_empty());

        // Archive file should exist
        assert!(store.archive_path("sqlite-tools", "1.0.0").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_duplicate_version_rejected() {
        let (store, dir) = temp_store("dup");
        let deps = serde_json::json!({});
        store
            .publish(
                "my-lib", "1.0.0", "my-lib", None, None, None, "lib.jg", &deps, b"v1",
            )
            .unwrap();
        let result = store.publish(
            "my-lib", "1.0.0", "my-lib", None, None, None, "lib.jg", &deps, b"v2",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_multiple_versions() {
        let (store, dir) = temp_store("multi_ver");
        let deps = serde_json::json!({});
        store
            .publish(
                "my-lib", "1.0.0", "my-lib", None, None, None, "lib.jg", &deps, b"v1",
            )
            .unwrap();
        store
            .publish(
                "my-lib", "1.1.0", "my-lib", None, None, None, "lib.jg", &deps, b"v2",
            )
            .unwrap();

        let versions = store.get_versions("my-lib").unwrap();
        assert_eq!(versions.len(), 2);

        let v = store.get_version("my-lib", "1.0.0").unwrap().unwrap();
        assert_eq!(v.version, "1.0.0");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_search() {
        let (store, dir) = temp_store("search");
        let deps = serde_json::json!({});
        store
            .publish(
                "sqlite-tools",
                "1.0.0",
                "sqlite",
                Some("SQLite utilities"),
                None,
                None,
                "lib.jg",
                &deps,
                b"a",
            )
            .unwrap();
        store
            .publish(
                "http-client",
                "1.0.0",
                "http",
                Some("HTTP client"),
                None,
                None,
                "lib.jg",
                &deps,
                b"b",
            )
            .unwrap();

        let results = store.search("sqlite").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "sqlite-tools");

        let results = store.search("client").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "http-client");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_get_nonexistent() {
        let (store, dir) = temp_store("noexist");
        assert!(store.get_package("doesnt-exist").unwrap().is_none());
        assert!(store.get_version("doesnt-exist", "1.0.0").unwrap().is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
