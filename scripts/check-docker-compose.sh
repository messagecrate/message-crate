#!/usr/bin/env bash
# Start docker/compose.release.yml on a release image already built, and check
# that it runs with its privileges dropped and serves the Demo Account.
#
#   docker build -f docker/Dockerfile -t message-crate-server:check .
#   ./scripts/check-docker-compose.sh [--website] message-crate-server:check [host-port]
#
# Both Compose files drop every Linux capability, set no-new-privileges and
# make the root filesystem read-only (#2178). This script checks that both
# carry the same settings, that the container Compose starts has them, and
# that the server still works under them: the health check passes, the Demo
# Account logs in, and an account the owner creates imports a conversation
# with a video, waits for the media pass to make its Thumbnail and Preview,
# and exports it again (#2658). Last, it stops the stack and checks the
# server exited on its own within the stop_grace_period both files set,
# rather than being killed by Docker (#2122). A server that writes outside
# /app/data or /tmp at start, during an import, in the media pass or during an export
# fails here rather than in a self-hoster's container. The conversation is
# tests/fixtures/docker-compose-check/conversation.jsonl, and its video is
# the server's own test fixture crates/server/server/tests/fixtures/media/hevc.mov,
# an HEVC video, which needs both a Thumbnail and a Preview.
#
# --website also runs scripts/check-served-website.sh against the stack: the
# website it serves loads in a headless browser, logs in to the Demo
# Account, searches and opens a conversation (#2217). nightly.yml passes it
# and CI's job does not, because every run would pull the Playwright image,
# mcr.microsoft.com/playwright, which is about 2.5 GB unpacked.
#
# The stack runs under its own project name,
# message-crate-compose-check-<host-port>, on its own volume, published on
# 127.0.0.1:<host-port> (18080 unless given), so it never touches a running
# Message Crate or its data, and two checks on different ports never share a
# stack. It is removed, volume and all, on exit. Runs in CI's "Docker image
# builds" job and in nightly.yml.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

usage() {
  echo "usage: $0 [--website] <image> [host-port]" >&2
  exit 2
}

# --website may come before, between or after the image and the port. Any
# other argument that starts with - is refused, so a mistyped option is never
# taken for the image or the port.
WEBSITE=false
served="the Demo Account"
positional=()
for arg in "$@"; do
  case "${arg}" in
    --website)
      WEBSITE=true
      served="the Demo Account and its website"
      ;;
    -*)
      echo "$0: unknown option ${arg}" >&2
      usage
      ;;
    *) positional+=("${arg}") ;;
  esac
done
if [[ ${#positional[@]} -lt 1 || ${#positional[@]} -gt 2 ]]; then
  usage
fi
IMAGE="${positional[0]}"
PORT="${positional[1]:-18080}"
# A mistyped option without its dashes, such as "website", would otherwise
# reach Compose as the port, and port 0 or one above 65535 would fail later
# with a message that blames the image or the server instead.
if [[ ! "${PORT}" =~ ^[0-9]{1,5}$ ]] || ((10#${PORT} < 1 || 10#${PORT} > 65535)); then
  echo "$0: the host port must be a number from 1 to 65535, not ${PORT}" >&2
  usage
fi
# Drop a leading zero, so Compose publishes the port the login uses.
PORT=$((10#${PORT}))
PROJECT="message-crate-compose-check-${PORT}"
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

# The time Docker gives the server to stop before it kills it, which must
# be the same in both files (#2122).
grace() {
  docker compose -f "$1" config --format json | jq -r '.services.server.stop_grace_period // "unset"'
}
release_grace="$(grace docker/compose.release.yml)"
published_grace="$(grace docker/compose.yml)"
if [[ "${release_grace}" == "unset" || "${release_grace}" != "${published_grace}" ]]; then
  echo "docker/compose.yml and docker/compose.release.yml must set the same stop_grace_period, not ${published_grace} and ${release_grace}" >&2
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

# Before the import below, which claims the Message Crate.
if [[ "${WEBSITE}" == "true" ]] && ! ./scripts/check-served-website.sh "${BASE}"; then
  failures=$((failures + 1))
fi

# What a self-hoster's container does after it starts, under the same
# read-only root filesystem: an import with an attachment, the media pass,
# and an export. The Demo Account may not import, so the owner claims the
# Message Crate and creates an account for it.
FIXTURE_JSONL="tests/fixtures/docker-compose-check/conversation.jsonl"
FIXTURE_VIDEO="crates/server/server/tests/fixtures/media/hevc.mov"
# The fingerprint the conversation names. The server refuses the upload
# when the video's bytes do not match it.
VIDEO_SHA="$(jq -r 'select(.attachments) | .attachments[].digest_sha256' "${FIXTURE_JSONL}")"
OWNER_PASSWORD="Compose-Check-Owner-7q2Lx9Vb"
READER_PASSWORD="Compose-Check-Reader-3kN8wZr4"

# One call to the server: METHOD PATH [curl arguments...]. Prints the body,
# and fails with the status and body when the answer is not a 2xx. Every
# call gives up after 60 seconds, so a server that never answers fails the
# check rather than holding it until the CI job times out.
api() {
  local method="$1" path="$2"
  shift 2
  local body status
  body="$(mktemp)"
  if ! status="$(curl -sS --max-time 60 -o "${body}" -w '%{http_code}' -X "${method}" "$@" "${BASE}${path}")"; then
    echo "${method} ${path} gave no answer: the connection failed or took over 60 seconds" >&2
    status="none"
  elif [[ "${status}" == 2* ]]; then
    cat "${body}"
  else
    echo "${method} ${path} answered ${status}: $(cat "${body}")" >&2
  fi
  rm -f "${body}"
  [[ "${status}" == 2* ]]
}

# The import, media pass and export. Each step stops at its first failure,
# so a later step never reports a failure the earlier one caused.
import_media_export() {
  local owner reader import_id export_id attachment state
  owner="$(api POST /v1/server/claim -H 'Content-Type: application/json' \
    -d "{\"username\":\"keeper\",\"password\":\"${OWNER_PASSWORD}\"}" | jq -r .token)" || return 1
  api POST /v1/accounts -H "Authorization: Bearer ${owner}" -H 'Content-Type: application/json' \
    -d "{\"username\":\"reader\",\"password\":\"${READER_PASSWORD}\"}" >/dev/null || return 1
  reader="$(api POST /v1/session -H 'Content-Type: application/json' \
    -d "{\"username\":\"reader\",\"password\":\"${READER_PASSWORD}\"}" | jq -r .token)" || return 1
  local auth=(-H "Authorization: Bearer ${reader}")

  # The attachment first, then the batch that names it, as the desktop app
  # sends them.
  api PUT "/v1/assets/${VIDEO_SHA}" "${auth[@]}" -H 'Content-Type: video/quicktime' \
    --data-binary "@${FIXTURE_VIDEO}" >/dev/null || return 1
  import_id="$(api POST /v1/imports "${auth[@]}" -H 'Content-Type: application/json' \
    -d '{"source":"imessage","tool":"check-docker-compose"}' | jq -r .id)" || return 1
  api POST "/v1/imports/${import_id}/batches" "${auth[@]}" -H 'Content-Type: application/jsonl' \
    --data-binary "@${FIXTURE_JSONL}" >/dev/null || return 1
  api POST "/v1/imports/${import_id}/complete" "${auth[@]}" -H 'Content-Type: application/json' \
    -d '{"status":"completed"}' >/dev/null || return 1

  # The media pass runs in the background after the Import Run ends. Ask
  # once a second, for at most 5 minutes, until the video has both its
  # Thumbnail and its Preview, or the pass records why it made neither.
  state="waiting"
  for _ in $(seq 300); do
    attachment="$(api GET '/v1/messages?limit=10' "${auth[@]}" |
      jq -c --arg sha "${VIDEO_SHA}" '[.items[].attachments[] | select(.sha256 == $sha)][0]')" || return 1
    if [[ "$(jq -r '.thumbnail_not_made_reason // .preview_not_made_reason // empty' <<<"${attachment}")" != "" ]]; then
      state="failed"
      break
    fi
    if [[ "$(jq -r 'if .thumbnail_mime_type and .preview_mime_type then "made" else "" end' <<<"${attachment}")" == "made" ]]; then
      state="made"
      break
    fi
    sleep 1
  done
  if [[ "${state}" != "made" ]]; then
    echo "The media pass did not make the video's Thumbnail and Preview (${state}): ${attachment}" >&2
    return 1
  fi
  api GET "/v1/assets/${VIDEO_SHA}/thumbnail" "${auth[@]}" >/dev/null || return 1
  api GET "/v1/assets/${VIDEO_SHA}/preview" "${auth[@]}" >/dev/null || return 1

  # An Export Run of everything: its messages, the original video, and the
  # run recorded as completed.
  export_id="$(api POST /v1/exports "${auth[@]}" -H 'Content-Type: application/json' \
    -d '{"scope":{"kind":"everything"},"tool":"check-docker-compose"}' | jq -r .id)" || return 1
  local total
  total="$(api GET "/v1/exports/${export_id}/messages?limit=10" "${auth[@]}" | jq -r .total)" || return 1
  if [[ "${total}" != "2" ]]; then
    echo "The Export Run handed over ${total} messages, not the 2 the import brought" >&2
    return 1
  fi
  if ! cmp -s <(api GET "/v1/assets/${VIDEO_SHA}" "${auth[@]}") "${FIXTURE_VIDEO}"; then
    echo "The exported video is not the one imported" >&2
    return 1
  fi
  local status
  status="$(api POST "/v1/exports/${export_id}/complete" "${auth[@]}" \
    -H 'Content-Type: application/json' -d '{"status":"completed"}' | jq -r .status)" || return 1
  if [[ "${status}" != "completed" ]]; then
    echo "The Export Run ended ${status}, not completed" >&2
    return 1
  fi
}

if ! import_media_export; then
  echo "The import, media pass or export failed under the read-only limits. The server's log:" >&2
  compose logs >&2 || true
  failures=$((failures + 1))
fi

# docker compose stop sends SIGTERM and kills the server after the
# stop_grace_period. A server that exits on its own exits 0; one Docker had
# to kill exits 137.
compose stop server >/dev/null
exit_code="$(docker inspect --format '{{.State.ExitCode}}' "${container}")"
if [[ "${exit_code}" != "0" ]]; then
  echo "The server exited ${exit_code} when the stack was stopped, not 0. Its log:" >&2
  compose logs >&2 || true
  failures=$((failures + 1))
fi

if [[ ${failures} -gt 0 ]]; then
  echo "Docker Compose check failed (${failures} failure(s))." >&2
  exit 1
fi
echo "The release Compose stack runs with its privileges dropped, serves ${served}, imports, makes the Thumbnail and Preview and exports under them, and stops on its own."
