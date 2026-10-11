#!/usr/bin/env bash
# Assert docker/Dockerfile copies every [patch.crates-io] path crate and
# rust-toolchain.toml, that its rust base image is on the pinned minor, and
# that every base image is pinned by tag and digest.
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
# the compiler check below.
stages=()
image_rust=""
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
  if [[ "${names_no_image}" -eq 0 && ! "${image}" =~ ^[^@[:space:]]+:[^@/:[:space:]]+@sha256:[0-9a-f]{64}$ ]]; then
    echo "${DOCKERFILE}: pin the base image by tag and digest (FROM <image>:<tag>@sha256:<digest>): ${line}" >&2
    failures=$((failures + 1))
  fi
  if [[ -z "${image_rust}" && "${image}" =~ ^rust:([0-9][0-9.]*) ]]; then
    image_rust="${BASH_REMATCH[1]}"
  fi
  [[ -n "${stage}" ]] && stages+=("${stage}")
done <"${DOCKERFILE}"

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
