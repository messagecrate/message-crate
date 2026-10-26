# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
# The line above makes BuildKit parse this file with the current Dockerfile
# frontend rather than the one bundled with the engine, because the frontend
# bundled with Docker Engine 24 and older refuses HEALTHCHECK --start-interval.
# BuildKit pulls that frontend as an image and runs it at build time, so it is
# pinned by tag and digest like the base images below, and
# scripts/check-docker-context.sh fails it without a digest. Dependabot reads
# only FROM lines, so this digest is moved by hand: take the new one from
#   docker buildx imagetools inspect docker/dockerfile:1
#
# Multi-stage release image built from the current checkout.
# Usage (from the repository root):
#   docker build -f docker/Dockerfile -t message-crate-server .
# Or:
#   docker compose -f docker/compose.release.yml up --build
#
# BUILD_METADATA is the part of the Build after the "+", normally the commit:
#   docker build --build-arg BUILD_METADATA="$(git rev-parse --short=8 HEAD)" ...
# .git is not in the build context, so neither stage can ask git for it. Left
# out, the server and the website report <version>+unknown. Passed empty, as
# the release job does, they report the Product Version alone.
#
# Each FROM names its base image by tag and digest. The tag is for the
# reader, and scripts/check-docker-context.sh reads the Rust minor and the
# frontend stage's Node.js major from it. That major must be the one in
# .nvmrc; the script says why.
# The digest fixes the image, so two builds of one commit start from the
# same bases. Dependabot's docker entry in .github/dependabot.yml moves the
# digests by pull request, and scripts/check-docker-context.sh fails a FROM
# without one.

# -----------------------------------------------------------------------------
# Stage 1: Vite SPA frontend
# -----------------------------------------------------------------------------
FROM node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5 AS frontend-builder

WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
ARG BUILD_METADATA=unknown
ENV MESSAGE_CRATE_BUILD_METADATA=$BUILD_METADATA
RUN npm run build

# -----------------------------------------------------------------------------
# Stage 2: Rust binary
# -----------------------------------------------------------------------------
# rust-toolchain.toml is the one place the compiler version is chosen. The
# image copies it and lets rustup install that exact toolchain before the
# build, so the published server compiles on the Rust that CI tested (#427).
# The base tag only picks the Debian release and pre-installs the same
# minor: once Docker Hub carries the patch release the install is a no-op,
# and scripts/check-docker-context.sh fails when the two minors disagree.
FROM rust:1.98-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS rust-builder

WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
RUN rustup toolchain install
COPY crates ./crates
# Required at compile time by crates/server/server/src/db/schema.rs include_str!(…).
COPY schema ./schema
# [patch.crates-io] sqlx-sqlite lives here (see Cargo.toml). Without this
# copy, cargo build fails: failed to read /src/vendor/sqlx-sqlite/Cargo.toml.
COPY vendor ./vendor

ARG BUILD_METADATA=unknown
ENV MESSAGE_CRATE_BUILD_METADATA=$BUILD_METADATA
# The server compiles in crates/server/demo-seed/data/ (corpus + names) and
# generates Demo Data from it on a first start. Do not ignore that path.
RUN cargo build --release -p message-crate-server

# -----------------------------------------------------------------------------
# Stage 3: slim runtime
# -----------------------------------------------------------------------------
# Debian 12 (bookworm) alone, the release the build stages use. The running
# container needs no Node.js: the website is built in Stage 1 and the health
# check is a server command (#2065).
FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8 AS runtime

# The container runs as user and group 1000, named app. Both Compose files
# run it as 1000:1000 by default, and the files in existing data volumes
# belong to that id.
RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates \
    ffmpeg \
    tini \
  && rm -rf /var/lib/apt/lists/* \
  && groupadd --gid 1000 app \
  && useradd --uid 1000 --gid 1000 --create-home --shell /usr/sbin/nologin app

WORKDIR /app

# Rust CLI / serve binary
COPY --from=rust-builder /src/target/release/message-crate-server /usr/local/bin/message-crate-server

# Container config, at the path every server command reads by default. The
# image carries it rather than the entrypoint copying it at start, because
# the Compose files make the root filesystem read-only.
COPY config/config.docker.toml ./config/config.toml

# Vite SPA (built in Stage 1)
COPY --from=frontend-builder /web/dist ./static

COPY docker/entrypoint-release.sh /usr/local/bin/docker-entrypoint-release.sh
RUN chmod +x /usr/local/bin/docker-entrypoint-release.sh \
  && mkdir -p /app/data \
  && chown -R app:app /app

USER app

EXPOSE 8080
VOLUME ["/app/data"]

# Docker marks the container unhealthy when the server stops answering, so
# `docker ps` and anything in front of the container can tell a server that
# hangs from one that runs. `message-crate-server healthcheck`
# (crates/server/server/src/healthcheck.rs) says what it asks and when it
# passes. The first start seeds the Demo Account before the server
# listens. A failed probe inside --start-period does not count against
# --retries, so a seed shorter than five minutes never marks the container
# unhealthy. --start-interval probes every 2 seconds during the start period
# (Docker Engine 25 and later), so a fresh container reads healthy as soon as
# the server answers.
HEALTHCHECK --interval=30s --timeout=5s --start-period=5m --start-interval=2s --retries=3 \
  CMD ["message-crate-server", "healthcheck"]

ENTRYPOINT ["tini", "--", "/usr/local/bin/docker-entrypoint-release.sh"]
