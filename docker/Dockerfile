# Freebuff2API Rust 版 — 多阶段构建
FROM rust:1.95-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main(){}' > src/main.rs && echo '' > src/lib.rs && \
    cargo build --release 2>/dev/null || true

COPY src/ src/
# 完整源码已就位：骨架缓存层由上一层 RUN 负责，这里直接全量构建（覆盖 lib+binary）
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
# v0.10.2：容器健康检查（gateway /healthz）
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD curl -sf http://127.0.0.1:47821/healthz >/dev/null || exit 1
COPY --from=builder /build/target/release/freebuff2api /usr/local/bin/freebuff2api
ENV PORT=47821
VOLUME ["/data"]
EXPOSE 47821
ENTRYPOINT ["freebuff2api", "--config", "/data/config.json"]
