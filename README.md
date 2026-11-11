# plexus-transport-mcp

Any Plexus RPC service as an MCP server: every method a tool, and every
call decided by the service's own login.

## Most people want plexus-rpc

A service takes one dependency, [plexus-rpc](https://github.com/hypermemetic-ai/plexus-rpc),
and serves MCP from its own `main` with the feature `mcp`. Its README shows
that. This repository is the bridge behind it, and a binary that puts MCP
beside a service that is already running.

## Install

The binary:

```sh
cargo install plexus-transport-mcp
```

The library, without the binary's dependencies:

```toml
[dependencies]
plexus-transport-mcp = { version = "0.5", default-features = false }
```

## Beside a running service

`myservice` listens on `ws://127.0.0.1:4444`. Nothing in it changes.

<!-- examples/beside.txt -->
```console
$ plexus-transport-mcp --url ws://127.0.0.1:4444 --service myservice --listen 127.0.0.1:4445
$ claude mcp add --transport http myservice http://127.0.0.1:4445/mcp
Added HTTP MCP server myservice with URL: http://127.0.0.1:4445/mcp to local config
File modified: ~/.claude.json [project: ~/project]
$ claude mcp list
Checking MCP server health…

myservice: http://127.0.0.1:4445/mcp (HTTP) - ✔ Connected
```

[`scripts/example.sh`](scripts/example.sh) runs this and writes what was
printed; `cargo test --test readme` holds this page to it.
[`tests/sidecar.rs`](tests/sidecar.rs) lists and calls the bridge's tools
as a caller who holds a role, one who holds a lesser one, one who is not
signed in, and one the service refuses.

## Reference

- [What a tool is](docs/tools.md): how a method, a child, a stream and an error reach an agent.
- [Configuration](docs/configuration.md): flags, the TOML file, stdio, OAuth, limits.
- [docs.rs/plexus-transport-mcp](https://docs.rs/plexus-transport-mcp)
- [CHANGELOG.md](CHANGELOG.md)
