//! 模型元数据契约集成测试（v0.9）：元数据表 / efforts 阶梯 / 可用性回落
use freebuff2api::models::{ModelRegistry, HARDCODED_MODELS};
use freebuff2api::router::{ModelRouter, RouterConfig};
use std::sync::Arc;

fn router() -> ModelRouter {
    ModelRouter::new(Arc::new(ModelRegistry::new()), RouterConfig::default())
}

#[test]
fn meta_snapshot_covers_hardcoded_models() {
    let reg = ModelRegistry::new();
    let snap = reg.meta_snapshot();
    for m in HARDCODED_MODELS {
        assert!(snap.iter().any(|x| x.id == *m), "缺元数据：{m}");
    }
}

#[test]
fn clamp_effort_aligned_with_upstream_ladders() {
    let r = router();
    // GLM 5.3：当前上游阶梯 low/high/max（GLM_V53_FLASH_REASONING_EFFORTS），max 原样保留
    assert_eq!(
        r.clamp_effort("z-ai/glm-5.3-flash", "max").as_deref(),
        Some("max")
    );
    assert_eq!(
        r.clamp_effort("z-ai/glm-5.3-flash", "high").as_deref(),
        Some("high")
    );
    assert_eq!(
        r.clamp_effort("z-ai/glm-5.3-flash", "low").as_deref(),
        Some("low")
    );
    assert_eq!(
        r.clamp_effort("z-ai/glm-5.3-flash", "xhigh").as_deref(),
        Some("max")
    );
    // deepseek 保持 low/high/max
    assert_eq!(
        r.clamp_effort("deepseek/deepseek-v4-flash", "high")
            .as_deref(),
        Some("high")
    );
    // muse 阶梯 minimal..xhigh，max → xhigh
    assert_eq!(
        r.clamp_effort("meta/muse-spark-1.2-contributor", "max")
            .as_deref(),
        Some("xhigh")
    );
    // 无阶梯模型剥离（solar/minimax/kimi/glm-5.2）
    assert!(r.clamp_effort("upstage/solar-pro4", "max").is_none());
    assert!(r.clamp_effort("minimax/minimax-m3", "high").is_none());
    assert!(r.clamp_effort("crof/kimi-k3-eco", "max").is_none());
    assert!(
        r.clamp_effort("z-ai/glm-5.2", "max").is_none(),
        "glm-5.2 上游无阶梯（忽略 reasoning_effort）"
    );
}

#[test]
fn availability_resolution() {
    let r = router();
    // 可用模型 → None
    assert_eq!(r.resolve_available("z-ai/glm-5.3-flash"), None);
    // 暂停模型 → 回落
    assert_eq!(
        r.resolve_available("google/gemini-3.8-flash").as_deref(),
        Some("google/gemini-3.1-flash-lite")
    );
    assert_eq!(
        r.resolve_available("deepseek/deepseek-v4-pro").as_deref(),
        Some("z-ai/glm-5.3-flash")
    );
    // 未知模型 → None（不误伤）
    assert_eq!(r.resolve_available("unknown/x"), None);
    // 可读原因
    let reason = r.unavailable_reason("stealth/ox-alpha").unwrap();
    assert!(reason.contains("暂停/下架"));
    assert!(reason.contains("z-ai/glm-5.3-flash"));
    assert!(r.unavailable_reason("z-ai/glm-5.3-flash").is_none());
}

#[test]
fn model_available_defaults_true_for_unknown() {
    let r = router();
    assert!(r.model_available("z-ai/glm-5.3-flash"));
    assert!(!r.model_available("stealth/ox-alpha"));
    assert!(
        r.model_available("upstream/dynamic-new-model"),
        "上游动态新增不应被误伤"
    );
}

// —— v0.10 T1.2：目录/契约对齐 ——

#[test]
fn mimo_in_catalog_and_meta() {
    let reg = ModelRegistry::new();
    let m = reg.meta_for("mimo/mimo-v2.5").expect("mimo 应有元数据");
    assert!(m.available);
    assert!(!m.premium);
    assert!(m.multimodal);
    assert!(m.efforts.is_none());
    assert_eq!(m.availability, "always");
}

#[test]
fn fixture_catalog_rows_covered_by_meta() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/freebuff-models.snapshot.json"))
            .expect("fixture 可解析");
    let reg = ModelRegistry::new();
    let models = fixture["models"].as_array().expect("models 数组");
    let mut checked = 0usize;
    for row in models {
        if row["catalog"].as_bool().unwrap_or(false) {
            let id = row["id"].as_str().unwrap();
            let meta = reg
                .meta_for(id)
                .unwrap_or_else(|| panic!("catalog 行缺 meta: {id}"));
            assert_eq!(
                meta.availability,
                row["availability"].as_str().unwrap(),
                "availability 漂移: {id}"
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 18,
        "catalog 行数应 ≥18（含 mimo），实际 {checked}"
    );
}

#[test]
fn vendored_snapshot_refresh_keeps_zero_drift() {
    let reg = ModelRegistry::new();
    let snap = freebuff2api::models::load_local_snapshot().expect("内置快照");
    let (added, updated) = reg.refresh_strategy_from_snapshot(&snap).unwrap();
    assert_eq!(added, 0, "快照不应新增静态表外 catalog 行");
    assert_eq!(updated, 0, "快照与静态表应零漂移");
}
