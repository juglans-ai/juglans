# 安装指南

本指南介绍 Juglans 的多种安装方式。

## 系统要求

- **操作系统**: Linux, macOS, Windows
- **Rust**: 1.70+ (从源码构建)
- **内存**: 建议 4GB+

## 从源码构建

### 1. 安装 Rust

```bash
# 使用 rustup 安装
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 验证安装
rustc --version
cargo --version
```

### 2. 克隆仓库

```bash
git clone https://github.com/juglans-ai/juglans.git
cd juglans
```

### 3. 构建

```bash
# Debug 构建（开发用）
cargo build

# Release 构建（生产用）
cargo build --release
```

### 4. 安装

**方式 A: 添加到 PATH**

```bash
# 临时添加
export PATH="$PATH:$(pwd)/target/release"

# 永久添加（写入 shell 配置）
echo 'export PATH="$PATH:/path/to/juglans/target/release"' >> ~/.bashrc
source ~/.bashrc
```

**方式 B: 系统安装**

```bash
cargo install --path .
```

**方式 C: 复制二进制**

```bash
sudo cp target/release/juglans /usr/local/bin/
```

### 5. 验证

```bash
juglans --version
juglans --help
```

## 预编译二进制

从 [Releases](https://github.com/juglans-ai/juglans/releases) 下载：

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

1. 下载 `juglans-windows-x64.zip`
2. 解压到目标目录
3. 添加目录到系统 PATH

## Docker

### 使用官方镜像

```bash
docker pull juglans/juglans:latest

# 运行
docker run -it --rm \
  -v $(pwd):/workspace \
  juglans/juglans:latest \
  /workspace/workflow.jgflow
```

### 构建自定义镜像

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

用于浏览器环境：

```bash
# 安装 wasm-pack
cargo install wasm-pack

# 构建 WASM
wasm-pack build --target web
```

在 JavaScript 中使用：

```javascript
import init, { parse_workflow, render_prompt } from './juglans_wasm.js';

await init();

const workflow = parse_workflow(workflowSource);
const rendered = render_prompt(promptSource, { name: "Alice" });
```

## 开发环境设置

### VS Code

推荐扩展：

- **rust-analyzer** - Rust 语言支持
- **TOML Language Support** - TOML 语法高亮
- **Better TOML** - TOML 增强

### 语法高亮

为 `.jgflow`, `.jgprompt`, `.jgagent` 文件添加语法高亮：

```json
// settings.json
{
  "files.associations": {
    "*.jgflow": "yaml",
    "*.jgprompt": "yaml",
    "*.jgagent": "yaml"
  }
}
```

## 依赖服务

### Jug0 后端

Juglans 需要 Jug0 后端进行 LLM 调用：

```bash
# 本地开发
git clone https://github.com/juglans-ai/jug0.git
cd jug0
cargo run

# 或使用云服务
# 在 juglans.toml 中配置 base_url
```

### MCP 服务器（可选）

安装 MCP 工具服务器：

```bash
# Anthropic 官方 MCP
npm install -g @anthropic/mcp-filesystem
npm install -g @anthropic/mcp-github

# 获取工具 schema
juglans install
```

## 配置

创建配置文件：

```bash
# 用户配置
mkdir -p ~/.config/juglans
cat > ~/.config/juglans/juglans.toml << 'EOF'
[account]
id = "your_user_id"
api_key = "your_api_key"

[jug0]
base_url = "http://localhost:3000"
EOF
```

## 常见问题

### Q: 构建失败 - OpenSSL 错误

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

### Q: 权限被拒绝

```bash
chmod +x /usr/local/bin/juglans
```

### Q: 找不到命令

确保 PATH 正确：
```bash
echo $PATH | grep -q juglans || echo "Juglans not in PATH"
which juglans
```

### Q: GLIBC 版本过低 (Linux)

使用静态链接构建：
```bash
RUSTFLAGS='-C target-feature=+crt-static' cargo build --release --target x86_64-unknown-linux-gnu
```

## 更新

### 从源码更新

```bash
cd juglans
git pull
cargo build --release
```

### 使用 cargo

```bash
cargo install --path . --force
```

## 卸载

### 手动安装的

```bash
sudo rm /usr/local/bin/juglans
```

### cargo 安装的

```bash
cargo uninstall juglans
```

### 清理配置

```bash
rm -rf ~/.config/juglans
rm -f ./juglans.toml
```

## 下一步

- [快速入门](./quickstart.md) - 创建第一个工作流
- [核心概念](../guide/concepts.md) - 了解 Juglans 架构
