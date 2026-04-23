# Installation

## Quick Install (Recommended)

### macOS & Linux (Script)

```bash
curl -fsSL https://raw.githubusercontent.com/juglans-ai/juglans/main/install.sh | sh
```

The script downloads the latest pre-built binary from GitHub Releases and installs it to `~/.juglans/bin/juglans`. If that directory is not already on your `PATH`, the script prints the one line you need to add to your shell profile.

### Windows

The `install.sh` script does not support Windows. Download `juglans-windows-x64.zip` directly from the [latest GitHub Release](https://github.com/juglans-ai/juglans/releases/latest), extract it, and add the folder containing `juglans.exe` to your `PATH`.

> Homebrew tap and a PowerShell installer are planned but not yet shipped — use the manual download path for now.

### Verify

```bash
juglans --version
```

You should see a version number like `juglans 0.2.16`. If you see `command not found`, check the [Troubleshooting](#troubleshooting) section below — the installer puts the binary at `~/.juglans/bin/juglans`, which may not be on your PATH yet.

Run `juglans --help` to see all subcommands. See [CLI Commands](../reference/cli.md) for the full reference.

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

Requires Rust 1.80+. If you don't have Rust, install it first:

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
