//! WebCookiePool 集成测试（v0.9 §1.1）：从真实 Config/tokens.json 构建 + 契约字段 + 并发稳定。
//!
//! 单元级（挑选/熔断/冷却/恢复/空池 ≥8）在 src/web_pool.rs 内；
//! 本文件覆盖"从磁盘构建""健康快照契约字段""并发挑选稳定"等跨模块行为。

use freebuff2api::config::Config;
use freebuff2api::web_pool::WebCookiePool;
use std::time::Duration;

fn cookie_a() -> String {
    "__Secure-next-auth.session-token=aaaa; __Host-next-auth.csrf-token=zzz".to_string()
}
fn cookie_b() -> String {
    "__Secure-next-auth.session-token=bbbb; __Host-next-auth.csrf-token=zzz".to_string()
}

/// 组装一个带 tokens_path 的临时 Config（auth_tokens 含一个 Cookie，导入库含一个 web-cookie）
fn tmp_cfg(cfg_auth_token: &str, tokens_json: &str) -> (Config, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let tokens_path = dir.path().join("tokens.json");
    std::fs::write(&tokens_path, tokens_json).unwrap();
    let cfg = Config {
        listen_addr: "127.0.0.1:47821".into(),
        auth_tokens: vec![cfg_auth_token.to_string()],
        tokens_path: tokens_path.to_str().unwrap().to_string(),
        skip_upstream_check: true,
        ..Default::default()
    };
    (cfg, dir)
}

#[tokio::test]
async fn builds_from_config_cookie_and_imported_web_cookie() {
    // config.auth_tokens 里的 Cookie 项 + tokens.json 的 web-cookie 都要入池
    let tokens_json = format!(
        r#"[{{"token":"{b}","source":"cookie","host":"freebuff.com","path":"/p","method":"GET","added_at":"2026-09-01T00:00:00Z"}}]"#,
        b = cookie_b()
    );
    let (cfg, _dir) = tmp_cfg(&cookie_a(), &tokens_json);
    let pool = WebCookiePool::new(&cfg);
    let snap = pool.snapshot().await;
    assert_eq!(snap.len(), 2, "config 与导入库凭证都应入池");
    let sources: Vec<&str> = snap.iter().map(|s| s.source).collect();
    assert!(sources.contains(&"config"), "config 项 source=config");
    assert!(sources.contains(&"imported"), "导入项 source=imported");
}

#[tokio::test]
async fn ignores_bearer_tokens_in_config_and_import() {
    // Bearer token（无 session-token）不得进 web Cookie 池
    let tokens_json = r#"[{"token":"sk-bearer-abc123","source":"curl","host":"h","path":"p","method":"POST","added_at":"2026-09-01T00:00:00Z"}]"#;
    let (cfg, _dir) = tmp_cfg("sk-plain-bearer", tokens_json);
    let pool = WebCookiePool::new(&cfg);
    assert!(
        pool.pick().await.is_none(),
        "无 web Cookie → pick 应为 None"
    );
}

#[tokio::test]
async fn snapshot_contract_fields_for_health_endpoint() {
    // /api/accounts/health 的 web 侧契约：字段名与类型不可变（前端按此画时间线）
    let (cfg, _dir) = tmp_cfg(&cookie_a(), "[]");
    let pool = WebCookiePool::new(&cfg);
    let id = pool.pick().await.unwrap().id;
    pool.mark_cooldown(
        &id,
        Duration::from_secs(600),
        "web chat HTTP 401 Unauthorized",
    )
    .await;
    let snap = pool.snapshot().await;
    let s = &snap[0];
    assert_eq!(s.kind, "web-cookie");
    assert_eq!(s.id, id);
    assert!(!s.masked.is_empty());
    assert_eq!(s.circuit_state, "open");
    assert!(s.cooldown_until.is_some(), "熔断后应有 cooldown_until");
    assert_eq!(s.trips, 1);
    assert!(s.last_error.as_deref().is_some());
    // last_ok_at 初始为 None（从未成功）
    assert!(s.last_ok_at.is_none());
    // added_at 字段必须存在（契约），config 项可为 null（导入项才有真实入库时间）
    assert!(
        serde_json::to_value(s).unwrap().get("added_at").is_some(),
        "added_at 字段必须存在"
    );
    let v = serde_json::to_value(s).unwrap();
    for field in [
        "id",
        "kind",
        "source",
        "added_at",
        "masked",
        "health_score",
        "circuit_state",
        "cooldown_until",
        "trips",
        "last_error",
        "last_ok_at",
    ] {
        assert!(v.get(field).is_some(), "契约字段 {field} 缺失");
    }
}

#[tokio::test]
async fn concurrent_picks_never_panic_and_respect_cooldown() {
    let (cfg, _dir) = tmp_cfg(
        &cookie_a(),
        &format!(
            r#"[{{"token":"{b}","source":"cookie","host":"h","path":"p","method":"GET","added_at":"2026-09-01T00:00:00Z"}}]"#,
            b = cookie_b()
        ),
    );
    let pool = WebCookiePool::new(&cfg);
    // 坏号 401：立即熔断冷却；并发挑选必须稳定落在好号（无 panic / 无 None）
    let idb = freebuff2api::import::cred_id(&cookie_b());
    pool.mark_cooldown(&idb, Duration::from_secs(600), "web chat HTTP 401")
        .await;
    let pool = std::sync::Arc::new(pool);
    let mut handles = Vec::new();
    for _ in 0..16 {
        let p = pool.clone();
        handles.push(tokio::spawn(async move { p.pick().await }));
    }
    for h in handles {
        let picked = h.await.unwrap();
        assert!(picked.is_some(), "好号存在时并发 pick 不得返回 None");
        let picked = picked.unwrap();
        assert_eq!(picked.cookie, cookie_a(), "坏号熔断后并发挑选应全部落好号");
    }
}

#[tokio::test]
async fn mark_ok_sets_last_ok_at() {
    let (cfg, _dir) = tmp_cfg(&cookie_a(), "[]");
    let pool = WebCookiePool::new(&cfg);
    let id = pool.pick().await.unwrap().id;
    pool.mark_cooldown(&id, Duration::from_millis(1), "boom")
        .await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    // 冷却到期 → HalfOpen 探测放行 → 连续两次成功恢复 Closed
    let p1 = pool.pick().await.unwrap();
    pool.mark_ok(&p1.id).await;
    pool.mark_ok(&p1.id).await;
    let snap = pool.snapshot().await;
    let s = snap.iter().find(|x| x.id == id).unwrap();
    assert!(s.last_ok_at.is_some(), "成功后应记录 last_ok_at");
    assert_eq!(s.circuit_state, "closed", "连续成功应恢复 Closed");
}
