#!/usr/bin/env bash
# Load the website a running Message Crate serves in a headless browser and
# use it: log in to the Demo Account, run a search, open a conversation.
#
#   ./scripts/check-served-website.sh http://127.0.0.1:18080
#
# The web app's tests mock the server and the server's tests call it over
# HTTP, so only this runs the built website against a real server. It fails
# when a page does not reach the screen it should, when the browser logs a
# console error or an uncaught exception, or when the server answers 5xx
# (#2217). The steps are in scripts/website-check/check.mjs.
#
# The server must hold the Demo Account, as every new Message Crate does.
# The browser runs in the Playwright image of the version
# scripts/website-check/package-lock.json pins, on the host's network, so
# nothing is installed on the machine and 127.0.0.1 is the server's
# address. scripts/check-docker-compose.sh --website runs it against the
# release image, and nightly.yml runs that.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CHECK_DIR="${SCRIPT_DIR}/website-check"

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <base-url>" >&2
  exit 2
fi
BASE="$1"

# The image must carry the browsers of the driver npm installs, so its tag
# is read from the lockfile rather than written down twice.
version="$(jq -r '.packages["node_modules/playwright"].version' "${CHECK_DIR}/package-lock.json")"
if [[ -z "${version}" || "${version}" == "null" ]]; then
  echo "No playwright version in ${CHECK_DIR}/package-lock.json" >&2
  exit 1
fi
image="mcr.microsoft.com/playwright:v${version}-noble"

# The check's tree is mounted read-only and copied, so npm ci writes no
# node_modules into the checkout, and none from the host is used.
docker run --rm --init --ipc=host --network host \
  -v "${CHECK_DIR}:/check:ro" \
  "${image}" \
  bash -c 'set -euo pipefail
    cp -r /check /tmp/website-check
    cd /tmp/website-check
    npm ci --no-audit --no-fund --silent
    node check.mjs "$1"' \
  check "${BASE}"
