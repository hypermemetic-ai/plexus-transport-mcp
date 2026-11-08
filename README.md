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
plexus-transport-mcp --url ws://127.0.0.1:4455 --service menger --listen 127.0.0.1:4459
# MCP at http://127.0.0.1:4459/mcp — nothing on the backend changes
```

For an MCP client that runs a command:

```sh
plexus-transport-mcp --url ws://127.0.0.1:4455 --service menger --stdio --bearer-file ~/.plexus/mcp.token
```

## From a file

```toml
# mcp.toml — every key optional; flags win over the file
path = "/mcp"
listen = "127.0.0.1:4459"
allowed_hosts = ["app.menger.sh"]

[upstream]
url = "ws://127.0.0.1:4455"
service = "menger"

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
`with_mcp(McpConfig)`, the service in memory (TRANSPORT-9 moves the consumers onto it).

## What a tool is

| schema | tool |
| -- | -- |
| `tickets.get` | `tickets.get`; `inputSchema` is the method's params with `type: object` at the root |
| a static child's method | `tickets.inputs.list` |
| `http_method = GET` | `readOnlyHint: true` |
| a `streaming` method | its items collected into the result (MCP's logging notifications, the one channel to relay them, are deprecated by SEP-2577) |
| `Progress` items | `notifications/progress`, when the client gave a token |
| an `Error` item, a closed socket, a cap of `[limits]` | `isError: true`, `structuredContent.error.{code,message}`, the items collected so far |
| a method that `asks` (`confirm` among its params) | an ordinary parameter of the tool |
| a `needs_input` item (a proposal, plexus-wire spec/08) | a successful result whose text says nothing was done, what would happen (summary, effects, fields) and that calling the same tool again with the same arguments plus `confirm: <token>` (and each field's value under its own name) carries it out; `structuredContent` is `{needs_input, confirm_with, items}`, `confirm_with` the confirming call's arguments. No elicitation |

A proposal's result, for `tickets.archive { ids: ["AMP-4", "AMP-5"] }`:

```text
Nothing was done yet: tickets.archive proposes this and waits for a confirmation.

Would: archive AMP-4, AMP-5

Effects:
- {"archive":"AMP-4"}
- {"archive":"AMP-5"}

Fields to fill, each a parameter of this tool, sent under its own name:
- reason (one of "done", "stale"; default "stale"): why they go

To carry it out, call tickets.archive again with the same arguments plus "confirm" and a value for each field above:
{"confirm":"v1.1794101342.XR_x…","ids":["AMP-4","AMP-5"]}

The token stands until 2026-11-08T01:29:02Z. Past then, or if anything it covers changes first, the call proposes again and does nothing.
```
