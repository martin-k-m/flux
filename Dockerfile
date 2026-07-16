# Build Flux from source and ship a slim image.
#   docker build -t flux .
#   docker run --rm flux --version
FROM rust:1-slim AS build
WORKDIR /src
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
LABEL org.opencontainers.image.source="https://github.com/martin-k-m/flux" \
      org.opencontainers.image.description="Local-first developer automation platform" \
      org.opencontainers.image.licenses="Apache-2.0"
COPY --from=build /src/target/release/flux /usr/local/bin/flux
WORKDIR /workspace
ENTRYPOINT ["flux"]
CMD ["--help"]
