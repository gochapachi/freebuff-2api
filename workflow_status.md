# Workflow Status — Freebuff2API

> 单一状态源（长期任务恢复 / 节点协作 / 最终验收）。只记录事实与证据。
> Phase G/H/I/J 已发布；当前跟踪 **Phase K（v0.8.0，核心链路加固 + 信号量落地 + 面板现代化 + 安全加固）**。

## Phase L（v0.9.0，2026-09-18 完成）

- **来源**：`计划书/下一步改进指南.md`（v0.8.0 → v0.9.x 可执行清单）
- **范围**：web Cookie 凭证池化 + 多账号轮询 / 上游模型元数据契约与 efforts 对齐 / 面板测试台多模态多轮 / 账号健康看板 / 模型推荐 / 全配置导出导入 / 鉴权纵深 / 文档与 CI 门禁 / 发布 v0.9.0
- **状态**：✅ DONE（全部条目落地，证据见下）

### Phase L 交付清单（含证据）

| ID | 目标 | 交付物 | 状态 | 证据 |
|----|------|--------|------|------|
| L1 | web Cookie 凭证池化 + 多账号轮询 | `src/web_pool.rs`（WebCookiePool：健康分/熔断/冷却/轮询）+ api.rs 全链路接入（chat/messages/余额/详情/上传/清理）+ 导入热刷新 | ✅ | 7 单测 + 5 集成全绿；`/api/accounts/health` 实测含 web-cookie 条目 |
| L2 | 上游模型元数据契约 + efforts 对齐 | `ModelMeta` 静态权威表（premium/multimodal/available/efforts/fallback）+ clamp 阶梯对齐 + `/v1/models` meta | ✅ | models 4 单测 + model_meta_test 4 全绿；/v1/models 实测 19 条 meta、6 个不可用正确标记 |
| L3 | 面板测试台多模态/多轮/effort | web.rs play tab：多轮上下文、system、effort 下拉、图片上传（拖拽/粘贴/选择）、复制/导出 | ✅ | check_panel_js 通过；headless Chrome DOM 渲染验证 |
| L4 | 账号健康看板 + 模型推荐 + 数据迁移 + 耗时时间线 | /api/accounts/health + 面板健康/推荐/迁移/时间线 | ✅ | e2e_phase_v0_9 26 断言全绿 |
| L5 | 鉴权纵深 | inject_peer（ConnectInfo→x-fb-peer）+ is_loopback_request 对端判定 + doctor listen_scope | ✅ | doctor 实测含 listen_scope；默认行为不变 |
| L6 | 全配置导出/导入 | src/export.rs + /api/export + /api/import（schema 校验/备份/安全最小集） | ✅ | export 单测 4 + E2E round-trip 实测 |
| L7 | 文档/CI/归档 | README 计数修正、旧报告归档 docs/archive、CI 加 fmt + llvm-cov 门禁、CHANGELOG 0.9.0、版本三同步 | ✅ | rg 236 无残留；YAML 解析通过 |

### Phase L 验证基线（v0.9.0）

- **单元/集成**：253 单测 + 8 core + 4 model_meta + 11 router + 5 web_pool **全绿**（`cargo test`）
- **Lint**：`cargo clippy --all-targets -- -D warnings` 零警告；`cargo fmt --check` 通过
- **真实 E2E**：`tests/e2e_phase_v0_9.cjs` 26 断言全绿（真实网关 47871）；`tests/e2e_phase_v0_8.cjs` 26 断言回归全绿
- **真实浏览器**：headless Chrome 渲染面板 → JS 完整执行（model-count 占位符→20、模型 chips 渲染、全部 v0.9 控件在 DOM）
- **新端点实测**：/api/accounts/health（bearer+web-cookie+history）、/api/export、/api/import（坏 schema 400、round-trip 200+备份）、/v1/models meta、doctor listen_scope
- **环境限制（诚实披露）**：`cargo test --doc` 在本机失败（HEAD 基线同样失败：chocolatey Rust 缺少 rustdoc.exe），非本次改动引入

## Task Contract — Phase K（v0.8.0，2026-09-15 完成）

- **来源**：`计划书/下一步改进指南.md`（v0.7.3 → v0.8+ 可执行清单）
- **目标**：把"已宣称未落地"能力补实（双桶并发信号量）、补齐 Claude 路径能力差（重试/记账/记忆）、面板体验现代化（新增测试台/设置/关于页 + 品牌化 + windowed 渲染）、安全/高可用细节（CSP、脱敏、端口冲突提示、桌面多开）。
- **授权**：全部（含测试、验收、提交、发布）。

## Phase K 交付清单（含证据）

| ID | 目标 | 交付物 | 状态 | 证据 |
|----|------|--------|------|------|
| K1 | 双桶并发信号量落地 | `src/semaphore.rs`（TieredSemaphore + TierGuard RAII）接入 chat/claude/web_bridge 三路径；config 4 项 + env | ✅ DONE | 6 单测全绿（桶独立/超时 429/RAII 无泄漏/订阅判定） |
| K2 | Claude 路径重试 + 记账 + 记忆 | handle_claude_messages 重写：重试循环/waiting_room 503/非流式+流式 usage 落库/memory.observe | ✅ DONE | Router 测试 messages_waiting_room 503 + messages_non_stream 200 + api 单测 |
| K3 | Cookie 判定收窄（%3A 误判修复） | `looks_like_cookie` 收窄为三特征 | ✅ DONE | api 单测 cookie_detection_requires_known_markers |
| K4 | web_threads 容量上限 + TTL | `MAX_BINDINGS=2000` + 24h TTL + 裁剪 | ✅ DONE | web_threads 单测 prune_caps / prune_removes_expired |
| K5 | 面板现代化 + 新页 | 测试台/设置/关于 tab + /api/config 读写 + CSS token + windowed 渲染 | ✅ DONE | e2e_phase_v0_8.cjs 23 断言全绿（真实网关 47871） |
| K6 | 安全头 + CSP + 脱敏 | secure_headers layer + `redact.rs` 接入日志/遥测 | ✅ DONE | e2e 验证 nosniff/Referrer-Policy/CSP；redact 6 单测 |
| K7 | 跨站 403 语义 | origin_blocked() 替换数据面跨站分支 | ✅ DONE | Router 测试 cross_site_origin_blocked_403 |
| K8 | 桌面多开 + 托盘体检 hash | `requestSingleInstanceLock` + `openConsoleAt` | ✅ DONE | `node --check desktop/main.js` 通过 |
| K9 | 端口占用中文错误 | main.rs bind 失败分类提示 | ✅ DONE | 实测占用 47871 报中文错误 + netstat 提示 |
| K10 | Router 级集成测试 | `tests/router_test.rs` 10 用例（Mock TCP 上游） | ✅ DONE | cargo test --test router_test 10/10 全绿 |
| K11 | 文档版本失真修复 | README 中/英版本号 + 信号量表述；CHANGELOG 0.8.0；API_GUIDE 新端点/配置 | ✅ DONE | `rg 0.3.0` 无残留（README 中英）；config.example 同步 |
| K12 | 版本三同步 + 发布 | Cargo.toml / desktop/package.json = 0.8.0 | ✅ DONE | release 构建 v0.8.0 |

## Phase K 独立审查（Critic，2026-09-15）

| 级别 | 发现 | 处理 |
|------|------|------|
| CRITICAL | CSP `default-src 'self'` 阻断面板**全部内联脚本/样式**（无构建单文件无法满足），整面板瘫痪（headless Edge 实测 116 处违规；E2E 只做 HTTP 文本断言未捕获） | ✅ 已修复：CSP 放行 `script-src/style-src 'unsafe-inline'`，保留 `object-src 'none'`/`base-uri`/`frame-ancestors` 防护；headless Edge 复验 model-count 被 JS 填充、0 违规 |
| MEDIUM | 信号量语义：README 宣称 {slot:1,multi:3} 暗示并发 4，实现每请求同占二者 → 实际 = min(slots,multi)=1 | ✅ 已修复：README/CHANGELOG/模块注释澄清"实际并发=槽位容量"；multi 为上游策略预留维度 |
| MEDIUM | Claude 流式遥测用新 UUID（claude_req_id）与请求级 req_id 断裂，事件链无法关联 | ✅ 已修复：流式复用 req_id |
| LOW | 裸 JWT（无 marker 前缀）不被脱敏（防御纵深缺口） | ✅ 已修复：redact 增 redact_jwt（eyJ 三段式 + 长度门槛）+ 2 单测 |
| MEDIUM | renderLogs 滚动阈值 `>200` 与 windowed 生效区间（~100 条起）不一致 | ✅ 已修复：阈值对齐（>100） |
| LOW | telemetry 恒脱敏不随 redact_logs 开关（比文档更严格） | 保留（更安全；文档已说明） |
| LOW | `claude_upstream` 仅 2xx 赋值 → `!is_success()` 分支死代码 | 保留（与 chat 路径同构，安全兜底） |
| P1 提示 | E2E 需补真实浏览器渲染验证 | ✅ 已补：headless Edge 面板 DOM 验证进入验收流程 |

**Critic 结论**：CONDITIONAL PASS → 修复后升级为 **PASS**（全部发现已闭环）

## 验证基线（v0.8.0）

- **单元测试**：238 通过 / 0 失败（`cargo test --lib`）
- **集成测试**：`tests/core_test.rs` 8 项全绿
- **Router 级集成**：`tests/router_test.rs` 10 项全绿（Mock 上游，不打真实付费 API）
- **Lint**：`cargo clippy --all-targets -- -D warnings` 零警告；`cargo fmt --check` 通过
- **真实 E2E**：`tests/e2e_phase_v0_8.cjs` 26 断言全绿（真实网关 47871：安全头/CSP/配置读写白名单/新 tab/windowed/脱敏/healthz）
- **真实浏览器**：headless Edge 渲染面板 → JS 完整执行（model-count 占位符→20），0 处 CSP 违规
- **端口冲突实测**：第二个实例绑定同端口 → 明确中文错误 + `netstat -ano` 排查提示
- **配置持久化实测**：写回 thread_max_age_hours=48 → 干净重启读回 48 ✓

## 明确不做 / Backlog（沿用）

- SSE 攒批提交（NewAPI-Gateway 模式）→ 保留 backlog（收益/风险比不足）
- web 版 agent-runs/stream、web 版广告链、视频上传
- 密码学随机 Key 轮换宽限期、桌面版多账号轮询增强（P3 记录）
- llvm-cov 覆盖率门禁（本机未装 `cargo llvm-cov`；`--fail-under-lines 80` 命令已写入 CI 建议）

## Phase J 存档（v0.7.0 已发布）

- WebView2 一键登录（commit d09397a）；J6 完整成功路径（人工登录）待用户配合一次
- 基线：216 单测全绿 · 真实窗口验证两轮

## Phase H/I 存档（v0.6.0 已发布，commit 1435a8f）

- 213 单测全绿 · clippy 0 警告 · phase_g 46/46 + phase_i 18/18
