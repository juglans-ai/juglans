use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::services::config::JuglansConfig;

const BASE_IMAGE: &str = "juglansai/juglans:latest";
const CONTAINER_PREFIX: &str = "juglans";
const DEFAULT_PORT: u16 = 8080;

/// File extensions to include in build context
const INCLUDE_EXTS: &[&str] = &[
    "jg", "jgx", "jgprompt", "json", "yaml", "yml", "toml", "csv", "txt", "py", "md",
];

/// Directories to exclude from build context
const EXCLUDE_DIRS: &[&str] = &[".git", "target", "node_modules", "__pycache__"];

// ── Entry point ──────────────────────────────────────────────

/// Configuration for a deploy operation
pub struct DeployConfig {
    pub tag: Option<String>,
    pub port: Option<u16>,
    pub build_only: bool,
    pub push: bool,
    pub stop: bool,
    pub status: bool,
    pub env_vars: Vec<String>,
    pub path: Option<PathBuf>,
}

pub fn handle_deploy(config: DeployConfig) -> Result<()> {
    let DeployConfig {
        tag,
        port,
        build_only,
        push,
        stop,
        status,
        env_vars,
        path,
    } = config;
    check_docker_available()?;

    let project_root = resolve_project_root(path.as_deref())?;
    let project_name = derive_project_name(&project_root)?;
    let container_name = format!("{}-{}", CONTAINER_PREFIX, project_name);
    let port = port.unwrap_or(DEFAULT_PORT);
    let image_tag = tag.unwrap_or_else(|| format!("{}:latest", container_name));

    // Handle --stop
    if stop {
        return stop_container(&container_name);
    }

    // Handle --status
    if status {
        return show_container_status(&container_name);
    }

    // Build flow
    validate_project(&project_root)?;

    let build_dir = prepare_build_context(&project_root, &project_name, port)?;
    build_image(&build_dir, &image_tag)?;

    // Clean up temp dir
    let _ = fs::remove_dir_all(&build_dir);

    if push {
        push_image(&image_tag)?;
    }

    if build_only {
        println!("Image built: {}", image_tag);
        return Ok(());
    }

    // Collect env vars: juglans.toml [env] + CLI -e flags
    let mut all_env = load_config_env();
    for item in &env_vars {
        if let Some((k, v)) = item.split_once('=') {
            all_env.insert(k.to_string(), v.to_string());
        } else {
            return Err(anyhow!("Invalid env format '{}', expected KEY=VALUE", item));
        }
    }

    run_container(&container_name, &image_tag, port, &all_env)?;

    Ok(())
}

// ── Docker availability ──────────────────────────────────────

fn check_docker_available() -> Result<()> {
    let output = Command::new("docker")
        .arg("version")
        .arg("--format")
        .arg("{{.Server.Version}}")
        .output()
        .context("Failed to run 'docker'. Is Docker installed and in PATH?")?;

    if !output.status.success() {
        return Err(anyhow!(
            "Docker daemon is not running. Start Docker and try again."
        ));
    }
    Ok(())
}

// ── Project resolution ───────────────────────────────────────

fn resolve_project_root(path: Option<&Path>) -> Result<PathBuf> {
    let start = match path {
        Some(p) => {
            fs::canonicalize(p).with_context(|| format!("Path not found: {}", p.display()))?
        }
        None => env::current_dir().context("Cannot determine current directory")?,
    };

    let mut dir = start.as_path();
    loop {
        if dir.join("juglans.toml").exists() {
            return Ok(dir.to_path_buf());
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => {
                return Err(anyhow!(
                    "juglans.toml not found (searched from {})",
                    start.display()
                ))
            }
        }
    }
}

fn derive_project_name(root: &Path) -> Result<String> {
    // Try jgpackage.toml name first
    let manifest_path = root.join("jgpackage.toml");
    if manifest_path.exists() {
        let content = fs::read_to_string(&manifest_path)?;
        if let Ok(table) = content.parse::<toml::Table>() {
            if let Some(pkg) = table.get("package").and_then(|v| v.as_table()) {
                if let Some(name) = pkg.get("name").and_then(|v| v.as_str()) {
                    return Ok(sanitize_name(name));
                }
            }
        }
    }

    // Fallback: directory name
    root.file_name()
        .and_then(|n| n.to_str())
        .map(sanitize_name)
        .ok_or_else(|| anyhow!("Cannot derive project name from path"))
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .to_lowercase()
}

fn validate_project(root: &Path) -> Result<()> {
    let has_jg = fs::read_dir(root)?.filter_map(|e| e.ok()).any(|e| {
        e.path()
            .extension()
            .is_some_and(|ext| ext == "jg" || ext == "jgx" || ext == "jgprompt")
    });

    if !has_jg {
        return Err(anyhow!("No .jg/.jgx files found in {}", root.display()));
    }
    Ok(())
}

// ── Dockerfile generation ────────────────────────────────────

fn generate_dockerfile(port: u16) -> String {
    format!(
        r#"FROM {base}
COPY workspace/ /workspace/
WORKDIR /workspace
EXPOSE {port}
CMD ["juglans", "web", "--host", "0.0.0.0", "--port", "{port}"]
"#,
        base = BASE_IMAGE,
        port = port
    )
}

// ── Build context ────────────────────────────────────────────

fn prepare_build_context(project_root: &Path, project_name: &str, port: u16) -> Result<PathBuf> {
    let build_dir = env::temp_dir().join(format!("juglans-deploy-{}", project_name));

    // Clean previous build context
    if build_dir.exists() {
        fs::remove_dir_all(&build_dir)?;
    }

    let workspace_dir = build_dir.join("workspace");
    fs::create_dir_all(&workspace_dir)?;

    // Write Dockerfile
    fs::write(build_dir.join("Dockerfile"), generate_dockerfile(port))?;

    // Copy project files
    copy_project_files(project_root, &workspace_dir)?;

    Ok(build_dir)
}

fn copy_project_files(src: &Path, dst: &Path) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden files and excluded dirs
        if name_str.starts_with('.') {
            continue;
        }
        if EXCLUDE_DIRS.contains(&name_str.as_ref()) {
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(&name);

        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_project_files(&src_path, &dst_path)?;
        } else if should_include_file(&src_path) {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn should_include_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| INCLUDE_EXTS.contains(&ext))
}

// ── Docker operations ────────────────────────────────────────

fn build_image(build_dir: &Path, tag: &str) -> Result<()> {
    println!("Building image {} ...", tag);

    let status = Command::new("docker")
        .args(["build", "-t", tag, "."])
        .current_dir(build_dir)
        .status()
        .context("Failed to run docker build")?;

    if !status.success() {
        return Err(anyhow!("docker build failed"));
    }

    println!("Image built: {}", tag);
    Ok(())
}

fn run_container(
    name: &str,
    image: &str,
    port: u16,
    env_vars: &HashMap<String, String>,
) -> Result<()> {
    // Stop old container with same name (idempotent)
    let _ = Command::new("docker").args(["rm", "-f", name]).output();

    println!("Starting container {} ...", name);

    let mut args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        name.to_string(),
        "-p".to_string(),
        format!("{}:{}", port, port),
        "--restart".to_string(),
        "unless-stopped".to_string(),
    ];

    for (k, v) in env_vars {
        args.push("-e".to_string());
        args.push(format!("{}={}", k, v));
    }

    args.push(image.to_string());

    let output = Command::new("docker")
        .args(&args)
        .output()
        .context("Failed to run docker run")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("docker run failed: {}", stderr.trim()));
    }

    let container_id = String::from_utf8_lossy(&output.stdout)
        .trim()
        .chars()
        .take(12)
        .collect::<String>();

    println!("Container started: {} ({})", name, container_id);
    println!("  http://localhost:{}", port);
    println!("  Logs: docker logs -f {}", name);
    Ok(())
}

fn push_image(tag: &str) -> Result<()> {
    println!("Pushing {} ...", tag);

    let status = Command::new("docker")
        .args(["push", tag])
        .status()
        .context("Failed to run docker push")?;

    if !status.success() {
        return Err(anyhow!("docker push failed"));
    }

    println!("Pushed: {}", tag);
    Ok(())
}

fn show_container_status(name: &str) -> Result<()> {
    let output = Command::new("docker")
        .args([
            "inspect",
            "--format",
            "Status: {{.State.Status}}\nStarted: {{.State.StartedAt}}\nPorts: {{range $p, $conf := .NetworkSettings.Ports}}{{$p}} -> {{(index $conf 0).HostPort}} {{end}}",
            name,
        ])
        .output()
        .context("Failed to inspect container")?;

    if !output.status.success() {
        println!("Container '{}' not found.", name);
        return Ok(());
    }

    println!("Container: {}", name);
    println!("{}", String::from_utf8_lossy(&output.stdout).trim());
    Ok(())
}

fn stop_container(name: &str) -> Result<()> {
    println!("Stopping {} ...", name);

    let output = Command::new("docker")
        .args(["rm", "-f", name])
        .output()
        .context("Failed to stop container")?;

    if output.status.success() {
        println!("Stopped and removed: {}", name);
    } else {
        println!("Container '{}' not found or already stopped.", name);
    }
    Ok(())
}

// ── Config helpers ───────────────────────────────────────────

fn load_config_env() -> HashMap<String, String> {
    JuglansConfig::load().map(|c| c.env).unwrap_or_default()
}
