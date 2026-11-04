# Changelog

All notable changes to plexus-transport-mcp are documented here. This file
starts at 0.2.0; 0.1.0 (TRANSPORT-16, the first release) is in the git history.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
