# termenbox-server image. Built and pushed by CI only (.github/workflows/docker-build.yaml).
# cargo-chef compiles the dependencies in their own layer, keyed on Cargo.lock, so a source change rebuilds
# only our crates. The runtime base comes from docker/Dockerfile.base (rebuilt weekly).
ARG BASE_IMAGE=ghcr.io/termenbox/termenbox-base:latest

FROM lukemathwalker/cargo-chef:latest-rust-1-slim-bookworm AS chef
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev cmake \
    && rm -rf /var/lib/apt/lists/*

# The workspace lists every package, so each must be present; only the server is built.
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY packages/core ./packages/core
COPY packages/server ./packages/server
COPY packages/agent ./packages/agent
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release -p termenbox-server --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY packages/core ./packages/core
COPY packages/server ./packages/server
COPY packages/agent ./packages/agent
# A release sets the full SemVer (0.1.36-nightly.20261022) so the image reports its channel; empty means the crate's.
ARG TERMENBOX_RELEASE_VERSION=
RUN if [ -z "$TERMENBOX_RELEASE_VERSION" ]; then unset TERMENBOX_RELEASE_VERSION; fi; \
    cargo build --release -p termenbox-server && strip target/release/termenbox-server

FROM ${BASE_IMAGE}
LABEL org.opencontainers.image.source="https://github.com/termenbox/termenbox" \
      org.opencontainers.image.description="Termenbox server: web terminal for remote AI coding agents" \
      org.opencontainers.image.licenses="MIT"
COPY --from=builder /app/target/release/termenbox-server /usr/local/bin/termenbox-server

# The base image already has a non-root user `node` (uid 1000); its home is the persistent volume.
USER node
WORKDIR /home/node
ARG TERMENBOX_BUILD=dev
ENV LANG=C.UTF-8 \
    TERMENBOX_BUILD=$TERMENBOX_BUILD \
    TERMENBOX_CONFIG_DIR=/home/node/.config/termenbox \
    TERMENBOX_HOST=127.0.0.1 \
    TERMENBOX_PORT=8080 \
    TERMENBOX_UDP_PORT=4443

# The HTTP/WebSocket port stays inside the container; clients reach the server on its UDP port.
HEALTHCHECK --interval=60s --timeout=5s --start-period=10s CMD curl -fs http://127.0.0.1:8080/health || exit 1
CMD ["termenbox-server"]
