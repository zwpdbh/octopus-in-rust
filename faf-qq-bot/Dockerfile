# syntax=docker/dockerfile:1

# Build stage
FROM rust:1-bookworm AS builder

WORKDIR /usr/src/faf-qq-bot

# Copy manifest first to cache dependencies.
COPY faf-qq-bot/Cargo.toml ./Cargo.toml

# Create a dummy main.rs so dependency compilation can be cached.
RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN cargo build --release

# Copy the real source and build.
COPY faf-qq-bot/src ./src
COPY faf-qq-bot/config.example.toml ./config.example.toml
RUN touch src/main.rs
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1000 faf
WORKDIR /app

COPY --from=builder /usr/src/faf-qq-bot/target/release/faf-qq-bot /usr/local/bin/faf-qq-bot
COPY --from=builder /usr/src/faf-qq-bot/config.example.toml /app/config.example.toml

USER faf

# The config.toml is expected to be mounted at runtime.
ENTRYPOINT ["/usr/local/bin/faf-qq-bot"]
CMD ["/app/config.toml"]
