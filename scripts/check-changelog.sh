#!/usr/bin/env bash
# Check CHANGELOG.md against the formatting rules in AGENTS.md, "Releases
# and versions":
#
#   ./scripts/check-changelog.sh                # checks CHANGELOG.md
#   ./scripts/check-changelog.sh <file>         # checks another file
#
# - width: no line is longer than 80 characters.
# - tight lists: no blank line between two items of the same list, bulleted
#   or numbered, even one before an item's indented second paragraph,
#   because one blank line makes the whole list render loose.
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
  # The braces carry the redirect, so a locale the machine lacks prints no
  # setlocale warning.
  { LC_ALL="${locale}"; } 2>/dev/null
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

# A list item starts with -, * or + (a bullet), or with a number and . or )
# (an ordered list). Only a bullet carries the date.
item_re='^( *)([-*+]|[0-9]+[.)]) '
bullet_re='^[-*+] '
date_re='^[-*+] [0-9]{4}-[0-9]{2}-[0-9]{2}'

n=0
in_development=false
# Whether a blank line came since the last list item. A blank line anywhere
# inside a list, even before an indented paragraph of an item, makes the
# whole list render loose, so it counts until the next item.
blank_in_list=false
# The open lists, each as its indent and marker (" 0- 2. "), so an item
# after a blank line can be told apart: one with an indent and marker
# already open is a second item of the same list. A bullet character or an
# ordered list's . or ) that changes starts a new list.
list_indents=" "

while IFS= read -r line || [[ -n "${line}" ]]; do
  n=$((n + 1))

  if (( ${#line} > max_width )); then
    fail "${n}" "width: ${#line} characters, more than ${max_width}"
  fi

  if [[ -z "${line//[[:space:]]/}" ]]; then
    blank_in_list=true
    continue
  fi

  if [[ "${line}" =~ ^# ]]; then
    list_indents=" "
    if [[ "${line}" =~ ^##\  ]]; then
      # Any dash, so "- in development" cannot turn the date rule off.
      if [[ "${line}" == *"in development"* ]]; then
        in_development=true
      else
        in_development=false
      fi
    fi
  elif [[ "${line}" =~ ${item_re} ]]; then
    indent=${#BASH_REMATCH[1]}
    marker="${BASH_REMATCH[2]: -1}"
    if ${blank_in_list} && [[ "${list_indents}" == *" ${indent}${marker} "* ]]; then
      fail "${n}" "tight lists: blank line inside the list before this item"
    fi
    # An item ends every list at its own indent or deeper.
    kept=" "
    for open in ${list_indents}; do
      if (( ${open%?} < indent )); then
        kept+="${open} "
      fi
    done
    list_indents="${kept}${indent}${marker} "
    if ${in_development} && [[ "${line}" =~ ${bullet_re} ]] \
      && ! [[ "${line}" =~ ${date_re} ]]; then
      fail "${n}" "dated entries: a bullet under the in-development heading starts with YYYY-MM-DD"
    fi
  elif ! [[ "${line}" =~ ^[[:space:]] ]]; then
    # A paragraph at the left margin ends the list.
    list_indents=" "
  fi
  # Only an indented line inside an item keeps a blank line counting.
  if ! [[ "${line}" =~ ^[[:space:]] ]] || [[ "${line}" =~ ${item_re} ]]; then
    blank_in_list=false
  fi
done < "${file}"


if (( failures > 0 )); then
  echo "check-changelog: ${failures} problem(s) in ${file##*/}; the rules are in AGENTS.md, \"Releases and versions\"" >&2
  exit 1
fi
echo "check-changelog: ${file##*/} follows the formatting rules"
