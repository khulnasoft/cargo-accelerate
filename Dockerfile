# Cargo-chef Dockerfile recipe for optimized multi-stage Docker builds
FROM lukemathwalker/cargo-chef:latest-rust-1.84 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder 
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this layer is cached unless dependencies change
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release

# Run-time Stage
FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/cargo-accelerate /usr/local/bin/app
CMD ["app"]
