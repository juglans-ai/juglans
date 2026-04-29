# Tutorial 9: HTTP Client (httpx-style)

Juglans provides a full-featured HTTP client library inspired by Python's httpx. Import it via `libs: ["http"]`.

> This tutorial is a cookbook — feel free to skim it for the verbs you need. The next tutorial (the full project) wires this together with everything else.

## Basic Requests

### GET

```juglans
libs: ["http"]

[users]: http.get(url="https://api.example.com/users")
```

Access response fields:

```juglans
libs: ["http"]

[r]: http.get(url="https://api.example.com/users")
[show]: print(message=output.json)
[r] -> [show]
```

### POST with JSON

```juglans
libs: ["http"]

[create]: http.post(url="https://api.example.com/users", json='{"name": "Alice", "age": 30}')
```

## Query Parameters

Pass `params` as a JSON object. They are appended to the URL automatically:

```juglans
libs: ["http"]

[search]: http.get(url="https://api.example.com/search", params='{"q": "juglans", "page": 1, "limit": 20}')
```

This sends a request to `https://api.example.com/search?q=juglans&page=1&limit=20`.

## Authentication

### Bearer Token

```juglans
libs: ["http"]

[data]: http.get(url="https://api.example.com/me", auth="Bearer sk-xxx-your-token")
```

### Basic Auth

```juglans
libs: ["http"]

[data]: http.get(url="https://api.example.com/admin", auth="admin:password123")
```

## Custom Headers

```juglans
libs: ["http"]

[r]: http.get(url="https://api.example.com/data", headers='{"X-API-Key": "abc123", "Accept": "application/json"}')
```

## Form Data

URL-encoded form submission:

```juglans
libs: ["http"]

[login]: http.post(url="https://example.com/login", data='{"username": "admin", "password": "secret"}')
```

## File Upload

Multipart file upload:

```juglans
libs: ["http"]

[upload]: http.post(url="https://example.com/upload", files='{"document": "/path/to/report.pdf"}')
```

## Timeout

Set request timeout in seconds:

```juglans
libs: ["http"]

[slow_api]: http.get(url="https://slow-api.example.com/data", timeout=60)
```

## Redirect Control

Disable automatic redirect following:

```juglans
libs: ["http"]

[check]: http.head(url="https://example.com/short-link", follow_redirects=false)
```

## Cookies

```juglans
libs: ["http"]

[r]: http.get(url="https://example.com/dashboard", cookies='{"session": "abc123", "lang": "en"}')
```

## Response Format

All HTTP functions return a response object with these fields:

| Field | Type | Description |
|-------|------|-------------|
| `status_code` | number | HTTP status code (200, 404, etc.) |
| `headers` | object | Response headers |
| `json` | any | Parsed JSON body (null if not JSON) |
| `text` | string | Raw response body as text |
| `url` | string | Final URL (after redirects) |
| `is_success` | boolean | true if status is 200-299 |
| `elapsed` | number | Request duration in seconds |
| `content_type` | string | Response content-type header |

## Using http_request() Directly

You can also use the `http_request()` builtin without the stdlib wrapper:

```juglans
[r]: http_request(url="https://example.com/api", method="OPTIONS", headers='{"Origin": "https://app.com"}')
```

## All HTTP Methods

```juglans
libs: ["http"]

[g]: http.get(url="https://httpbin.org/get")
[p]: http.post(url="https://httpbin.org/post", json='{"key": "value"}')
[u]: http.put(url="https://httpbin.org/put", json='{"key": "updated"}')
[a]: http.patch(url="https://httpbin.org/patch", json='{"key": "patched"}')
[d]: http.delete(url="https://httpbin.org/delete")
[h]: http.head(url="https://httpbin.org/get")
[o]: http.options(url="https://httpbin.org/get")
```

---

**Next:** [Full Project: AI Assistant](./full-project.md) — combines AI chat, branching, prompts, composition, error handling, and HTTP into one deployable bot.
