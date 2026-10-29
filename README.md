# plexus-transport-mcp

Any Plexus RPC backend as an MCP server (TRANSPORT-16).

The bridge is a **client** of Plexus RPC. On one side it speaks
`plexus_client::Transport`; on the other, MCP (rmcp 3.x, streamable HTTP and
stdio). It validates no credential of its own: the `Authorization: Bearer`
it is shown is the one it presents to the backend, so the backend's
validator and gate decide every call exactly as they decide a WebSocket
call. `tools/list` is what the backend's `hashes` and `<ns>.schema` answer
that caller — the caller's view (AUTH-4), read off the wire.

## Beside a running backend

```sh
plexus-transport-mcp --url ws://127.0.0.1:4455 --hub menger --listen 127.0.0.1:4459
# MCP at http://127.0.0.1:4459/mcp — nothing on the backend changes
```

For an MCP client that runs a command:

```sh
plexus-transport-mcp --url ws://127.0.0.1:4455 --hub menger --stdio --bearer-file ~/.plexus/mcp.token
```

## From a file

```toml
# mcp.toml — every key optional; flags win over the file
path = "/mcp"
listen = "127.0.0.1:4459"
allowed_hosts = ["app.menger.sh"]

[upstream]
url = "ws://127.0.0.1:4455"
hub = "menger"

[oauth]                                   # /mcp as an OAuth 2.1 resource server
resource = "https://app.menger.sh/mcp"   # the audience a token must carry
authorization_servers = ["https://login.menger.sh"]

[limits]
items = 10000
bytes = 104857600
call_secs = 300
```

```sh
plexus-transport-mcp --config mcp.toml
```

With `[oauth]`, a request without a token is answered `401` with
`WWW-Authenticate: Bearer resource_metadata="…/.well-known/oauth-protected-resource/mcp"`,
and that document is served — which is how an MCP client finds the
authorization server and signs in. A token the backend refuses is a `401`
with `error="invalid_token"`. Without `[oauth]`, a request with no
credential is the anonymous caller (AUTH-6's anonymous view).

## Inside a backend

The same bridge over an in-process `Upstream` — plexus-transport's
`with_mcp(McpConfig)` (TRANSPORT-9 moves the consumers onto it).

## What a tool is

| schema | tool |
| -- | -- |
| `tickets.get` | `tickets.get`; `inputSchema` is the method's params with `type: object` at the root |
| a static child's method | `tickets.inputs.list` |
| `http_method = GET` | `readOnlyHint: true` |
| a `streaming` method | its items collected into the result (MCP's logging notifications, the one channel to relay them, are deprecated by SEP-2577) |
| `Progress` items | `notifications/progress`, when the client gave a token |
| an `Error` item, a closed socket, a cap of `[limits]` | `isError: true`, `structuredContent.error.{code,message}`, the items collected so far |
