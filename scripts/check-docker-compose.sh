#!/usr/bin/env bash
# Start docker/compose.release.yml on a release image already built, and check
# that it runs with its privileges dropped and serves the Demo Account.
#
#   docker build -f docker/Dockerfile -t message-crate-server:check .
#   ./scripts/check-docker-compose.sh message-crate-server:check [host-port]
#
# Both Compose files drop every Linux capability, set no-new-privileges and
# make the root filesystem read-only (#2178). This script checks that both
# carry the same settings, that the container Compose starts has them, and
# that the server still starts under them: the health check passes and the
# Demo Account logs in. A server that writes outside /app/data or /tmp fails
# here rather than in a self-hoster's container.
#
# The stack runs under its own project name, on its own volume, published on
# 127.0.0.1:<host-port> (18080 unless given), so it never touches a running
# Message Crate or its data. It is removed, volume and all, on exit. Runs in
# CI's "Docker image builds" job.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 <image> [host-port]" >&2
  exit 2
fi
IMAGE="$1"
PORT="${2:-18080}"
PROJECT="message-crate-compose-check"
BASE="http://127.0.0.1:${PORT}"

# The settings that limit what a process inside the container can do.
hardening() {
  docker compose -f "$1" config --format json |
    jq -S '.services.server | {cap_drop, security_opt, read_only, tmpfs}'
}

failures=0
release="$(hardening docker/compose.release.yml)"
published="$(hardening docker/compose.yml)"
if [[ "${release}" != "${published}" ]]; then
  echo "docker/compose.yml and docker/compose.release.yml drop different privileges:" >&2
  diff <(echo "${published}") <(echo "${release}") >&2 || true
  failures=$((failures + 1))
fi

override="$(mktemp)"
compose() {
  docker compose -p "${PROJECT}" -f docker/compose.release.yml -f "${override}" "$@"
}
cleanup() {
  compose down --volumes --remove-orphans >/dev/null 2>&1 || true
  rm -f "${override}"
}
trap cleanup EXIT

cat >"${override}" <<EOF
services:
  server:
    image: ${IMAGE}
    ports: !override
      - "127.0.0.1:${PORT}:8080"
EOF

# --wait returns once the image's HEALTHCHECK passes, and fails when the
# container exits or reads unhealthy.
if ! compose up --detach --no-build --wait --wait-timeout 300; then
  echo "The release Compose stack did not reach healthy. Its log:" >&2
  compose logs >&2 || true
  exit 1
fi

container="$(compose ps --quiet server)"
read -r readonly capdrop secopt < <(docker inspect --format \
  '{{.HostConfig.ReadonlyRootfs}} {{join .HostConfig.CapDrop ","}} {{join .HostConfig.SecurityOpt ","}}' \
  "${container}")
if [[ "${readonly}" != "true" ]]; then
  echo "The container's root filesystem is writable; read_only: true is missing" >&2
  failures=$((failures + 1))
fi
if [[ ",${capdrop}," != *",ALL,"* ]]; then
  echo "The container keeps Linux capabilities (cap_drop: ${capdrop:-none}); cap_drop: [ALL] is missing" >&2
  failures=$((failures + 1))
fi
if [[ ",${secopt}," != *",no-new-privileges:true,"* && ",${secopt}," != *",no-new-privileges,"* ]]; then
  echo "The container may gain privileges (security_opt: ${secopt:-none}); no-new-privileges:true is missing" >&2
  failures=$((failures + 1))
fi

# The Demo Account has no password.
token="$(curl -fsS -H 'Content-Type: application/json' \
  -d '{"username":"demo","password":""}' "${BASE}/v1/session" | jq -r .token)"
username="$(curl -fsS -H "Authorization: Bearer ${token}" "${BASE}/v1/session" | jq -r .username)"
if [[ "${username}" != "demo" ]]; then
  echo "Logging in to the Demo Account gave a Session for ${username:-nobody}" >&2
  failures=$((failures + 1))
fi

if [[ ${failures} -gt 0 ]]; then
  echo "Docker Compose check failed (${failures} failure(s))." >&2
  exit 1
fi
echo "The release Compose stack runs with its privileges dropped and serves the Demo Account."
