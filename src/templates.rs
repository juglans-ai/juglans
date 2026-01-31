// src/templates.rs

use include_dir::{include_dir, Dir};

// 【修改】增加 [server] 和 workspace 资源配置部分
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
agents = ["agents/**/*.jgagent"]
workflows = ["workflows/**/*.jgflow"]
prompts = ["prompts/**/*.jgprompt"]
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

[env]
DEBUG = "true"
"#;

pub static PROJECT_TEMPLATE_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/examples");
pub static DOCS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/docs");
