#!/usr/bin/env bash
# Fast pre-PR check: formatting and lint only, nothing expensive.
#
#   ./scripts/check-pr.sh
#
# Stops on the first failure. Checks, never rewrites: CHANGELOG.md's
# formatting rules, rustfmt --check and Clippy at -D warnings on the
# workspace and src-tauri, Biome ci, the check that the timed-wait rule for
# web test files still catches every form, the check that web/biome.json's
# restricted-import lists agree, and the web type-check. Run
# ./scripts/format-all.sh to fix rustfmt and Biome failures; a CHANGELOG.md
# failure names its line and rule, and is fixed by hand.
# CI is the complete gate; ./scripts/check-all.sh runs the full set locally.
# Why this split: docs/adr/0007-ci-is-the-only-gate.md.
# Runs npm ci in web/ only when that tree has no node_modules yet.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

echo "==> CHANGELOG.md formatting"
./scripts/check-changelog.sh

echo "==> cargo fmt --check (workspace)"
cargo fmt --all -- --check

echo "==> cargo fmt --check (src-tauri)"
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check

echo "==> cargo clippy (workspace)"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> cargo clippy (src-tauri)"
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

if [[ ! -d web/node_modules ]]; then
  echo "==> npm ci (web)"
  (cd web && npm ci)
fi
echo "==> web lint + format check"
(cd web && npx biome ci .)
echo "==> web timed-wait rule still catches every form"
(cd web && npx biome lint --error-on-warnings biome-plugins/noTimedWaitsInTests.canary.ts)
echo "==> web noRestrictedImports lists agree"
node scripts/check-biome-restricted-imports.mjs
echo "==> web type-check"
(cd web && npm run typecheck)

echo "All pre-PR checks passed."
