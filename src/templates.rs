// src/templates.rs
//
// TPL_TOML is a plain string and always available. The two `include_dir!`
// statics are gated behind the `cli` feature because they compile-in the
// `examples/` and `docs/` directories, which `Cargo.toml` excludes from
// the published crate tarball — library consumers (default-features = false)
// would otherwise fail to build after downloading from crates.io.

// Added [server] and workspace resource configuration sections
pub const TPL_TOML: &str = r#"[account]
id = "u_demo"
name = "Demo User"
role = "admin"
api_key = ""

[workspace]
id = "ws_default"
name = "My Workspace"
members = ["u_demo"]

# Resource paths (glob patterns supported)
workflows = ["workflows/**/*.jg"]
prompts = ["prompts/**/*.jgx"]
tools = ["tools/**/*.json"]

# Exclude patterns
exclude = [
  "**/*.backup",
  "**/.draft",
  "**/test_*"
]

[server]
host = "127.0.0.1"
port = 3000

# [paths]
# Enable @ path alias for imports (e.g. "@/prompts/*.jgx")
# @ resolves to {project_root}/{base}
# base = "."

[env]
DEBUG = "true"
"#;

#[cfg(feature = "cli")]
pub static PROJECT_TEMPLATE_DIR: include_dir::Dir =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/examples");

#[cfg(feature = "cli")]
pub static DOCS_DIR: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/docs");
