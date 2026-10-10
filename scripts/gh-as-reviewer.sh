#!/usr/bin/env bash
# Runs gh as the message-crate-reviewer GitHub App instead of the logged-in
# account, so pr-review's posts count against the app's posting limit and not
# the user's.
#
#   ./scripts/gh-as-reviewer api repos/messagecrate/message-crate/pulls/<N>/reviews ...
#   ./scripts/gh-as-reviewer pr comment <N> --body "..."
#
# Each run signs a JWT with the app's private key, exchanges it for an
# installation token for messagecrate/message-crate (valid an hour, never
# written to disk), and passes it to gh as GH_TOKEN. Needs openssl, curl, jq.
#
#   MC_REVIEWER_APP_ID   the app's ID (default 5267106)
#   MC_REVIEWER_APP_KEY  its private key (default ~/.ssh/message-crate-reviewer.pem)
#
# Why: AGENTS.md, "Posting pace".
set -euo pipefail

APP_ID="${MC_REVIEWER_APP_ID:-5267106}"
KEY="${MC_REVIEWER_APP_KEY:-$HOME/.ssh/message-crate-reviewer.pem}"
REPO="messagecrate/message-crate"

if [[ ! -r "${KEY}" ]]; then
  echo "gh-as-reviewer: no private key at ${KEY}; set MC_REVIEWER_APP_KEY" >&2
  exit 1
fi

b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }

now=$(date +%s)
header=$(printf '{"alg":"RS256","typ":"JWT"}' | b64url)
# iat is backdated a minute for clock drift; GitHub allows exp at most 10 minutes out.
payload=$(printf '{"iat":%d,"exp":%d,"iss":"%s"}' $((now - 60)) $((now + 540)) "${APP_ID}" | b64url)
signature=$(printf '%s.%s' "${header}" "${payload}" | openssl dgst -sha256 -sign "${KEY}" | b64url)
jwt="${header}.${payload}.${signature}"

# The JWT goes in through a header file, not argv, so ps does not show it.
app_api() {
  curl -sS --fail-with-body \
    -H @<(printf 'Authorization: Bearer %s\n' "${jwt}") \
    -H 'Accept: application/vnd.github+json' \
    "$@"
}

if ! installation=$(app_api "https://api.github.com/repos/${REPO}/installation"); then
  echo "gh-as-reviewer: app ${APP_ID} is not installed on ${REPO}: ${installation}" >&2
  exit 1
fi
installation_id=$(jq -r .id <<<"${installation}")

if ! created=$(app_api -X POST "https://api.github.com/app/installations/${installation_id}/access_tokens"); then
  echo "gh-as-reviewer: could not create an installation token: ${created}" >&2
  exit 1
fi

GH_TOKEN=$(jq -r .token <<<"${created}")
export GH_TOKEN
exec gh "$@"
