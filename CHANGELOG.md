# Changelog

All notable changes to plexus-transport-mcp are documented here. This file
starts at 0.2.0; 0.1.0 (TRANSPORT-16, the first release) is in the git history.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] — unreleased

NEST-24: on plexus-client 0.11 (NEST-14, NEST-16, NEST-17), so plexus-transport
0.10's `mcp` feature has one plexus-client in its tree.

### Breaking

| Change | Before (0.4.0) | After | What to do |
|---|---|---|---|
| plexus-client | 0.10 | 0.11 | Move a backend that hands the bridge a `plexus_client::Transport` to plexus-client 0.11 |

## [0.4.0]

On plexus-client 0.10 (ASK-2, ASK-5, ASK-1): the in-call question (bidir) is
gone from the wire, with nothing in its place — Plexus has no in-call or
protocol-wide confirmation.

### Breaking

| Change | Before (0.3.0) | After | What to do |
|---|---|---|---|
| plexus-client | 0.9 | 0.10 | Move a backend that hands the bridge a `plexus_client::Transport` to plexus-client 0.10 |
| a `request` item (bidir) | logged at debug and dropped | gone from the wire (plexus-client 0.10 has no `StreamItem::Request`) | Nothing: a 0.10 backend sends none |

## [0.3.0] — 2026-11-06

On plexus-client 0.9 (PLEXUS-46), so plexus-transport 0.8's `mcp` feature —
which hands the bridge plexus-core 0.9's in-process `HubTransport` as a
`plexus_client::Transport` — has one plexus-client in its tree.

### Breaking

| Change | Before (0.2.0) | After | What to do |
|---|---|---|---|
| plexus-client | 0.8 | 0.9 | Move a backend that hands the bridge a `plexus_client::Transport` to plexus-client 0.9 |

## [0.2.0]

On plexus-client 0.8 (RETURNS-36), the 0.8 line's typed frames.

### Breaking

| Change | Before (0.1.0) | After | What to do |
|---|---|---|---|
| plexus-client | 0.7 | 0.8 | Move a backend that hands the bridge a `plexus_client::Transport` to plexus-client 0.8 |
| `tools::ToolRef.streaming` | the method's declared `streaming` flag | removed: a 0.8 schema has no such flag, and a tool's call collects every item either way | Drop reads of the field and its struct-literal entry |

### Changed

- A progress frame's `content` is the method's progress value. The default
  `Note` maps to an MCP progress notification's message and percentage; any
  other progress type is sent as its JSON text, with no percentage.
- An error frame with no `message` or `code` falls back to its typed
  `RemoteError`: its `Display` as the message, its kind as the code. This
  applies to a tool call's failure and to the schema reads behind
  `tools/list`.
