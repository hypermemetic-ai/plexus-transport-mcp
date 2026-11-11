#!/usr/bin/env bash
# Runs the README's example and writes what was printed to
# examples/beside.txt: the bridge beside a running service, then Claude Code
# pointed at it. `cargo test --test readme` holds the README to that file.
#
# Needs a service named `myservice` on ws://127.0.0.1:4444 (plexus-rpc's
# examples/echo is one), claude on PATH, and port 4445 free. Claude's
# config goes to a home made for the run, shown as `~`.
set -euo pipefail

cd "$(dirname "$0")/.."
root=$PWD
run=$(mktemp -d)
home=$run/home
mkdir -p "$home/project"
out=$root/examples/beside.txt

cargo build

"$root/target/debug/plexus-transport-mcp" --url ws://127.0.0.1:4444 --service myservice --listen 127.0.0.1:4445 >"$run/bridge.log" 2>&1 &
bridge=$!
trap 'kill $bridge 2>/dev/null || true' EXIT
for _ in $(seq 100); do
    (exec 3<>/dev/tcp/127.0.0.1/4445) 2>/dev/null && break
    kill -0 "$bridge" 2>/dev/null || { cat "$run/bridge.log" >&2; exit 1; }
    sleep 0.1
done

show() {
    echo "\$ $*"
    (cd "$home/project" && HOME="$home" "$@" 2>&1) || true
}

{
    echo "\$ plexus-transport-mcp --url ws://127.0.0.1:4444 --service myservice --listen 127.0.0.1:4445"
    cat "$run/bridge.log"
    show claude mcp add --transport http myservice http://127.0.0.1:4445/mcp
    show claude mcp list
} | sed -e "s|$home|~|g" -e 's/\x1b\[[0-9;]*m//g' >"$out"

echo "written: examples/beside.txt"
