FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app
LABEL org.opencontainers.image.source=https://github.com/paradigmxyz/reth
LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"

# Install system dependencies
RUN apt-get update && apt-get -y upgrade && apt-get install -y libclang-dev pkg-config git

# Builds a cargo-chef plan
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

# Build profile, release by default
ARG BUILD_PROFILE=release
ENV BUILD_PROFILE $BUILD_PROFILE

# Extra Cargo flags
ARG RUSTFLAGS=""
ENV RUSTFLAGS "$RUSTFLAGS"

# Extra Cargo features
ARG FEATURES=""
ENV FEATURES $FEATURES

# Builds dependencies
RUN cargo chef cook --profile $BUILD_PROFILE --features "$FEATURES" --recipe-path recipe.json

# Build application
COPY . .
RUN cargo build --profile $BUILD_PROFILE --features "$FEATURES" --locked --bin reth

# Clone and build rbuilder (gwyneth branch)
RUN git clone -b gwyneth https://github.com/taikoxyz/rbuilder.git /app/rbuilder
WORKDIR /app/rbuilder
RUN cargo build --release

# Copy binaries to a temporary location
RUN cp /app/target/$BUILD_PROFILE/reth /app/reth
RUN cp /app/rbuilder/target/release/rbuilder /app/rbuilder

# Use Ubuntu as the release image
FROM ubuntu AS runtime
WORKDIR /app

# Install necessary runtime dependencies
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy reth and rbuilder over from the build stage
COPY --from=builder /app/reth /usr/local/bin
COPY --from=builder /app/rbuilder /usr/local/bin

# Copy the entire rbuilder repository
COPY --from=builder /app/rbuilder /app/rbuilder

# Copy licenses
COPY LICENSE-* ./

EXPOSE 30303 30303/udp 9001 8545 8546

ENTRYPOINT ["/usr/local/bin/reth"]