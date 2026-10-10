#!/usr/bin/env bash
# The web app's server API types are generated from docs/src/assets/openapi.json.
# That JSON is already pinned to the running server by a Rust test
# (crates/server/server/src/openapi.rs). This is the other half: it fails when
# the checked-in TypeScript no longer matches the JSON, so a route or field
# renamed on the server cannot reach the web app as a silent runtime error.
#
#   ./scripts/check-generated-api-types.sh
#
# Regenerate with: (cd web && npm run gen:api)
#
# The generator is not a web/ dependency: it declares a peer dependency on
# TypeScript 5 and this project is on TypeScript 7, so installing it into web/
# fails to resolve. It has a tree of its own instead, scripts/openapi-typescript/,
# whose package-lock.json pins every package it runs, so a new release of one of
# its dependencies cannot run here unreviewed, and Dependabot and `npm audit` see
# that tree. Its `generate` script installs the tree with npm ci and writes the
# types to the path it is given; this check and web/package.json's gen:api both
# run it. Only its text output ever reaches the repository.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

GENERATED="web/src/lib/serverApi.types.ts"

if [[ ! -f "${GENERATED}" ]]; then
  echo "missing ${GENERATED}; run: (cd web && npm run gen:api)" >&2
  exit 1
fi

tmp="$(mktemp -t serverApi.types.XXXXXX.ts)"
trap 'rm -f "${tmp}"' EXIT

npm run --prefix scripts/openapi-typescript --silent generate -- "${tmp}" >/dev/null

if ! diff -u "${GENERATED}" "${tmp}"; then
  echo >&2
  echo "${GENERATED} is out of date with the OpenAPI document" >&2
  echo "that the generate script in scripts/openapi-typescript/ reads." >&2
  echo "run: (cd web && npm run gen:api)" >&2
  exit 1
fi

echo "${GENERATED} matches the OpenAPI document"
