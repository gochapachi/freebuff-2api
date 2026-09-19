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

## 四、覆盖率

- CI：`.github/workflows/build-release.yml` coverage job（`cargo llvm-cov --fail-under-lines 80`，当前 continue-on-error=true 收集基线；v0.10 目标：本机记录真实基线 → 移除 continue-on-error 收紧，见 计划书/下一步改进指南.md §3.1）。
- 本机安装：`cargo install cargo-llvm-cov --locked`；跑：`cargo llvm-cov --fail-under-lines 80`。

## 五、环境限制（诚实披露）

- 本机 cargo 为 rustup 1.95.0 stable-msvc；`cargo test --doc` 需要 rust-docs 组件（CI 已装；本机若缺可 `rustup component add rust-docs`）。
- E2E 需真实上游连通或空凭证配置；Mock 上游场景由 router_test 覆盖（不打真实付费 API）。