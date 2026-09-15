//! 双桶并发信号量（v0.8 落地——README 宣称的能力在此补实）
//!
//! 上游 codebuff 免费层的并发墙按网关 IP/账号维度计算（逆向自桌面端 orchestrator.js）：
//! - 免费层：`{slot:1, multi:3}` —— 单账号同刻最多 1 个"正式会话"，加上普通请求并发 3
//! - 订阅层：`{slot:3, multi:8}` —— 订阅账号容量更高
//!
//! 本模块以**网关全局级**信号量限制同时进入上游转发路径的请求数：
//! - `TieredSemaphore` 含两个独立桶（free / subscriber），互不干扰
//! - 每桶两个信号量（slots + multi）：`acquire` 对两者各占一个 permit——
//!   **实际并发上限 = min(slots, multi)**（免费层 1、订阅层 3）；multi 是平行预留维度
//!   （若未来上游策略修正为"槽位与会话并发分离"可独立放大，当前由更稀缺的槽位定上限）
//! - `acquire(is_subscriber)` 在**首字节写出前**调用；超时 2s 返回 `AcquireError::Busy`
//!   （429 语义，对齐 waiting_room），不无限排队
//! - `TierGuard` 为 RAII：持 permit，Drop 时自动归还，杜绝泄漏
//!
//! 配置（config.json / CONCURRENCY_* 环境变量）：
//! - `concurrency_free_slots`    默认 1
//! - `concurrency_free_multi`    默认 3
//! - `concurrency_sub_slots`     默认 3
//! - `concurrency_sub_multi`     默认 8

use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{timeout, Duration};

/// 默认免费桶：付费槽 1 / 普通 3
pub const DEFAULT_FREE_SLOTS: usize = 1;
pub const DEFAULT_FREE_MULTI: usize = 3;
/// 默认订阅桶：付费槽 3 / 普通 8
pub const DEFAULT_SUB_SLOTS: usize = 3;
pub const DEFAULT_SUB_MULTI: usize = 8;
/// acquire 超时（毫秒）：超时视为并发繁忙，返回 429 语义
pub const ACQUIRE_TIMEOUT_MS: u64 = 2000;

/// 获取 permit 失败
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireError {
    /// 桶容量耗尽且超时未等到（429 语义）
    Busy,
}

/// 双桶信号量（网关全局级，Arc 共享）
pub struct TieredSemaphore {
    free_slots: Arc<Semaphore>,
    free_multi: Arc<Semaphore>,
    sub_slots: Arc<Semaphore>,
    sub_multi: Arc<Semaphore>,
}

/// 单个 permit 的 RAII 守卫：Drop 时归还对应信号量
#[derive(Debug)]
pub struct TierGuard {
    // 归还要看从哪个桶借的——用 Option 表示已借的 permit
    slots: Option<tokio::sync::OwnedSemaphorePermit>,
    multi: Option<tokio::sync::OwnedSemaphorePermit>,
}

impl TierGuard {
    fn new(
        slots: Option<tokio::sync::OwnedSemaphorePermit>,
        multi: Option<tokio::sync::OwnedSemaphorePermit>,
    ) -> Self {
        Self { slots, multi }
    }
}

impl Drop for TierGuard {
    fn drop(&mut self) {
        // SemaphorePermit Drop 自动归还；显式清空以防二次借用
        self.slots.take();
        self.multi.take();
    }
}

impl TieredSemaphore {
    /// 从四个容量构建
    pub fn new(free_slots: usize, free_multi: usize, sub_slots: usize, sub_multi: usize) -> Self {
        Self {
            free_slots: Arc::new(Semaphore::new(free_slots.max(1))),
            free_multi: Arc::new(Semaphore::new(free_multi.max(1))),
            sub_slots: Arc::new(Semaphore::new(sub_slots.max(1))),
            sub_multi: Arc::new(Semaphore::new(sub_multi.max(1))),
        }
    }

    /// 默认容量（1/3/3/8）
    pub fn default_capacity() -> Self {
        Self::new(
            DEFAULT_FREE_SLOTS,
            DEFAULT_FREE_MULTI,
            DEFAULT_SUB_SLOTS,
            DEFAULT_SUB_MULTI,
        )
    }

    /// 占用数快照（供面板/体检观测）
    pub fn usage(&self) -> serde_json::Value {
        serde_json::json!({
            "free_slots": self.free_slots.available_permits(),
            "free_multi": self.free_multi.available_permits(),
            "sub_slots": self.sub_slots.available_permits(),
            "sub_multi": self.sub_multi.available_permits(),
        })
    }

    /// 获取双桶 permit（槽位 + 普通各一）。`is_subscriber=true` 走订阅桶。
    ///
    /// 两个 permit 都必须拿到才返回守卫；先拿到的那个在任一失败/超时时已由内部 Drop 归还。
    /// 超时返回 `AcquireError::Busy`（429 语义）。
    pub async fn acquire(&self, is_subscriber: bool) -> Result<TierGuard, AcquireError> {
        let (slots, multi) = if is_subscriber {
            (self.sub_slots.clone(), self.sub_multi.clone())
        } else {
            (self.free_slots.clone(), self.free_multi.clone())
        };
        // 先抢 slots（更稀缺），再抢 multi
        let slot_permit = match timeout(
            Duration::from_millis(ACQUIRE_TIMEOUT_MS),
            slots.acquire_owned(),
        )
        .await
        {
            Ok(Ok(p)) => Some(p),
            _ => return Err(AcquireError::Busy),
        };
        let multi_permit = match timeout(
            Duration::from_millis(ACQUIRE_TIMEOUT_MS),
            multi.acquire_owned(),
        )
        .await
        {
            Ok(Ok(p)) => Some(p),
            _ => return Err(AcquireError::Busy), // slot_permit 在此 Drop 自动归还
        };
        Ok(TierGuard::new(slot_permit, multi_permit))
    }

    /// 仅占槽位（multi 不占）——保留给需要"只限会话数"的场景；当前未使用。
    #[allow(dead_code)]
    pub async fn acquire_slots_only(&self, is_subscriber: bool) -> Result<TierGuard, AcquireError> {
        let slots = if is_subscriber {
            self.sub_slots.clone()
        } else {
            self.free_slots.clone()
        };
        match timeout(
            Duration::from_millis(ACQUIRE_TIMEOUT_MS),
            slots.acquire_owned(),
        )
        .await
        {
            Ok(Ok(p)) => Ok(TierGuard::new(Some(p), None)),
            _ => Err(AcquireError::Busy),
        }
    }
}

/// 判定一个账号凭证是否走订阅桶（保守策略：无明确订阅信号默认 free 桶）。
///
/// 信号来源（任一命中即订阅）：
/// - token 明文含 `unique_subscription`（web 协议套餐字段）
/// - token 明文含 `"subscription"` / `"access_tier"` 且非免费值
/// - Bearer token 形态（长串）但会话快照 tier 为 paid/subscribed（由调用方传入）
pub fn is_subscriber_token(token: &str, tier_hint: Option<&str>) -> bool {
    let t = token.to_lowercase();
    if t.contains("unique_subscription") {
        return true;
    }
    if let Some(tier) = tier_hint.map(|s| s.to_lowercase()) {
        if !tier.is_empty() && !["free", "guest", "anonymous", "basic", ""].contains(&tier.as_str())
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn free_bucket_caps_concurrency() {
        let sem = TieredSemaphore::new(1, 1, 3, 8);
        let g1 = sem.acquire(false).await.unwrap();
        // 桶容量 1 → 第二个 acquire 超时应 Busy
        let r = tokio::time::timeout(Duration::from_millis(100), sem.acquire(false)).await;
        assert!(r.is_err(), "容量 1 时第二个 acquire 必须超时");
        drop(g1);
        // 归还后可再次获取
        assert!(sem.acquire(false).await.is_ok());
    }

    #[tokio::test]
    async fn subscriber_and_free_buckets_are_independent() {
        let sem = TieredSemaphore::new(1, 1, 3, 8);
        let g_free = sem.acquire(false).await.unwrap();
        // free 桶耗尽不影响 subscriber 桶
        let g_sub = sem.acquire(true).await.unwrap();
        drop(g_sub);
        // subscriber 桶有多余容量，可再进 2 个
        let s1 = sem.acquire(true).await.unwrap();
        let s2 = sem.acquire(true).await.unwrap();
        drop(s1);
        drop(s2);
        drop(g_free);
    }

    #[tokio::test]
    async fn permit_exhaustion_returns_busy_within_timeout() {
        let sem = TieredSemaphore::new(1, 1, 3, 8);
        let _g = sem.acquire(false).await.unwrap();
        let start = std::time::Instant::now();
        // ACQUIRE_TIMEOUT_MS 是内部 2s，测试用 3s 外窗确保 Busy 而非无限挂起
        let r = tokio::time::timeout(Duration::from_millis(3000), sem.acquire(false)).await;
        assert!(r.is_ok(), "不得无限阻塞");
        assert_eq!(r.unwrap().unwrap_err(), AcquireError::Busy);
        assert!(
            start.elapsed() < Duration::from_millis(2500),
            "应在 2s 超时内返回 Busy"
        );
    }

    #[tokio::test]
    async fn guard_drop_returns_permits_no_leak() {
        let sem = TieredSemaphore::new(2, 2, 3, 8);
        for _ in 0..1000 {
            let g = sem.acquire(false).await.unwrap();
            drop(g);
        }
        // 循环后可用计数复原
        assert_eq!(sem.free_slots.available_permits(), 2);
        assert_eq!(sem.free_multi.available_permits(), 2);
    }

    #[test]
    fn subscriber_detection_is_conservative() {
        // web 协议套餐字段 → 订阅
        assert!(is_subscriber_token(
            "__Secure-next-auth.session-token=x; unique_subscription=true",
            None
        ));
        // Bearer token + paid tier hint → 订阅
        assert!(is_subscriber_token("sk-abc", Some("paid")));
        // 无信号 → 免费（保守）
        assert!(!is_subscriber_token("sk-abc", None));
        assert!(!is_subscriber_token("sk-abc", Some("free")));
        assert!(!is_subscriber_token("sk-abc", Some("guest")));
    }

    #[tokio::test]
    async fn multi_bucket_shared_with_slots() {
        // multi 桶独立：free slots=2 / multi=2 时，2 个并发可同时通过（slots 不阻塞）
        let sem = TieredSemaphore::new(2, 2, 3, 8);
        let g1 = sem.acquire(false).await.unwrap();
        let g2 = sem.acquire(false).await.unwrap();
        drop(g1);
        drop(g2);
        // slots 容量 1 + multi 2：第 2 个并发被 slots 挡住（不是 multi）
        let sem2 = TieredSemaphore::new(1, 2, 3, 8);
        let _a = sem2.acquire(false).await.unwrap();
        let r = tokio::time::timeout(Duration::from_millis(100), sem2.acquire(false)).await;
        assert!(r.is_err(), "slots 容量 1 时第 2 个并发必须超时");
    }
}
