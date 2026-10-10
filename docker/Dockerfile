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

# -----------------------------------------------------------------------------
# Stage 1: Vite SPA frontend
# -----------------------------------------------------------------------------
FROM node:22-bookworm-slim AS frontend-builder

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
FROM rust:1.98-bookworm AS rust-builder

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
FROM node:20-bookworm-slim AS runtime

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates \
    ffmpeg \
    tini \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Rust CLI / serve binary
COPY --from=rust-builder /src/target/release/message-crate-server /usr/local/bin/message-crate-server

# Container config template
COPY config/config.docker.toml ./config/config.docker.toml

# Vite SPA (built in Stage 1)
COPY --from=frontend-builder /web/dist ./static

COPY docker/entrypoint-release.sh /usr/local/bin/docker-entrypoint-release.sh
RUN chmod +x /usr/local/bin/docker-entrypoint-release.sh \
  && mkdir -p /app/data \
  && chown -R node:node /app

USER node

EXPOSE 8080
VOLUME ["/app/data"]

# Docker marks the container unhealthy when the server stops answering, so
# `docker ps` and anything in front of the container can tell a server that
# hangs from one that runs. The probe asks /health with the node this base
# image already carries, so the image needs no curl. It assumes port 8080,
# the port config/config.docker.toml binds: a `serve --bind` with another
# port reads unhealthy while the server answers. The first start seeds the
# Demo Account before the server listens. A failed probe inside
# --start-period does not count against --retries, so a seed shorter than
# five minutes never marks the container unhealthy. --start-interval probes
# every 2 seconds during the start period (Docker Engine 25 and later), so a
# fresh container reads healthy as soon as the server answers.
HEALTHCHECK --interval=30s --timeout=5s --start-period=5m --start-interval=2s --retries=3 \
  CMD ["node", "-e", "fetch('http://127.0.0.1:8080/health').then((r) => process.exit(r.ok ? 0 : 1), () => process.exit(1))"]

ENTRYPOINT ["tini", "--", "/usr/local/bin/docker-entrypoint-release.sh"]
