# Testing — Freebuff2API 测试台账（v0.10）

> 单一测试事实源：数量、命令、基线、环境限制。**先看这里再改代码。**

## 一、测试规模（v0.10.0 基线）

| 层 | 数量 | 命令 |
|----|------|------|
| 单元测试（src 内） | 275 | `cargo test --lib` |
| Router 级集成（Mock TCP 上游） | 11 | `cargo test --test router_test` |
| 核心集成 | 8 | `cargo test --test core_test` |
| web Cookie 池集成 | 5 | `cargo test --test web_pool_test` |
| 模型元数据契约 | 7 | `cargo test --test model_meta_test` |
| 文档测试 | 环境受限 | `cargo test --doc`（CI 有 rust-docs 组件可跑；本机 chocolatey/rustup 缺 rustdoc 时跳过） |
| 真实网关 E2E | 26+26 断言 | `node tests/e2e_phase_v0_8.cjs <port>` / `e2e_phase_v0_9.cjs <port>`（需先起网关） |
| 面板 JS 语法 | 1 | `node tests/check_panel_js.cjs` |
| 模型合同对齐 | 1 | `node scripts/check_model_contract.mjs` |

## 二、本地一键验证

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify_release.ps1
```

覆盖：fmt --check → clippy -D warnings → lib 单测 → 4 个集成测试 → 面板 JS → 模型合同 → release 冒烟（healthz/ui/models 200）。

## 三、E2E 手动跑法

```powershell
# 1) 起网关（临时端口）
target\release\freebuff2api.exe --config <临时 config（listen_addr 127.0.0.1:47980, skip_upstream_check=true）>
# 2) 跑断言
node tests/e2e_phase_v0_8.cjs 47980 config.e2e.json
node tests/e2e_phase_v0_9.cjs 47980 config.e2e.json
```

## 四、覆盖率（v0.10.2 基线，2026-09-19 本机实测）

- 工具：`cargo-llvm-cov 0.9.1`（`cargo install cargo-llvm-cov --locked`；需 `rustup component add llvm-tools-preview`），跑：`cargo llvm-cov --fail-under-lines 65`
- **TOTAL 行覆盖率 66.8%**；**CI 已收紧为真门禁**：`--fail-under-lines 65` + `continue-on-error: false`（留 1.8pt 环境缓冲）
- 关键业务模块 ≥80%：models 83.2 / telemetry 94.4 / router 89.0 / web_pool 84.6 / errors 97.9 / retry 94.1 / redact 98.5 / import 92.8 / mcp 98.6 / memory 93.9 / export 84.0 / account_meta 81.6
- 拉低总体：api.rs 30.6（HTTP 单体，靠集成/E2E 覆盖）、main.rs 0（启动路径）、upstream 51.6 / session 65.6 / web_protocol 78.5（网络层，需 Mock 上游）
- **提升路径（backlog）**：补 api.rs 集成测试（Mock 上游扩 case）→ 70% → 75% → 80%

## 五、环境限制（诚实披露）

- 本机 cargo 为 rustup 1.95.0 stable-msvc；`cargo test --doc` 需要 rust-docs 组件（CI 已装；本机若缺可 `rustup component add rust-docs`）。
- E2E 需真实上游连通或空凭证配置；Mock 上游场景由 router_test 覆盖（不打真实付费 API）。