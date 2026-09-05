# ====================================
# STAGE 1: The Builder (Isolated Cargo Clean Room )
# ====================================
# The official, heavy Rust image to compile the OS.
FROM rust:bookworm AS builder

# Create a sterile working Dir
WORKDIR /usr/src/raqim

# Intall native C toolchain & build dependenciesp
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    build-essential \
    cmake \
    protobuf-compiler \
    libprotobuf-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy the workspace manifest and spurce tree
COPY Cargo.toml Cargo.lock ./
COPY raqim-core ./raqim-core
COPY raqim-cli ./raqim-cli
COPY raqim-mcp ./raqim-mcp
COPY raqim-siege ./raqim-siege
COPY raqim-py ./raqim-py

# Compile optimizeed binaries with stripped debug symbols
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
RUN cargo build --release --bin raqim-core --bin raqim-cli --bin raqim-mcp

# Strip binary symbols for minimal size
RUN strip /usr/src/raqim/target/release/raqim-core && \
    strip /usr/src/raqim/target/release/raqim-cli && \
    strip /usr/src/raqim/target/release/raqim-mcp

# =================================================================
# STAGE 2: Production Runtime (Minimal Attack Surface)
# ==================================================================
FROM debian:bookworm-slim AS runtime

# Install runtime dynamic libraries & TLS CA root certificates
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root unprivileged service user
RUN groupadd -g 10001 raqim && \
    useradd -u 10001 -g raqim -d /var/lib/raqim -m -s /sbin/nologin raqim

# Establish persistent storage diirectories with non-root ownership 
WORKDIR /var/lib/raqim 
RUN mkdir -p /var/lib/raqim/data /var/lib/raqim/ca-keys /var/lib/raqim/vault && \
    chown -R raqim:raqim /var/lib/raqim

# Copy stripped binaries from Stage 1
COPY --from=builder /usr/src/raqim/target/release/raqim-core /usr/local/bin/raqim-core
COPY --from=builder /usr/src/raqim/target/release/raqim-cli /usr/local/bin/raqim-cli
COPY --from=builder /usr/src/raqim/target/release/raqim-mcp /usr/local/bin/raqim-mcp

# Volume declaration for persistence
VOLUME ["/var/lib/raqim/data", "/var/lib/raqim/ca-keys", "/var/lib/raqim/vault"]


# Expose the TCP Firehose (8080), Zenoh Mesh (7447) and HTTP Admin (8081)
EXPOSE 8080 7447 8081

# Drop to non-root user
USER raqim:raqim

# Default runtime configuration
ENV RUST_LOG=info
ENV RAQIM_WAL_PATH=/var/lib/raqim/data/production.wal
ENV RAQIM_LANCE_PATH=/var/lib/raqim/data/production_semantic.lancedb

# The container execute the binary directly 
ENTRYPOINT ["/usr/local/bin/raqim-core"] 
