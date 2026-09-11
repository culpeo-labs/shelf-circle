# syntax=docker/dockerfile:1

# ---- build ----
# Debian 13 "trixie". SQLx here uses compile-time-*unchecked* `query_as` and
# `sqlx::migrate!` embeds the SQL at build time, so the build needs no database.
FROM rust:1-trixie AS build
WORKDIR /app

# Prime the dependency cache: build with a stub main against the real manifest,
# so `cargo build` below only recompiles our crate when sources change.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY . .
RUN touch src/main.rs \
    && cargo build --release --locked --bin shelf-circle-backend

# ---- runtime ----
FROM debian:trixie-slim
# rustls needs the system trust store to reach Hanko / Open Library / Postgres.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 10001 appuser

COPY --from=build /app/target/release/shelf-circle-backend /usr/local/bin/shelf-circle-backend

USER appuser
ENV RUST_LOG=info
EXPOSE 8080
CMD ["shelf-circle-backend"]
