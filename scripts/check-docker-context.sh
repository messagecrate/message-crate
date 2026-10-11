#!/usr/bin/env bash
# Assert docker/Dockerfile copies every [patch.crates-io] path crate and
# rust-toolchain.toml, that its rust base image is on the pinned minor, that
# its frontend-builder stage is on the Node.js major in .nvmrc, and that every
# base image, and the BuildKit frontend its syntax line names, is pinned by
# tag and digest.
#
#   ./scripts/check-docker-context.sh
#
# Read-only. Cargo resolves workspace patches from the rust-builder
# WORKDIR. If a patched path is missing from the image, `cargo build`
# fails with "failed to read …/Cargo.toml". Without rust-toolchain.toml
# the image compiles on whatever Rust the base tag carries instead of the
# version CI tested (#427).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

DOCKERFILE="docker/Dockerfile"
failures=0
saw_patch=0
# An image pinned by tag and digest: <image>:<tag>@sha256:<digest>.
pinned_image_re='^[^@[:space:]]+:[^@/:[:space:]]+@sha256:[0-9a-f]{64}$'

if [[ ! -f "${DOCKERFILE}" ]]; then
  echo "${DOCKERFILE}: missing" >&2
  exit 1
fi

in_patch=0
while IFS= read -r line || [[ -n "${line}" ]]; do
  if [[ "${line}" == "[patch.crates-io]" ]]; then
    in_patch=1
    continue
  fi
  if [[ "${in_patch}" -eq 1 && "${line}" =~ ^\[ ]]; then
    break
  fi
  if [[ "${in_patch}" -eq 1 && "${line}" =~ path[[:space:]]*=[[:space:]]*\"([^\"]+)\" ]]; then
    saw_patch=1
    path="${BASH_REMATCH[1]}"
    top="${path%%/*}"
    if [[ ! -f "${path}/Cargo.toml" ]]; then
      echo "Cargo.toml patches ${path}, but ${path}/Cargo.toml is missing" >&2
      failures=$((failures + 1))
      continue
    fi
    if ! grep -Eq "^[[:space:]]*COPY[[:space:]]+(${top}|${path})([[:space:]]|/)" "${DOCKERFILE}"; then
      echo "${DOCKERFILE}: COPY ${top} (or ${path}) so cargo can load the [patch.crates-io] crate at ${path}" >&2
      failures=$((failures + 1))
    fi
  fi
done <Cargo.toml

if [[ "${in_patch}" -eq 0 ]]; then
  echo "Cargo.toml has no [patch.crates-io] section; nothing to check." >&2
fi

if [[ "${in_patch}" -eq 1 && "${saw_patch}" -eq 0 ]]; then
  echo "Cargo.toml [patch.crates-io] has no path = crates; nothing to check." >&2
fi

# The frontend. A "# syntax=" parser directive makes BuildKit pull that
# image and parse the Dockerfile with it, so it is pinned like a base image
# (#2670). The comment under the Dockerfile's syntax line says how its
# digest is moved.
#
# This reads the directives as BuildKit's parser does
# (frontend/dockerfile/parser/directives.go): it drops a byte order mark
# and skips a first line that starts with "#!", then reads "# key=value"
# lines until the first line that is not one, or whose key is not syntax,
# escape or check. The key is in any case, and spaces may sit around the
# "#", the key and the "=". When that finds no syntax, BuildKit reads the
# directives again with "//" in place of "#", so this does too. BuildKit
# pulls only the first word of the syntax value; the rest are frontend
# arguments.
#
# Sets syntax_value and syntax_line when it finds a syntax directive.
read_syntax_directive() {
  local prefix="$1" on_first_line=1 line
  while IFS= read -r line || [[ -n "${line}" ]]; do
    line="${line%$'\r'}"
    if [[ "${on_first_line}" -eq 1 ]]; then
      on_first_line=0
      line="${line#$'\xEF\xBB\xBF'}"
      [[ "${line}" == '#!'* ]] && continue
    fi
    [[ "${line}" =~ ^${prefix}[[:space:]]*([A-Za-z][A-Za-z0-9]*)[[:space:]]*=[[:space:]]*(.*[^[:space:]])[[:space:]]*$ ]] || return 0
    case "${BASH_REMATCH[1],,}" in
      syntax)
        syntax_value="${BASH_REMATCH[2]%%[[:space:]]*}"
        syntax_line="${line}"
        return 0
        ;;
      escape | check) ;;
      *) return 0 ;;
    esac
  done <"${DOCKERFILE}"
}
syntax_value=""
syntax_line=""
read_syntax_directive "#"
[[ -n "${syntax_line}" ]] || read_syntax_directive "//"
if [[ -n "${syntax_line}" && ! "${syntax_value}" =~ ${pinned_image_re} ]]; then
  echo "${DOCKERFILE}: pin the BuildKit frontend by tag and digest (# syntax=<image>:<tag>@sha256:<digest>): ${syntax_line}" >&2
  failures=$((failures + 1))
fi

# The base images. A tag names whatever its publisher pushed last, so a
# FROM by tag alone can start two builds of one commit from different
# images. Each FROM names its digest after the tag, and Dependabot's docker
# entry moves the digests by pull request (#2179).
#
# Docker reads the keyword in any case and after an indent. A --flag=value
# such as --platform may come before the image. A FROM that names an earlier
# stage (AS <name>), or scratch, has no image of its own to pin. A line
# inside a heredoc, or after a line that ends in a backslash, is not an
# instruction, so it is skipped. The first rust:<version> image is kept for
# the compiler check below, and the frontend-builder stage's image for the
# Node.js check.
stages=()
image_rust=""
image_frontend=""
heredoc=""
continued=0
while IFS= read -r line || [[ -n "${line}" ]]; do
  line="${line%$'\r'}"
  if [[ -n "${heredoc}" ]]; then
    [[ "${line#"${line%%[!$'\t']*}"}" == "${heredoc}" ]] && heredoc=""
    continue
  fi
  # Docker drops comment and blank lines, even inside a continued line.
  [[ "${line}" =~ ^[[:space:]]*(#|$) ]] && continue
  instruction=$((1 - continued))
  continued=0
  [[ "${line}" =~ \\[[:space:]]*$ ]] && continued=1
  if [[ "${line}" =~ (^|[^<])\<\<-?[\"\']?([A-Za-z_][A-Za-z0-9_]*) ]]; then
    heredoc="${BASH_REMATCH[2]}"
  fi
  [[ "${instruction}" -eq 1 ]] || continue
  read -ra words <<<"${line}"
  [[ ${#words[@]} -gt 0 && "${words[0],,}" == "from" ]] || continue
  image=""
  stage=""
  for ((k = 1; k < ${#words[@]}; k++)); do
    word="${words[k]}"
    if [[ -z "${image}" ]]; then
      [[ "${word}" == --* ]] && continue
      image="${word}"
    elif [[ "${word,,}" == "as" && $((k + 1)) -lt ${#words[@]} ]]; then
      stage="${words[k + 1],,}"
      break
    fi
  done
  names_no_image=0
  [[ "${image,,}" == "scratch" ]] && names_no_image=1
  for name in "${stages[@]}"; do
    [[ "${image,,}" == "${name}" ]] && names_no_image=1
  done
  if [[ "${names_no_image}" -eq 0 && ! "${image}" =~ ${pinned_image_re} ]]; then
    echo "${DOCKERFILE}: pin the base image by tag and digest (FROM <image>:<tag>@sha256:<digest>): ${line}" >&2
    failures=$((failures + 1))
  fi
  if [[ -z "${image_rust}" && "${image}" =~ ^rust:([0-9][0-9.]*) ]]; then
    image_rust="${BASH_REMATCH[1]}"
  fi
  [[ "${stage}" == "frontend-builder" ]] && image_frontend="${image}"
  [[ -n "${stage}" ]] && stages+=("${stage}")
done <"${DOCKERFILE}"

# Node.js. .nvmrc holds the one major that every workflow that installs
# Node.js reads (actions/setup-node's node-version-file) and that builds web/
# in the image, so CI tests the website on the Node.js that builds it there
# (#2821). The file holds the major alone, and only the major of the stage's
# tag is compared with it, so a tag such as node:24.11.1-bookworm-slim
# passes. The runtime stage runs the server, not the build, and is not
# checked.
nvmrc_major="$(tr -d '[:space:]' 2>/dev/null <.nvmrc || true)"
if [[ ! "${nvmrc_major}" =~ ^[1-9][0-9]*$ ]]; then
  echo ".nvmrc: hold a Node.js major alone, such as 24; found '${nvmrc_major}'" >&2
  failures=$((failures + 1))
elif [[ -z "${image_frontend}" ]]; then
  echo "${DOCKERFILE}: could not read a FROM ... AS frontend-builder stage" >&2
  failures=$((failures + 1))
elif [[ ! "${image_frontend}" =~ ^node:([0-9]+)([.@-]|$) ]]; then
  echo "${DOCKERFILE}: the frontend-builder stage is not a node:<major>-<variant> image: ${image_frontend}" >&2
  failures=$((failures + 1))
elif [[ "${BASH_REMATCH[1]}" != "${nvmrc_major}" ]]; then
  echo "${DOCKERFILE}: the frontend-builder stage is on Node.js ${BASH_REMATCH[1]}, .nvmrc names ${nvmrc_major}; keep them on one major" >&2
  failures=$((failures + 1))
fi

# The compiler. rust-toolchain.toml pins it; the rust-builder stage has to
# copy the file so rustup installs that version inside the image. The base
# tag stays on the same minor so the install is a no-op once Docker Hub
# carries the patch release, and so a toolchain bump that forgets the
# Dockerfile fails here rather than silently downloading a second toolchain.
channel="$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)"
if [[ -z "${channel}" ]]; then
  echo "rust-toolchain.toml: could not read a channel" >&2
  failures=$((failures + 1))
else
  if ! grep -Eq '^[[:space:]]*COPY[[:space:]]+([^#]*[[:space:]])?rust-toolchain\.toml([[:space:]]|$)' "${DOCKERFILE}"; then
    echo "${DOCKERFILE}: COPY rust-toolchain.toml into the rust-builder stage so the image compiles with the pinned Rust ${channel}" >&2
    failures=$((failures + 1))
  fi
  if [[ -z "${image_rust}" ]]; then
    echo "${DOCKERFILE}: could not read a rust:<version> base image" >&2
    failures=$((failures + 1))
  elif [[ "$(cut -d. -f1,2 <<<"${channel}")" != "$(cut -d. -f1,2 <<<"${image_rust}")" ]]; then
    echo "${DOCKERFILE}: base image is rust:${image_rust}, rust-toolchain.toml pins ${channel}; keep them on one minor" >&2
    failures=$((failures + 1))
  fi
fi

if [[ ${failures} -gt 0 ]]; then
  echo "Docker context check failed (${failures} failure(s))." >&2
  exit 1
fi
