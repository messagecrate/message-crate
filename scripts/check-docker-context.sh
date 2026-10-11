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
  # The keyword in any case, after any indent, and past any --flag=value.
  image_rust="$(sed -n 's/^[[:space:]]*[Ff][Rr][Oo][Mm][[:space:]]\{1,\}\(--[^[:space:]]*[[:space:]]\{1,\}\)*rust:\([0-9][0-9.]*\).*$/\2/p' "${DOCKERFILE}" | head -1)"
  if [[ -z "${image_rust}" ]]; then
    echo "${DOCKERFILE}: could not read a rust:<version> base image" >&2
    failures=$((failures + 1))
  elif [[ "$(cut -d. -f1,2 <<<"${channel}")" != "$(cut -d. -f1,2 <<<"${image_rust}")" ]]; then
    echo "${DOCKERFILE}: base image is rust:${image_rust}, rust-toolchain.toml pins ${channel}; keep them on one minor" >&2
    failures=$((failures + 1))
  fi
fi

# The base images. A tag names whatever its publisher pushed last, so a
# FROM by tag alone can start two builds of one commit from different
# images. Each FROM names its digest after the tag, and Dependabot's docker
# entry moves the digests by pull request (#2179).
# Docker reads the keyword in any case and after an indent, a --flag=value
# such as --platform may come before the image, and a FROM that names an
# earlier stage (AS <name>) has no image of its own to pin.
stages=()
while IFS= read -r from; do
  read -ra words <<<"${from}"
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
  earlier=0
  for name in "${stages[@]}"; do
    [[ "${image,,}" == "${name}" ]] && earlier=1
  done
  if [[ "${earlier}" -eq 0 && ! "${image}" =~ ^[^@[:space:]]+:[^@/:[:space:]]+@sha256:[0-9a-f]{64}$ ]]; then
    echo "${DOCKERFILE}: pin the base image by tag and digest (FROM <image>:<tag>@sha256:<digest>): ${from}" >&2
    failures=$((failures + 1))
  fi
  [[ -n "${stage}" ]] && stages+=("${stage}")
done < <(grep -iE '^[[:space:]]*FROM[[:space:]]' "${DOCKERFILE}")

if [[ ${failures} -gt 0 ]]; then
  echo "Docker context check failed (${failures} failure(s))." >&2
  exit 1
fi
