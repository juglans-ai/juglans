# Docker Deployment

## Documentation Site

The docs are built with [mdBook](https://rust-lang.github.io/mdBook/) and served via nginx.

### Build & Run

```bash
cd docs/
docker build -t juglans-docs .
docker run -d -p 3080:80 juglans-docs
```

Open [http://localhost:3080](http://localhost:3080) to view the docs.

### With Docker Compose

From the project root:

```bash
docker compose up docs
```

This builds the mdBook site and serves it on port `3080`.

---

## Workflow Runtime

The main `Dockerfile` in the project root packages the juglans CLI for running workflows via `juglans web`.

### Build

```bash
# Build the juglans binary first
cargo build --release

# Build the Docker image
docker build -t juglans .
```

### Run

```bash
# Run with a workspace mounted
docker run -d \
  -p 8080:8080 \
  -v $(pwd)/workspace:/workspace \
  juglans
```

This starts `juglans web` on port 8080 with your workspace files.

### With Docker Compose

```bash
docker compose up juglans
```

### Environment Variables

| Variable | Description | Default |
|---|---|---|
| `JUG0_BASE_URL` | jug0 backend URL | `http://localhost:3000` |
| `JUG0_API_KEY` | API key for jug0 | — |

Pass them via `-e` or in `docker-compose.yml`:

```bash
docker run -d \
  -p 8080:8080 \
  -e JUG0_BASE_URL=https://api.juglans.ai \
  -e JUG0_API_KEY=jug0_sk_xxx \
  -v $(pwd)/workspace:/workspace \
  juglans
```

---

## Docker Compose (Full Stack)

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

---

## CI/CD

### Validate Docs Before Deploy

Use `juglans doctest` to ensure all code examples in the docs are valid:

```bash
juglans doctest docs/
```

This extracts all ` ```juglans ` code blocks from markdown files and validates them through the parser. Any syntax errors will cause a non-zero exit code, making it suitable for CI pipelines.

### Example GitHub Actions

```yaml
- name: Validate doc examples
  run: juglans doctest docs/

- name: Build docs
  run: |
    cargo install mdbook
    cd docs && mdbook build --dest-dir ../target/book
```
