# Freebuff2API Rust 版 — 多阶段构建
FROM rust:1.95-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main(){}' > src/main.rs && echo '' > src/lib.rs && \
    cargo build --release 2>/dev/null || true

COPY src/ src/
COPY browser-extension/ browser-extension/
COPY tests/ tests/
# 必须 touch 触发全量真实源码编译，避免 BuildKit 缓存导致空 main() 骨架被打包
RUN touch src/main.rs src/lib.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/freebuff2api /usr/local/bin/freebuff2api
ENV PORT=47821
VOLUME ["/data"]
EXPOSE 47821
ENTRYPOINT ["freebuff2api", "--config", "/data/config.json"]
