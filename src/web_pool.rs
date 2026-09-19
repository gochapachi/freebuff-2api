//! web Cookie 凭证池（v0.9）：多 web 账号健康评分 + 轮询 + 熔断/冷却
//!
//! 背景：Bearer 账号池（`pool.rs`）只覆盖桌面版 Bearer 协议；web Cookie 凭证在
//! v0.8 及以前只取"第一个有效凭证"（`pick_web_cookie`），无健康分/轮询/冷却，
//! 一个失效账号会让整个桥接链路 401。
//!
//! 本模块把 web Cookie 凭证**池化**：复用 `pool.rs::CircuitBreaker` 语义
//! （Closed/Open/HalfOpen、连续失败熔断、指数冷却封顶 10 分钟、HalfOpen 探测闸门），
//! 提供：
//! - `pick()`：选未熔断 + 冷却期满 + 健康分最高的凭证（多账号轮询）
//! - `mark_ok()` / `mark_failure()` / `mark_cooldown()`：请求结果回写
//! - `snapshot()`：面板账号健康展示
//!
//! 网络安全边界：web Cookie 是用户账号的完整会话凭证，仅存本机内存与本地
//! `tokens.json`，不出网、不写日志明文（快照/面板展示一律脱敏）。

use crate::config::Config;
use crate::import;
use crate::pool::CircuitBreaker;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// 凭证来源
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebCredSource {
    Config,
    Imported,
}

/// 池内条目（内部可变态）
#[derive(Debug)]
struct WebEntry {
    cookie: String,
    id: String,
    source: WebCredSource,
    added_at: Option<String>,
    score: f64,
    breaker: CircuitBreaker,
    last_ok_at: Option<Instant>,
}

/// 挑选结果（调用方只拿 cookie + 展示信息 + 稳定 id，不持有内部态）
#[derive(Debug, Clone)]
pub struct WebPick {
    pub cookie: String,
    pub id: String,
    pub source: &'static str,
    pub added_at: Option<String>,
    /// 展示信息（source/added_at/token_masked/id），与旧 pick_web_cookie 返回兼容
    pub cred: serde_json::Value,
}

/// 健康快照（串行化，供面板）
#[derive(Debug, Clone, serde::Serialize)]
pub struct WebCookieHealth {
    pub id: String,
    pub kind: &'static str,
    pub source: &'static str,
    pub added_at: Option<String>,
    /// 脱敏 Cookie（前 6 后 4）
    pub masked: String,
    pub health_score: f64,
    pub circuit_state: String,
    pub cooldown_until: Option<String>,
    /// v0.10：冷却剩余秒（面板排序/预估恢复用）
    pub cooldown_seconds: Option<u64>,
    pub trips: u64,
    pub last_error: Option<String>,
    pub last_ok_at: Option<String>,
}

/// 与旧 `pick_web_cookie` 一致的 Cookie 判定（只认 next-auth 三件套特征，防 URL 编码串误判）
pub fn looks_like_cookie(t: &str) -> bool {
    t.contains("session-token") || t.contains(".next-auth") || t.contains("callback-url")
}

fn mask(cookie: &str) -> String {
    if cookie.len() <= 10 {
        return "***".to_string();
    }
    format!("{}...{}", &cookie[..6], &cookie[cookie.len() - 4..])
}

/// 冷却到期时刻 → ISO8601 UTC（v0.10：面板按时间轴展示；剩余秒单独给 cooldown_seconds）
fn cooldown_iso(instant: Option<Instant>) -> Option<String> {
    instant.map(|i| {
        let d = i.saturating_duration_since(Instant::now());
        (chrono::Utc::now() + chrono::Duration::from_std(d).unwrap_or_default()).to_rfc3339()
    })
}

/// 冷却剩余秒
fn cooldown_secs(instant: Option<Instant>) -> Option<u64> {
    instant.map(|i| i.saturating_duration_since(Instant::now()).as_secs())
}

impl WebEntry {
    fn new(cookie: String, source: WebCredSource, added_at: Option<String>, id: String) -> Self {
        Self {
            cookie,
            id,
            source,
            added_at,
            score: 0.0,
            breaker: CircuitBreaker::new(),
            last_ok_at: None,
        }
    }
}

pub struct WebCookiePool {
    inner: RwLock<Vec<WebEntry>>,
}

impl WebCookiePool {
    /// 构建条目（config 优先，同 cookie 去重）
    fn build_entries(cfg: &Config) -> Vec<WebEntry> {
        let mut map: HashMap<String, WebEntry> = HashMap::new();
        for t in &cfg.auth_tokens {
            if looks_like_cookie(t) {
                let id = import::cred_id(t);
                map.entry(id.clone())
                    .or_insert_with(|| WebEntry::new(t.clone(), WebCredSource::Config, None, id));
            }
        }
        if let Ok(toks) = import::load_tokens_healed(&cfg.tokens_path) {
            for t in toks {
                if looks_like_cookie(&t.token) {
                    let id = import::cred_id(&t.token);
                    map.entry(id.clone()).or_insert_with(|| {
                        WebEntry::new(
                            t.token.clone(),
                            WebCredSource::Imported,
                            t.added_at.clone(),
                            id,
                        )
                    });
                }
            }
        }
        map.into_values().collect()
    }

    /// 从配置 + tokens.json 构建（config 优先，同 cookie 去重）
    pub fn load(cfg: &Config) -> Self {
        Self {
            inner: RwLock::new(Self::build_entries(cfg)),
        }
    }

    /// 导入/删除凭证后重建池（保留既有健康状态与熔断冷却）
    pub async fn reload(&self, cfg: &Config) {
        let fresh = Self::build_entries(cfg);
        let mut cur = self.inner.write().await;
        let mut merged: Vec<WebEntry> = Vec::with_capacity(fresh.len());
        for mut e in fresh {
            if let Some(i) = cur.iter().position(|o| o.id == e.id) {
                let prev = cur.remove(i);
                e.score = prev.score;
                e.breaker = prev.breaker;
                e.last_ok_at = prev.last_ok_at;
            }
            merged.push(e);
        }
        *cur = merged;
    }

    /// 别名：与 Config 构建入口一致（`load` 为规范名）
    pub fn new(cfg: &Config) -> Self {
        Self::load(cfg)
    }

    /// 热追加（导入新 Cookie 后调用；按 id 去重），返回是否真正新增
    pub async fn add_if_absent(
        &self,
        cookie: &str,
        source: &str,
        added_at: Option<String>,
    ) -> bool {
        let mut entries = self.inner.write().await;
        let id = import::cred_id(cookie);
        if entries.iter().any(|e| e.id == id) {
            return false;
        }
        let src = if source == "config" {
            WebCredSource::Config
        } else {
            WebCredSource::Imported
        };
        entries.push(WebEntry::new(cookie.to_string(), src, added_at, id));
        true
    }

    /// 热移除（凭证删除后调用），返回是否真的移除了
    pub async fn remove(&self, id: &str) -> bool {
        let mut entries = self.inner.write().await;
        let before = entries.len();
        entries.retain(|e| e.id != id);
        entries.len() != before
    }

    /// 空池？
    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }

    pub async fn count(&self) -> usize {
        self.inner.read().await.len()
    }

    /// 挑选健康分最高且熔断允许的凭证；全不可用 → None
    pub async fn pick(&self) -> Option<WebPick> {
        let mut entries = self.inner.write().await;
        let mut best: Option<usize> = None;
        // 下标访问（avoid iter_mut + 闭包二次借用冲突；breaker.allow 可变语义不变）
        for i in 0..entries.len() {
            if !entries[i].breaker.allow() {
                continue;
            }
            let score = entries[i].score;
            if best.map(|b| score > entries[b].score).unwrap_or(true) {
                best = Some(i);
            }
        }
        best.map(|i| {
            let e = &entries[i];
            let source_label: &'static str = match e.source {
                WebCredSource::Config => "config",
                WebCredSource::Imported => "imported",
            };
            WebPick {
                cookie: e.cookie.clone(),
                id: e.id.clone(),
                source: source_label,
                added_at: e.added_at.clone(),
                cred: serde_json::json!({
                    "source": source_label,
                    "added_at": e.added_at,
                    "token_masked": mask(&e.cookie),
                    "id": e.id,
                }),
            }
        })
    }

    /// 业务成功（HalfOpen 探测成功 → 逐步恢复）
    pub async fn mark_ok(&self, id: &str) {
        let mut entries = self.inner.write().await;
        if let Some(e) = entries.iter_mut().find(|e| e.id == id) {
            e.breaker.record_success();
            e.score = (e.score + 1.0).min(100.0);
            e.last_ok_at = Some(Instant::now());
        }
    }

    /// 业务失败（连续失败超阈值才熔断）
    pub async fn mark_failure(&self, id: &str, reason: &str) {
        let mut entries = self.inner.write().await;
        if let Some(e) = entries.iter_mut().find(|e| e.id == id) {
            e.breaker.record_failure(reason);
            e.score = (e.score - 2.0).max(-10.0);
            tracing::warn!("web Cookie 凭证 {} 失败: {reason}", mask(&e.cookie));
        }
    }

    /// 确定性失败（401/403 等）→ 立即熔断冷却，不依赖连续失败计数
    pub async fn mark_cooldown(&self, id: &str, duration: Duration, reason: &str) {
        let mut entries = self.inner.write().await;
        if let Some(e) = entries.iter_mut().find(|e| e.id == id) {
            e.breaker.trip_for(duration, reason);
            e.score = (e.score - 4.0).max(-10.0);
            tracing::warn!(
                "web Cookie 凭证 {} 冷却 {duration:?}: {reason}",
                mask(&e.cookie)
            );
        }
    }

    /// 健康快照（供 /api/accounts/health 与账号列表）
    pub async fn snapshot(&self) -> Vec<WebCookieHealth> {
        let entries = self.inner.read().await;
        entries
            .iter()
            .map(|e| WebCookieHealth {
                id: e.id.clone(),
                kind: "web-cookie",
                source: match e.source {
                    WebCredSource::Config => "config",
                    WebCredSource::Imported => "imported",
                },
                added_at: e.added_at.clone(),
                masked: mask(&e.cookie),
                health_score: e.score,
                circuit_state: match e.breaker.state {
                    crate::pool::CircuitState::Closed => "closed".into(),
                    crate::pool::CircuitState::Open => "open".into(),
                    crate::pool::CircuitState::HalfOpen => "half_open".into(),
                },
                cooldown_until: cooldown_iso(e.breaker.open_until),
                cooldown_seconds: cooldown_secs(e.breaker.open_until),
                trips: e.breaker.trips,
                last_error: e.breaker.last_reason.clone(),
                last_ok_at: e.last_ok_at.map(|i| format!("{i:?}")),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn cfg_with(tokens: Vec<&str>, tokens_path: &str) -> Config {
        Config {
            auth_tokens: tokens.iter().map(|s| s.to_string()).collect(),
            tokens_path: tokens_path.to_string(),
            ..Config::default()
        }
    }

    #[tokio::test]
    async fn load_collects_config_cookies_only() {
        let c = cfg_with(
            vec!["__Secure-next-auth.session-token=abc", "bearer-plain"],
            "no-such-tokens.json",
        );
        let pool = WebCookiePool::load(&c);
        assert_eq!(pool.count().await, 1, "只有 Cookie 值入池");
    }

    #[tokio::test]
    async fn pick_prefers_healthy_and_higher_score() {
        let c = cfg_with(
            vec![
                "__Secure-next-auth.session-token=good-one",
                "__Secure-next-auth.session-token=bad-one",
            ],
            "no-such-tokens.json",
        );
        let pool = WebCookiePool::load(&c);
        assert_eq!(pool.count().await, 2);
        // 直接操作内部计分：bad 低分，good 高分 → pick 应选 good
        {
            let mut n = pool.inner.write().await;
            for e in n.iter_mut() {
                e.score = if e.cookie.contains("bad-one") {
                    -5.0
                } else {
                    30.0
                };
            }
        }
        let p = pool.pick().await.unwrap();
        assert!(p.cookie.contains("good-one"), "应选高分凭证");
        assert!(p.cred.get("token_masked").is_some());
        assert!(p.cred.get("id").is_some());
    }

    #[tokio::test]
    async fn cooldown_skips_until_expiry() {
        let c = cfg_with(
            vec!["__Secure-next-auth.session-token=only"],
            "no-such-tokens.json",
        );
        let pool = WebCookiePool::load(&c);
        let p0 = pool.pick().await.unwrap();
        pool.mark_cooldown(&p0.id, Duration::from_secs(3600), "401 expired")
            .await;
        // 冷却期内：无可用 → None
        assert!(pool.pick().await.is_none());
        // 快照能反映冷却
        let snap = pool.snapshot().await;
        assert_eq!(snap[0].circuit_state, "open");
        assert!(snap[0].cooldown_until.is_some());
        assert_eq!(snap[0].last_error.as_deref(), Some("401 expired"));
    }

    #[tokio::test]
    async fn mark_ok_recovers_half_open() {
        let c = cfg_with(
            vec!["__Secure-next-auth.session-token=a"],
            "no-such-tokens.json",
        );
        let pool = WebCookiePool::load(&c);
        let p0 = pool.pick().await.unwrap();
        pool.mark_cooldown(&p0.id, Duration::from_millis(1), "boom")
            .await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        // 到期后 allow() 进入 HalfOpen 放行一次探测（探测在途期间不再放行，故直接取该次结果）
        let p1 = pool.pick().await.expect("冷却到期应放行单个探测");
        pool.mark_ok(&p1.id).await;
        let snap = pool.snapshot().await;
        assert_eq!(
            snap[0].circuit_state, "half_open",
            "HalfOpen 一次成功待二次确认"
        );
        // 第二次成功 → 恢复 Closed
        pool.mark_ok(&p1.id).await;
        let snap2 = pool.snapshot().await;
        assert_eq!(snap2[0].circuit_state, "closed");
        assert!(snap2[0].last_ok_at.is_some());
    }

    #[tokio::test]
    async fn mark_failure_accumulates_until_trip() {
        let c = cfg_with(
            vec!["__Secure-next-auth.session-token=a"],
            "no-such-tokens.json",
        );
        let pool = WebCookiePool::load(&c);
        let p0 = pool.pick().await.unwrap();
        // 连续 4 次失败 → Closed→Open
        for _ in 0..4 {
            pool.mark_failure(&p0.id, "upstream 5xx").await;
        }
        let snap = pool.snapshot().await;
        assert_eq!(snap[0].circuit_state, "open");
        assert!(pool.pick().await.is_none());
    }

    #[tokio::test]
    async fn snapshot_masks_cookie() {
        let c = cfg_with(
            vec!["__Secure-next-auth.session-token=0123456789abcdef"],
            "no-such-tokens.json",
        );
        let pool = WebCookiePool::load(&c);
        let snap = pool.snapshot().await;
        assert!(snap[0].masked.contains("..."));
        assert!(!snap[0].masked.contains("0123456789abcdef"), "不得明文");
        assert_eq!(snap[0].kind, "web-cookie");
    }

    #[tokio::test]
    async fn empty_pool_picks_none() {
        let c = cfg_with(vec![], "no-such-tokens.json");
        let pool = WebCookiePool::load(&c);
        assert!(pool.is_empty().await);
        assert!(pool.pick().await.is_none());
        assert!(pool.snapshot().await.is_empty());
    }
}
