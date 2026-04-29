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

Two paths land at a working container — pick by use case.

### Path A — `juglans deploy` (recommended)

The fastest way to ship a workflow project is the built-in `juglans deploy` subcommand. It generates a Dockerfile based on the public `juglansai/juglans:latest` base image, copies your `workspace/` directory in, and builds + runs the container — no manual staging.

```bash
juglans deploy                     # build + run
juglans deploy --port 9000         # custom host port
juglans deploy --build-only        # build only, don't start
juglans deploy --tag my-bot:v1     # custom image tag
juglans deploy --stop              # stop the running container
juglans deploy --status            # print container status
```

The generated Dockerfile is roughly:

```dockerfile
FROM juglansai/juglans:latest
COPY workspace/ /workspace/
WORKDIR /workspace
EXPOSE 8080
CMD ["juglans", "serve", "--host", "0.0.0.0", "--port", "8080"]
```

The base image already contains a Linux build of `juglans` plus the Python worker, so there is no cross-compile step on the host — `juglans deploy` works directly from macOS, Windows, or Linux.

### Path B — from-source build (root Dockerfile)

The root `Dockerfile` in the repo is what produces the public base image. It expects a **staged build context** with a pre-built Linux `juglans` binary plus the `workers/` directory at the context root — not the raw repo. Use this flow when you're building the base image itself or when you can't use `juglans deploy`:

```bash
# 1. Build the Linux binary (cross-compile from macOS — see note)
cargo build --release --target x86_64-unknown-linux-gnu

# 2. Stage the Docker build context
mkdir -p docker-context/workers
cp target/x86_64-unknown-linux-gnu/release/juglans docker-context/
cp src/workers/python_worker.py docker-context/workers/
cp Dockerfile docker-context/

# 3. Build the image
docker build -t juglans docker-context/

# 4. Run with workspace mounted
docker run -d -p 8080:8080 -v $(pwd)/workspace:/workspace juglans
```

The image's default `CMD` is `juglans serve --host 0.0.0.0 --port 8080`, which boots the HTTP API plus every configured channel in one process.

> **macOS note:** `cargo build --target x86_64-unknown-linux-gnu` on macOS needs a cross-compile toolchain. Use [`cross`](https://github.com/cross-rs/cross) (`cross build --release --target x86_64-unknown-linux-gnu`) or set up a `Cross.toml`. A native `cargo build` on macOS produces a Mach-O binary that won't run inside a Linux container.

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
      # Channel credentials (optional — only set those you want to run)
      - TELEGRAM_BOT_TOKEN=${TELEGRAM_BOT_TOKEN:-}
      - FEISHU_APP_ID=${FEISHU_APP_ID:-}
      - FEISHU_APP_SECRET=${FEISHU_APP_SECRET:-}
      - DISCORD_BOT_TOKEN=${DISCORD_BOT_TOKEN:-}
      # History storage (override defaults if you want SQLite or a custom path)
      - JUGLANS_HISTORY_BACKEND=${JUGLANS_HISTORY_BACKEND:-jsonl}
      - JUGLANS_HISTORY_DIR=${JUGLANS_HISTORY_DIR:-/workspace/.juglans/history}
    # CRITICAL: use `serve` to also boot every configured channel; `web` is HTTP-only.
    command: ["juglans", "serve", "--host", "0.0.0.0", "--port", "8080"]
```

> **Channels and serverless / scale-to-zero**. Discord holds a persistent Gateway WebSocket — it cannot survive container suspension. Run on a long-lived host (Cloud Run with `min-instances >= 1`, Fly.io without autostop, a regular VM, etc.). Telegram-polling and WeChat are long-poll based, also incompatible with idle suspension. The serverless-friendly options are Telegram-webhook (set `mode = "webhook"` and a public `server.endpoint_url`) and Feishu event-subscription — both are passive HTTP routes that wake on incoming requests.

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
| `ARK_API_KEY` | ByteDance / BytePlus Ark API key |

Channel credentials (only needed for the channels you want to run — `juglans serve` boots them automatically when the corresponding `[channels.<kind>.<id>]` section is present in `juglans.toml`, or the env var below auto-synthesizes one):

| Variable | Effect |
|----------|--------|
| `TELEGRAM_BOT_TOKEN` | Synthesizes `[channels.telegram.default]` if absent — channel starts on `juglans serve` |
| `FEISHU_APP_ID` / `FEISHU_APP_SECRET` | Synthesize `[channels.feishu.default]` (event-subscription mode) |
| `DISCORD_BOT_TOKEN` | NOT auto-consumed — reference via `${DISCORD_BOT_TOKEN}` interpolation in `[channels.discord.<id>].token` |

History storage overrides (default backend is JSONL at `.juglans/history/`):

| Variable | Effect |
|----------|--------|
| `JUGLANS_HISTORY_BACKEND` | `jsonl` (default), `sqlite`, `memory`, `none` |
| `JUGLANS_HISTORY_DIR` | JSONL directory; mount this to a volume for cross-restart persistence |
| `JUGLANS_HISTORY_PATH` | SQLite database path |
| `JUGLANS_HISTORY_MAX_MESSAGES` / `JUGLANS_HISTORY_MAX_TOKENS` / `JUGLANS_HISTORY_ENABLED` | Per-call limits + master switch |

> Mount the history directory as a Docker volume (`./workspace/.juglans/history:/workspace/.juglans/history`) so conversations survive container restarts.

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
ExecStart=/usr/local/bin/juglans serve --host 0.0.0.0 --port 8080
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
