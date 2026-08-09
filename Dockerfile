FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev gcc
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM scratch
COPY --from=builder /app/target/release/mkcdoc /usr/local/bin/mkcdoc
WORKDIR /github/workspace
ENTRYPOINT ["/usr/local/bin/mkcdoc"]
