# Installation

## Quick Install (Recommended)

### macOS (Homebrew)

```bash
brew tap juglans-ai/tap
brew install juglans
```

### macOS & Linux (Script)

```bash
curl -fsSL https://juglans.ai/get-sdk | sh
```

### Windows (PowerShell)

```powershell
irm https://juglans.ai/get-sdk.ps1 | iex
```

### Verify

```bash
juglans --version
```

You should see output like `juglans 0.2.6`. If you see `command not found`, check the [Troubleshooting](#troubleshooting) section below.

## Other Methods

### Pre-built Binaries

Download from [GitHub Releases](https://github.com/juglans-ai/juglans/releases):

| Platform | Download |
|----------|----------|
| macOS (Apple Silicon) | `juglans-darwin-arm64.tar.gz` |
| macOS (Intel) | `juglans-darwin-x64.tar.gz` |
| Linux (x64) | `juglans-linux-x64.tar.gz` |
| Windows (x64) | `juglans-windows-x64.zip` |

```bash
# Example: Linux x64
curl -LO https://github.com/juglans-ai/juglans/releases/latest/download/juglans-linux-x64.tar.gz
tar -xzf juglans-linux-x64.tar.gz
sudo mv juglans /usr/local/bin/
```

### Build from Source

Requires Rust 1.70+. If you don't have Rust, install it first:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then build juglans:

```bash
git clone https://github.com/juglans-ai/juglans.git
cd juglans
cargo install --path .
```

### Docker

```bash
docker pull ghcr.io/juglans-ai/juglans:latest

docker run -it --rm \
  -v $(pwd):/workspace \
  ghcr.io/juglans-ai/juglans:latest \
  /workspace/hello.jg
```

## Troubleshooting

**`command not found: juglans`**

Make sure the binary is in your PATH:
```bash
# Check where it was installed
which juglans || echo "Not in PATH"

# If installed via cargo:
export PATH="$HOME/.cargo/bin:$PATH"

# If installed manually:
export PATH="/usr/local/bin:$PATH"
```

**OpenSSL errors on Linux**

```bash
# Ubuntu/Debian
sudo apt install libssl-dev pkg-config

# Then rebuild
cargo install --path . --force
```

**Permission denied**

```bash
chmod +x /usr/local/bin/juglans
```

## Next Step

Now that Juglans is installed, let's create your first workflow: [Quick Start →](./quickstart.md)
