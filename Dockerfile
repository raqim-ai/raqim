# ==============================================================================
# STAGE 1: Rust Builder (Clean-Room Compilation)
# ==============================================================================
FROM rust:bookworm AS builder

WORKDIR /usr/src/raqim

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY raqim-core ./raqim-core
COPY raqim-cli ./raqim-cli
COPY raqim-siege ./raqim-siege

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse

RUN cargo build --release --bin raqim-core && \
    cargo build --release --bin raqim-cli && \
    strip /usr/src/raqim/target/release/raqim-core && \
    strip /usr/src/raqim/target/release/raqim-cli

# ==============================================================================
# STAGE 2: Hardened Production Runtime (<85 MB Total Image)
# ==============================================================================
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -g 10001 raqim && \
    useradd -u 10001 -g raqim -d /var/lib/raqim -m -s /sbin/nologin raqim

WORKDIR /var/lib/raqim
RUN mkdir -p /var/lib/raqim/ca-keys /var/lib/raqim/vault && \
    chown -R raqim:raqim /var/lib/raqim

COPY --from=builder /usr/src/raqim/target/release/raqim-core /usr/local/bin/raqim-core
COPY --from=builder /usr/src/raqim/target/release/raqim-cli /usr/local/bin/raqim-cli

VOLUME ["/var/lib/raqim/ca-keys", "/var/lib/raqim/vault"]

# Expose Binary TCP Ingress (8080) and Agent HTTP Data & Egress Plane (8081)
EXPOSE 8080 8081

USER raqim:raqim

ENV RAQIM_ENV=production
ENTRYPOINT ["/usr/local/bin/raqim-core"]