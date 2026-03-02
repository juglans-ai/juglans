# Installation Guide

This guide covers the various ways to install Juglans.

## System Requirements

- **Operating System**: Linux, macOS, Windows
- **Rust**: 1.70+ (for building from source)
- **Memory**: 4GB+ recommended

## Build from Source

### 1. Install Rust

```bash
# Install using rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify installation
rustc --version
cargo --version
```

### 2. Clone the Repository

```bash
git clone https://github.com/juglans-ai/juglans.git
cd juglans
```

### 3. Build

```bash
# Debug build (for development)
cargo build

# Release build (for production)
cargo build --release
```

### 4. Install

**Option A: Add to PATH**

```bash
# Temporary
export PATH="$PATH:$(pwd)/target/release"

# Permanent (add to shell config)
echo 'export PATH="$PATH:/path/to/juglans/target/release"' >> ~/.bashrc
source ~/.bashrc
```

**Option B: System install**

```bash
cargo install --path .
```

**Option C: Copy the binary**

```bash
sudo cp target/release/juglans /usr/local/bin/
```

### 5. Verify

```bash
juglans --version
juglans --help
```

## Pre-built Binaries

Download from [Releases](https://github.com/juglans-ai/juglans/releases):

### macOS (Apple Silicon)

```bash
curl -LO https://github.com/juglans-ai/juglans/releases/latest/download/juglans-darwin-arm64.tar.gz
tar -xzf juglans-darwin-arm64.tar.gz
sudo mv juglans /usr/local/bin/
```

### macOS (Intel)

```bash
curl -LO https://github.com/juglans-ai/juglans/releases/latest/download/juglans-darwin-x64.tar.gz
tar -xzf juglans-darwin-x64.tar.gz
sudo mv juglans /usr/local/bin/
```

### Linux (x64)

```bash
curl -LO https://github.com/juglans-ai/juglans/releases/latest/download/juglans-linux-x64.tar.gz
tar -xzf juglans-linux-x64.tar.gz
sudo mv juglans /usr/local/bin/
```

### Windows

1. Download `juglans-windows-x64.zip`
2. Extract to the target directory
3. Add the directory to the system PATH

## Docker

### Using the Official Image

```bash
docker pull juglans/juglans:latest

# Run
docker run -it --rm \
  -v $(pwd):/workspace \
  juglans/juglans:latest \
  /workspace/workflow.jg
```

### Build a Custom Image

```dockerfile
# Dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/juglans /usr/local/bin/
ENTRYPOINT ["juglans"]
```

```bash
docker build -t my-juglans .
docker run -it --rm my-juglans --help
```

## WebAssembly

For browser environments:

```bash
# Install wasm-pack
cargo install wasm-pack

# Build WASM
wasm-pack build --target web
```

Using in JavaScript:

```javascript
import init, { parse_workflow, render_prompt } from './juglans_wasm.js';

await init();

const workflow = parse_workflow(workflowSource);
const rendered = render_prompt(promptSource, { name: "Alice" });
```

## Development Environment Setup

### VS Code

Recommended extensions:

- **rust-analyzer** - Rust language support
- **TOML Language Support** - TOML syntax highlighting
- **Better TOML** - Enhanced TOML support

### Syntax Highlighting

Add syntax highlighting for `.jg`, `.jgprompt`, `.jgagent` files:

```json
// settings.json
{
  "files.associations": {
    "*.jg": "yaml",
    "*.jgprompt": "yaml",
    "*.jgagent": "yaml"
  }
}
```

## Dependency Services

### Jug0 Backend

Juglans requires the Jug0 backend for LLM calls:

```bash
# Local development
git clone https://github.com/juglans-ai/jug0.git
cd jug0
cargo run

# Or use the cloud service
# Configure base_url in juglans.toml
```

### MCP Server (Optional)

Install MCP tool servers:

```bash
# Anthropic official MCP
npm install -g @anthropic/mcp-filesystem
npm install -g @anthropic/mcp-github

# Fetch tool schemas
juglans install
```

## Configuration

Create a configuration file:

```bash
# User configuration
mkdir -p ~/.config/juglans
cat > ~/.config/juglans/juglans.toml << 'EOF'
[account]
id = "your_user_id"
api_key = "your_api_key"

[jug0]
base_url = "http://localhost:3000"
EOF
```

## FAQ

### Q: Build fails — OpenSSL error

**macOS:**
```bash
brew install openssl
export OPENSSL_DIR=$(brew --prefix openssl)
cargo build --release
```

**Ubuntu/Debian:**
```bash
sudo apt install libssl-dev pkg-config
cargo build --release
```

### Q: Permission denied

```bash
chmod +x /usr/local/bin/juglans
```

### Q: Command not found

Make sure PATH is correct:
```bash
echo $PATH | grep -q juglans || echo "Juglans not in PATH"
which juglans
```

### Q: GLIBC version too old (Linux)

Use a statically linked build:
```bash
RUSTFLAGS='-C target-feature=+crt-static' cargo build --release --target x86_64-unknown-linux-gnu
```

## Updating

### Update from Source

```bash
cd juglans
git pull
cargo build --release
```

### Using cargo

```bash
cargo install --path . --force
```

## Uninstalling

### Manually installed

```bash
sudo rm /usr/local/bin/juglans
```

### Installed via cargo

```bash
cargo uninstall juglans
```

### Clean up configuration

```bash
rm -rf ~/.config/juglans
rm -f ./juglans.toml
```

## Next Steps

- [Quick Start](./quickstart.md) - Create your first workflow
- [Core Concepts](../guide/concepts.md) - Understand the Juglans architecture
