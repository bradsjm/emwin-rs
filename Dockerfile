ARG RUST_VERSION=1.94
ARG ALPINE_VERSION=3.22

FROM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS builder
WORKDIR /app
RUN apk add --no-cache musl-dev
COPY . .
RUN cargo build --release -p emwin-cli

FROM alpine:${ALPINE_VERSION}
LABEL org.opencontainers.image.description="EMWIN CLI with stream, server, inspect, and relay subcommands"

RUN apk add --no-cache ca-certificates && addgroup -S emwin && adduser -S -G emwin emwin
COPY --from=builder /app/target/release/emwin-cli /usr/local/bin/emwin

USER emwin
ENTRYPOINT ["/usr/local/bin/emwin"]
