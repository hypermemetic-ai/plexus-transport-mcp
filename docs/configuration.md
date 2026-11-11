# Configuration

## Flags

| flag | what it is |
| -- | -- |
| `--url <URL>` | the service's WebSocket, `ws://127.0.0.1:4444` |
| `--service <SERVICE>` | the service's name |
| `--listen <ADDR>` | where MCP over HTTP is served |
| `--path <PATH>` | the endpoint's path; `/mcp` |
| `--stdio` | MCP on standard input and output, with no HTTP |
| `--bearer-file <FILE>` | with `--stdio`: a file holding the token presented to the service |
| `--allowed-host <HOST>` | a `Host` value to answer to; repeatable |
| `--resource <URL>` | this endpoint's public URL, as an OAuth 2.1 resource |
| `--authorization-server <URL>` | a server that mints tokens for it; repeatable |
| `--config <FILE>` | a TOML file; a flag wins over the file |

## For an MCP client that runs a command

```sh
plexus-transport-mcp --url ws://127.0.0.1:4444 --service myservice --stdio --bearer-file ~/.plexus/mcp.token
```

## A file

Every key is optional.

```toml
path = "/mcp"
listen = "127.0.0.1:4445"
allowed_hosts = ["mcp.example.com"]

[upstream]
url = "ws://127.0.0.1:4444"
service = "myservice"

[oauth]
resource = "https://mcp.example.com/mcp"
authorization_servers = ["https://login.example.com"]

[limits]
items = 10000
bytes = 104857600
call_secs = 300
```

```sh
plexus-transport-mcp --config mcp.toml
```

## OAuth

With `[oauth]`, `/mcp` is an OAuth 2.1 protected resource. `resource` is the
audience a token carries.

| request | answer |
| -- | -- |
| no token | `401`, with `WWW-Authenticate: Bearer resource_metadata="…/.well-known/oauth-protected-resource/mcp"`; that document is served, and names the authorization servers |
| a token the service refuses | `401`, with `error="invalid_token"` |

An MCP client reads the document and signs in by itself. Without `[oauth]`,
a request with no credential is the anonymous caller.

## Limits

A call that streams is collected into one result, up to `[limits]`: `items`
data items, `bytes` in all, `call_secs` seconds. A call that reaches one
answers an error carrying the items collected so far.

## In a program

`McpBridge::new(upstream, config)` is the bridge and `router(bridge)` is its
axum router. `SocketUpstream` reaches a service over its WebSocket; a
program that holds the service in memory implements `Upstream` over it.
`serve_stdio` serves the same bridge on standard input and output.
