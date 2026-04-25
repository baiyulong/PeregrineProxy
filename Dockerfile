# Stage 1: Builder
FROM rust:latest AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/peregrine /usr/local/bin/peregrine
COPY config.example.yaml /etc/peregrine/config.yaml
EXPOSE 8080 1080 9090
ENTRYPOINT ["peregrine"]
CMD ["--config", "/etc/peregrine/config.yaml"]
