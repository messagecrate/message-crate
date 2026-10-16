#!/usr/bin/env bash
# Scan the commits a branch adds for secrets with gitleaks.
#
#   ./scripts/check-secrets.sh [<base>]
#
# Reads every commit in <base>..HEAD (default origin/main), with the default
# gitleaks rules plus the Message Crate token rule and the test-vector
# allowlists in .gitleaks.toml. Read-only. Prints each finding with the secret
# redacted, because CI logs on this repository are public, and exits 1 when
# there is one. Runs in CI ("Secret scan (gitleaks)") and in check-all.sh.
# Why: docs/adr/0007-ci-is-the-only-gate.md.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

base="${1:-origin/main}"

if ! command -v gitleaks >/dev/null 2>&1; then
  echo "gitleaks is not on PATH (https://github.com/gitleaks/gitleaks/releases)" >&2
  exit 2
fi

gitleaks git --config .gitleaks.toml --redact --no-banner --verbose \
  --log-opts="${base}..HEAD" .
