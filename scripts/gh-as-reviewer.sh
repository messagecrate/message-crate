#!/usr/bin/env bash
# Runs gh as the message-crate-reviewer GitHub App instead of the logged-in
# account, so pr-review's posts count against the app's posting limit and not
# the user's.
#
#   ./scripts/gh-as-reviewer.sh api repos/messagecrate/message-crate/pulls/<N>/reviews ...
#   ./scripts/gh-as-reviewer.sh pr comment <N> --body "..."
#
# Each run signs a JWT with the app's private key
# (~/.ssh/message-crate-reviewer.pem), exchanges it for an installation token
# for messagecrate/message-crate (valid an hour, never written to disk), and
# passes it to gh as GH_TOKEN. Needs openssl, curl, jq. When GitHub refuses a
# request, it prints the HTTP status and GitHub's reply and exits 1.
#
# Why: docs/adr/0007-ci-is-the-only-gate.md.
set -euo pipefail

APP_ID=5267106
KEY="${HOME}/.ssh/message-crate-reviewer.pem"
REPO="messagecrate/message-crate"

if [[ ! -r "${KEY}" ]]; then
  echo "gh-as-reviewer: no private key at ${KEY}" >&2
  exit 1
fi

b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }

now=$(date +%s)
header=$(printf '{"alg":"RS256","typ":"JWT"}' | b64url)
# iat is backdated a minute for clock drift. GitHub allows exp at most 10
# minutes out.
payload=$(printf '{"iat":%d,"exp":%d,"iss":"%s"}' $((now - 60)) $((now + 540)) "${APP_ID}" | b64url)
signature=$(printf '%s.%s' "${header}" "${payload}" | openssl dgst -sha256 -sign "${KEY}" | b64url)
jwt="${header}.${payload}.${signature}"

# Calls GitHub as the app and prints the reply. On any status but 2xx, names
# the step, the status and the reply, and exits. The JWT goes in through a
# header file, not argv, so ps does not show it.
app_api() {
  local step="$1"
  shift
  local reply status
  if ! reply=$(curl -sS -w '\n%{http_code}' \
    -H @<(printf 'Authorization: Bearer %s\n' "${jwt}") \
    -H 'Accept: application/vnd.github+json' \
    "$@"); then
    echo "gh-as-reviewer: ${step}: could not reach GitHub" >&2
    exit 1
  fi
  status="${reply##*$'\n'}"
  reply="${reply%$'\n'*}"
  if [[ "${status}" != 2* ]]; then
    echo "gh-as-reviewer: ${step}: GitHub answered HTTP ${status}: ${reply}" >&2
    exit 1
  fi
  printf '%s' "${reply}"
}

installation=$(app_api "looking up the installation on ${REPO}" \
  "https://api.github.com/repos/${REPO}/installation")
created=$(app_api "creating an installation token" -X POST \
  "https://api.github.com/app/installations/$(jq -r .id <<<"${installation}")/access_tokens")

GH_TOKEN=$(jq -r .token <<<"${created}")
export GH_TOKEN
exec gh "$@"
