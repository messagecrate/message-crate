#!/usr/bin/env bash
# Release entrypoint: run the server.
#
# Nothing is seeded here. `message-crate-server serve` adds the Demo Account
# itself when the database does not exist yet, so a Message Crate started by
# Docker and one started any other way begin the same.
#
# Arguments run another server command in place of `serve`, with the stack
# stopped: docker compose run --rm server reset-demo --size large
#
# It writes nothing. The image carries the config at config/config.toml, the
# path every server command reads by default, and the Compose files make the
# root filesystem read-only, so the server writes under /app/data alone.
set -euo pipefail

cd /app

CONFIG="config/config.toml"

if [[ $# -gt 0 ]]; then
  exec message-crate-server "$@"
fi

echo "Starting message-crate-server (API + static files)…"
exec message-crate-server serve --config "${CONFIG}"
