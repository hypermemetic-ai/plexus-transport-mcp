# What a tool is

The bridge is a client of the service. `tools/list` is the service's own
schema as the caller sees it, and `tools/call` is a call as that caller, so
the service's gate decides it as it decides a WebSocket call.

| in the service | over MCP |
| -- | -- |
| a method, `echo.count` | the tool `echo.count`; its `inputSchema` is the method's parameters, an object |
| a child's method | the tool `echo.stats.total`, named by its path |
| a method whose `http_method` is `GET` | `readOnlyHint: true` |
| a method that streams | one result, its items collected |
| `Progress` items | `notifications/progress`, when the client gave a token |
| an `Error` item, a closed socket, a limit reached | `isError: true`, `structuredContent.error.{code,message}`, and the items collected so far |

## Who is calling

The bridge checks no credential itself. The `Authorization: Bearer` of an
MCP request is the one it presents to the service.

| the request carries | the caller |
| -- | -- |
| no credential | the anonymous caller: the tools the service lists to everyone |
| a credential the service accepts | that caller: the tools it may call |
| a credential the service refuses | none: the answer is `401` |
