#!/usr/bin/env bash
# Check CHANGELOG.md against the formatting rules in AGENTS.md, "Releases
# and versions":
#
#   ./scripts/check-changelog.sh                # checks CHANGELOG.md
#   ./scripts/check-changelog.sh <file>         # checks another file
#
# - width: no line is longer than 80 characters.
# - tight lists: no blank line between two bullets of the same list, because
#   one blank line makes the whole list render loose.
# - dated entries: every top-level bullet under the "— in development"
#   heading starts with an ISO date (YYYY-MM-DD). Released sections are
#   exempt; their date is on the heading.
#
# Read-only. Prints one line per failure, naming the line number and the
# rule, and exits 1 when there is any. Plain bash, so it needs nothing
# installed. Runs in check-pr.sh and in CI's version job.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
file="${1:-${REPO_ROOT}/CHANGELOG.md}"

max_width=80

# Bash counts characters, not bytes, only under a UTF-8 locale. Pick one the
# machine has, and refuse to run rather than count "—" as three.
probe='—'
for locale in C.UTF-8 C.utf8 en_US.UTF-8 en_US.utf8; do
  LC_ALL="${locale}" 2>/dev/null
  if [[ ${#probe} -eq 1 ]]; then
    break
  fi
done
if [[ ${#probe} -ne 1 ]]; then
  echo "check-changelog: no UTF-8 locale found to count characters with" >&2
  exit 2
fi

failures=0
fail() {
  echo "${file##*/}:$1: $2"
  failures=$((failures + 1))
}

bullet_re='^( *)[-*+] '
date_re='^[-*+] [0-9]{4}-[0-9]{2}-[0-9]{2}'

n=0
in_development=false
blank_before=false
# The indents of the bullets in the list being read, as " 0 2 ", so a
# bullet after a blank line can be told apart: one at an indent already in
# the list is a second bullet of the same list.
list_indents=" "

while IFS= read -r line || [[ -n "${line}" ]]; do
  n=$((n + 1))

  if (( ${#line} > max_width )); then
    fail "${n}" "width: ${#line} characters, more than ${max_width}"
  fi

  if [[ -z "${line//[[:space:]]/}" ]]; then
    blank_before=true
    continue
  fi

  if [[ "${line}" =~ ^# ]]; then
    list_indents=" "
    if [[ "${line}" =~ ^##\  ]]; then
      if [[ "${line}" == *"— in development"* ]]; then
        in_development=true
      else
        in_development=false
      fi
    fi
  elif [[ "${line}" =~ ${bullet_re} ]]; then
    indent=${#BASH_REMATCH[1]}
    if ${blank_before} && [[ "${list_indents}" == *" ${indent} "* ]]; then
      fail "${n}" "tight lists: blank line before this bullet of the same list"
    fi
    # A bullet ends every deeper list under the one before it.
    kept=" "
    for i in ${list_indents}; do
      if (( i < indent )); then
        kept+="${i} "
      fi
    done
    list_indents="${kept}${indent} "
    if ${in_development} && (( indent == 0 )) && ! [[ "${line}" =~ ${date_re} ]]; then
      fail "${n}" "dated entries: a bullet under the in-development heading starts with YYYY-MM-DD"
    fi
  elif ! [[ "${line}" =~ ^[[:space:]] ]]; then
    # A paragraph at the left margin ends the list.
    list_indents=" "
  fi
  blank_before=false
done < "${file}"

if (( failures > 0 )); then
  echo "check-changelog: ${failures} problem(s) in ${file##*/}; the rules are in AGENTS.md, \"Releases and versions\"" >&2
  exit 1
fi
echo "check-changelog: ${file##*/} follows the formatting rules"
