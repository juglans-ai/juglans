# How to Deploy with Docker

This guide covers building and deploying Juglans documentation and workflow runtime using Docker.

## Documentation Site

The `docs/` directory includes a Dockerfile that builds the mdBook site and serves it via Nginx.

```bash
cd docs/
docker build -t juglans-docs .
docker run -d -p 3080:80 juglans-docs
```

Open `http://localhost:3080` to view the docs.

## Workflow Runtime

The root `Dockerfile` packages the pre-built `juglans` binary for running workflows via `juglans serve`. The relevant directives are `COPY juglans /usr/local/bin/juglans` and `COPY workers/ /usr/local/bin/workers/` — it expects a **staged `docker-context/` directory** containing the Linux binary and Python workers, not the repo root directly.

The fastest path is `juglans deploy` (see the section below), which stages, builds, and runs the container in one command. Manual staging is shown next for transparency.

Prepare the context and build:

```bash
# Build the Linux binary (see note below for macOS)
cargo build --release --target x86_64-unknown-linux-gnu

# Stage the Docker build context
mkdir -p docker-context/workers
cp target/x86_64-unknown-linux-gnu/release/juglans docker-context/
cp src/workers/python_worker.py docker-context/workers/
cp Dockerfile docker-context/

# Build the image
docker build -t juglans docker-context/
```

The `juglans deploy` subcommand wraps this staging step if you prefer not to do it by hand.

> **macOS note:** `cargo build --target x86_64-unknown-linux-gnu` on macOS requires a cross-compilation toolchain. Use [`cross`](https://github.com/cross-rs/cross) (`cross build --release --target x86_64-unknown-linux-gnu`) or configure a `Cross.toml` in the repo root. Native `cargo build` on macOS produces a Mach-O binary that will not run inside a Linux container.

Run with a workspace mounted:

```bash
docker run -d \
  -p 8080:8080 \
  -v $(pwd)/workspace:/workspace \
  juglans
```

This starts `juglans web --host 0.0.0.0 --port 8080` with your workflow files in `/workspace`.

## Docker Compose

A `docker-compose.yml` in the project root provides both services:

```bash
# Start everything
docker compose up -d

# Only docs
docker compose up docs

# Only runtime
docker compose up juglans

# Rebuild after changes
docker compose up --build
```

The compose file:

```yaml
services:
  docs:
    build:
      context: ./docs
      dockerfile: Dockerfile
    ports:
      - "3080:80"

  juglans:
    build:
      context: .
      dockerfile: Dockerfile
    ports:
      - "8080:8080"
    volumes:
      - ./workspace:/workspace
    environment:
      - OPENAI_API_KEY=${OPENAI_API_KEY:-}
      - ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY:-}
      - DEEPSEEK_API_KEY=${DEEPSEEK_API_KEY:-}
      - QWEN_API_KEY=${QWEN_API_KEY:-}
```

## Environment Variables

Pass any LLM provider API key (juglans is local-first — providers are called directly):

| Variable | Description |
|----------|-------------|
| `OPENAI_API_KEY` | OpenAI API key |
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `DEEPSEEK_API_KEY` | DeepSeek API key |
| `GEMINI_API_KEY` | Google Gemini API key |
| `QWEN_API_KEY` | Alibaba Qwen API key |
| `XAI_API_KEY` | xAI API key |
| `ARK_API_KEY` | ByteDance Ark API key |

Pass via `-e` flag or in the compose file:

```bash
docker run -d \
  -p 8080:8080 \
  -e OPENAI_API_KEY=sk-... \
  -v $(pwd)/workspace:/workspace \
  juglans
```

## CI/CD with GitHub Actions

The project includes a `deploy-docker.yml` workflow that builds and pushes images on every version tag (`v*`).

Pipeline stages:

1. **CI Gate** -- format check, clippy, tests
2. **Build binary** -- `cargo build --release --target x86_64-unknown-linux-gnu`
3. **Build & push runtime image** -- to GHCR and Docker Hub
4. **Build & push docs image** -- to GHCR

Key steps in your own pipeline:

```yaml
# .github/workflows/deploy-docker.yml
on:
  push:
    tags: ['v*']

jobs:
  build-and-push:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-unknown-linux-gnu

      - run: cargo build --release --target x86_64-unknown-linux-gnu

      - name: Prepare Docker context
        run: |
          mkdir -p docker-context/workers
          cp target/x86_64-unknown-linux-gnu/release/juglans docker-context/
          cp src/workers/python_worker.py docker-context/workers/
          cp Dockerfile docker-context/

      - uses: docker/build-push-action@v5
        with:
          context: docker-context
          push: true
          tags: |
            ghcr.io/${{ github.repository_owner }}/juglans:latest
            ghcr.io/${{ github.repository_owner }}/juglans:${{ steps.version.outputs.version }}
```

Validate doc examples before deploying:

```bash
juglans doctest docs/
```

This extracts all ```` ```juglans ```` code blocks from markdown and runs them through the parser. Non-zero exit on syntax errors -- suitable for CI gates.

## Production Deployment

### systemd Service

```ini
# /etc/systemd/system/juglans.service
[Unit]
Description=Juglans Workflow Server
After=network.target

[Service]
Type=simple
User=juglans
WorkingDirectory=/opt/juglans
ExecStart=/usr/local/bin/juglans web --host 0.0.0.0 --port 8080
Restart=always
RestartSec=5
EnvironmentFile=/etc/juglans/llm.env  # contains OPENAI_API_KEY, ANTHROPIC_API_KEY, etc.

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable juglans
sudo systemctl start juglans
```

### Nginx Reverse Proxy

```nginx
upstream juglans {
    server 127.0.0.1:8080;
}

server {
    listen 443 ssl http2;
    server_name api.example.com;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;

    location / {
        proxy_pass http://juglans;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_buffering off;  # Required for SSE streaming
    }
}
```

`proxy_buffering off` is critical -- without it, SSE events will be buffered and not stream to the client in real time.
