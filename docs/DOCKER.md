# Docker — Freebuff2API 容器化（v0.10）

> 本机无 Docker 时，镜像构建/推送由 CI（`.github/workflows/docker.yml`）在 main 分支 push 时自动完成（多架构 amd64+arm64 → ghcr.io/lza6/freebuff2api）。

## 一、镜像

- 注册表：`ghcr.io/lza6/freebuff2api`
- 标签：`latest`（默认分支）、`dev`（dev 分支）、sha 标签
- Dockerfile：`docker/Dockerfile`（多阶段：rust:1.95-bookworm 构建 → debian:bookworm-slim 运行；仅 ca-certificates 运行时依赖；rustls 纯 Rust TLS，无需 OpenSSL）

## 二、本机实跑（需 Docker）

```bash
# 构建
docker build -f docker/Dockerfile -t freebuff2api:local .

# 准备配置（listen_addr 需 0.0.0.0 以便容器端口映射；本机无凭证也可 skip_upstream_check）
cat > config.json <<'EOF'
{
  "listen_addr": "0.0.0.0:47821",
  "upstream_base_url": "https://www.codebuff.com",
  "auth_tokens": [],
  "api_keys": [],
  "skip_upstream_check": true,
  "memory_enabled": false,
  "sqlite_path": "/data/freebuff2api.sqlite",
  "telemetry_path": "/data/telemetry.sqlite",
  "memory_path": "/data/memory.sqlite",
  "skills_dir": "/data/skills",
  "redact_logs": true
}
EOF

# 运行
docker run -d --name fba -p 47821:47821 -v "$(pwd)/config.json:/data/config.json:ro" -v fba-data:/data freebuff2api:local

# 冒烟
curl -sf http://127.0.0.1:47821/healthz && echo OK
curl -sf http://127.0.0.1:47821/ui | head -c 200

# 清理
docker rm -f fba
```

## 三、已知改进项

- ✅ v0.10.2：Dockerfile 已加 `HEALTHCHECK`（运行阶段安装 curl，`curl -sf /healthz`，interval 30s / start_period 10s）
- 容器内 `thread_cleanup_interval_sec`/`thread_max_age_hours` 默认沿用 config 默认；多实例部署时注意 `/data` 卷唯一。

## 四、CI 证据（2026-09-19）

- `Build & Push Docker Image`（docker.yml）在 main push（b561752）→ **success**（amd64 + arm64 双平台构建 + GHCR manifest 合并推送）
- 本机环境限制：当前主机无 docker CLI，容器内 healthz 实跑需在有 Docker 的主机上按第二节命令执行（命令即验证脚本）。