#!/usr/bin/env bash
# Audit the docs site's npm dependencies.
#
#   ./scripts/audit-docs.sh
#
# Fails when docs/ holds a high or critical advisory that is not on the
# ACCEPTED list below. The Audit workflow (.github/workflows/audit.yml) and
# ./scripts/check-all.sh both run it, so the list lives in one place.
#
# An advisory with a patched version is fixed, never accepted: by an update,
# `npm audit fix`, or an `overrides` entry in docs/package.json. An advisory
# with no patched version goes on the list, because no change to docs/ can
# fix it and the audit would otherwise fail every week until one ships. Each
# entry carries a comment naming the package, the affected range, and why it
# is accepted. When a fix ships, the entry comes off the list and the fix goes
# in instead. Why: #1457.
#
# Reads docs/node_modules, so run `npm ci` in docs/ first.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}/docs"

# GHSA IDs accepted with no patched version. Empty while every high or
# critical advisory in docs/ has a fix.
ACCEPTED=()

report="$(npm audit --json || true)"

if ! jq -e '.vulnerabilities | type == "object"' <<<"${report}" >/dev/null 2>&1; then
  echo "npm audit did not produce a report:" >&2
  echo "${report}" >&2
  exit 1
fi

accepted_json="$(jq -cn '$ARGS.positional' --args "${ACCEPTED[@]}")"

# Each advisory sits in the `via` list of the package it names. Entries that
# are plain strings point at another vulnerable package, not an advisory.
failing="$(jq -r --argjson accepted "${accepted_json}" '
  [.vulnerabilities[].via[]
    | objects
    | select(.severity == "high" or .severity == "critical")
    | {id: (.url | split("/") | last), name, severity, range, title}]
  | unique_by(.id)
  | map(select(.id as $id | $accepted | index($id) | not))
  | .[]
  | "\(.id) \(.severity) \(.name) \(.range): \(.title)"
' <<<"${report}")"

if [[ -n "${failing}" ]]; then
  echo "High or critical advisories in docs/ that are not accepted:" >&2
  echo "${failing}" >&2
  exit 1
fi

echo "No high or critical advisories in docs/ outside the accepted list (${#ACCEPTED[@]} accepted)."
