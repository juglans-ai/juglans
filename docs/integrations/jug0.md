# Jug0 Backend Integration

Jug0 is the backend AI platform for Juglans, providing LLM invocation, resource storage, and other services.

## Overview

```
┌─────────────────┐         ┌─────────────────┐
│    Juglans      │  HTTP   │      Jug0       │
│    (Local)      │────────▶│    (Backend)    │
│                 │         │                 │
│  - Parse DSL    │         │  - LLM calls    │
│  - Execute      │         │  - Resource     │
│    workflows    │         │    storage      │
│  - Local        │         │  - User         │
│    resources    │         │    management   │
└─────────────────┘         └─────────────────┘
```

## Configure Connection

### juglans.toml

```toml
[account]
id = "your_user_id"
api_key = "jug0_sk_your_api_key"

[jug0]
base_url = "http://localhost:3000"  # Local development
# base_url = "https://api.jug0.com"  # Production
timeout = 30
```

### Environment Variables

```bash
export JUGLANS_API_KEY="jug0_sk_..."
export JUGLANS_JUG0_URL="http://localhost:3000"
```

## Authentication

### API Key Authentication

All requests must include an API Key:

```
Authorization: Bearer jug0_sk_your_api_key
```

### Obtaining an API Key

1. Log in to the Jug0 console
2. Go to Settings > API Keys
3. Create a new API Key
4. Copy it to juglans.toml

## Resource Management

### Push Resources

Upload local resources to Jug0:

```bash
# Push a Prompt
juglans push prompts/my-prompt.jgprompt

# Push an Agent
juglans push agents/my-agent.jgagent

# Push a Workflow
juglans push workflows/my-flow.jg

# Force overwrite
juglans push prompts/my-prompt.jgprompt --force
```

### Pull Resources

Download resources from Jug0:

```bash
# Pull a Prompt
juglans pull my-prompt --type prompt

# Pull to a specific directory
juglans pull my-agent --type agent --output ./agents/

# Pull all
juglans pull --all --output ./resources/
```

### List Resources

```bash
# List all Prompts
juglans list --type prompt

# List all Agents
juglans list --type agent

# List all Workflows
juglans list --type workflow
```

### Delete Resources

```bash
juglans delete my-prompt --type prompt
```

## Resource References

### GitHub-style Slugs

Jug0 uses the `owner/slug` format to identify resources:

```yaml
# Reference remote resources in workflows
[chat]: chat(agent="juglans/assistant", message=$input.query)
[render]: p(slug="juglans/greeting", name=$input.name)
```

### Local vs Remote

```yaml
# Local resources (imported via file)
prompts: ["./prompts/*.jgprompt"]
[render]: p(slug="my-local-prompt")

# Remote resources (fetched from Jug0)
[render]: p(slug="owner/remote-prompt")
```

### Mixed Usage

```yaml
name: "Hybrid Workflow"

# Import local resources
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [start]
exit: [end]

# Use a local Agent
[local_chat]: chat(agent="my-local-agent", message=$input.query)

# Use a remote Agent
[remote_chat]: chat(agent="juglans/premium-agent", message=$output)

[start] -> [local_chat] -> [remote_chat] -> [end]
```

## Chat API

### Basic Call

The `chat()` tool in workflows calls the Jug0 Chat API:

```yaml
[chat]: chat(
  agent="my-agent",
  message="Hello!",
  format="json"
)
```

### Streaming Response

Jug0 uses SSE (Server-Sent Events) to stream responses:

```
POST /api/chat
Content-Type: application/json

{
  "agent": "my-agent",
  "message": "Hello!",
  "stream": true
}
```

Response:

```
event: content
data: {"type": "content", "text": "Hello"}

event: content
data: {"type": "content", "text": "! How"}

event: content
data: {"type": "content", "text": " can I help?"}

event: done
data: {"type": "done", "tokens": 15}
```

### Non-streaming Response

```yaml
[chat]: chat(
  agent="my-agent",
  message="Hello!",
  stream="false"
)
```

## API Endpoints

### Prompts

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/prompts` | GET | List Prompts |
| `/api/prompts/:slug` | GET | Get a Prompt |
| `/api/prompts` | POST | Create a Prompt |
| `/api/prompts/:slug` | PUT | Update a Prompt |
| `/api/prompts/:slug` | DELETE | Delete a Prompt |
| `/api/prompts/:slug/render` | POST | Render a Prompt |

### Agents

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/agents` | GET | List Agents |
| `/api/agents/:slug` | GET | Get an Agent |
| `/api/agents` | POST | Create an Agent |
| `/api/agents/:slug` | PUT | Update an Agent |
| `/api/agents/:slug` | DELETE | Delete an Agent |

### Workflows

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/workflows` | GET | List Workflows |
| `/api/workflows/:slug` | GET | Get a Workflow |
| `/api/workflows` | POST | Create a Workflow |
| `/api/workflows/:slug/execute` | POST | Execute a Workflow |

### Chat

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/chat` | POST | Send a message (SSE) |
| `/api/chat/:id/stop` | POST | Stop generation |

### Resource (Unified Entry Point)

```
GET /api/r/:owner/:slug
```

Automatically identifies the resource type (Prompt/Agent/Workflow).

## Error Handling

### HTTP Status Codes

| Status Code | Description |
|-------------|-------------|
| 200 | Success |
| 400 | Bad request parameters |
| 401 | Unauthenticated |
| 403 | Unauthorized |
| 404 | Resource not found |
| 429 | Too many requests (rate limited) |
| 500 | Server error |

### Error Response Format

```json
{
  "error": {
    "code": "RESOURCE_NOT_FOUND",
    "message": "Prompt 'my-prompt' not found",
    "details": {}
  }
}
```

### Handling in Workflows

```yaml
[api_call]: chat(agent="external", message=$input)
[api_call] -> [success]
[api_call] on error -> [handle_error]

[handle_error]: notify(status="API call failed, using fallback")
[fallback]: chat(agent="local-fallback", message=$input)
[handle_error] -> [fallback]
```

## Local Development

### Start Local Jug0

```bash
git clone https://github.com/juglans-ai/jug0.git
cd jug0
cargo run
```

### Configure Connection

```toml
[jug0]
base_url = "http://localhost:3000"
```

### Development Mode

```bash
# Use local files without connecting to Jug0
juglans src/test.jg --offline

# Verbose logging
juglans src/test.jg --verbose
```

## Production Deployment

### Recommended Configuration

```toml
[jug0]
base_url = "https://api.jug0.com"
timeout = 60

[logging]
level = "warn"
format = "json"
```

### Health Check

```bash
curl https://api.jug0.com/health
```

### Monitoring

Jug0 provides Prometheus metrics:

```
GET /metrics
```

## Best Practices

1. **Version control** - Commit juglans.toml (without API Key) to Git
2. **Environment separation** - Use different API Keys for development/testing/production
3. **Error handling** - Add `on error` paths for network calls
4. **Timeout settings** - Adjust timeout based on task complexity
5. **Resource sync** - Periodically run `juglans pull` to keep local resources up to date
