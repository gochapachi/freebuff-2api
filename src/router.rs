//! 模型路由 + 降级链 + token 节省（9router 式）
//!
//! - 主模型失败自动降级到 fallback_models 中的备选
//! - 按 free 配额/成本选择最优模型
//! - token 节省：压缩超长 tool_result（可选，默认关闭）

use crate::models::ModelRegistry;
use anyhow::Result;
use std::sync::Arc;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RouterConfig {
    /// 冻结的降级链（用户自选）
    pub fallback_chain: Vec<String>,
    /// 配额感知（按账号 rateLimitsByModel 剩余）
    pub quota_aware: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            fallback_chain: vec![
                "z-ai/glm-5.3-flash".into(),
                "google/gemini-3.8-flash".into(),
                "google/gemini-3.1-flash-lite".into(),
                "deepseek/deepseek-v4-flash".into(),
            ],
            quota_aware: true,
        }
    }
}

impl RouterConfig {
    /// 从全局配置构造：用户配置了 fallback_models 时优先使用（此前该配置解析后零消费）
    pub fn from_app_config(cfg: &crate::config::Config) -> Self {
        let mut rc = Self::default();
        if !cfg.fallback_models.is_empty() {
            rc.fallback_chain = cfg.fallback_models.clone();
        }
        rc
    }
}

pub struct ModelRouter {
    pub config: RouterConfig,
    pub registry: Arc<ModelRegistry>,
}

impl ModelRouter {
    pub fn new(registry: Arc<ModelRegistry>, config: RouterConfig) -> Self {
        Self { config, registry }
    }

    /// 解析请求模型 → 实际可用模型（含降级链；时间感知）
    pub async fn resolve(&self, requested: &str) -> String {
        self.resolve_at(requested, chrono::Utc::now()).await
    }

    /// 指定时刻解析：请求模型当时可用才直接选用；否则降级链取第一条当时可用
    pub async fn resolve_at(&self, requested: &str, now: chrono::DateTime<chrono::Utc>) -> String {
        if self.registry.has_model(requested).await
            && self.registry.model_available_at(requested, now)
        {
            return requested.to_string();
        }
        for m in &self.config.fallback_chain {
            if self.registry.has_model(m).await && self.registry.model_available_at(m, now) {
                return m.clone();
            }
        }
        crate::models::DEFAULT_MODEL.to_string()
    }

    /// 检查某模型是否可直接用（杜绝幻觉）
    pub fn is_available(&self, model: &str) -> bool {
        self.registry.models_sync().contains(&model.to_string())
    }

    /// 模型是否支持 reasoning_effort（思考程度），逆向自上游 orchestrator.js efforts 字段
    /// 支持清单（efforts 非空）：deepseek 系/glm 系/gpt-5.6/gemini-3.8/fable-5/ox-alpha/muse-spark
    /// 不支持（无 efforts 字段）：solar-pro4 / minimax-m3 / mimo-v2.5 / kimi-k3
    pub fn supports_reasoning(&self, model: &str) -> bool {
        // 支持 efforts 的模型前缀
        const SUPPORTED: &[&str] = &[
            "deepseek/",
            "z-ai/glm",
            "openai/gpt-5.6",
            "google/gemini-3.8",
            "anthropic/claude-fable",
            "stealth/ox-alpha",
            "meta/muse-spark",
        ];
        SUPPORTED.iter().any(|p| model.starts_with(p))
    }

    /// 模型支持的 efforts 范围；None = 不支持
    ///
    /// v0.9：以元数据权威表（`ModelRegistry` 静态表，对齐上游 freebuff-models.ts）优先；
    /// 表内模型 efforts=None 即不支持（不再按前缀推测）；表外模型（上游动态新增）按前缀回退，
    /// 避免 deepseek/glm 变体丢档位。
    pub fn reasoning_efforts(&self, model: &str) -> Option<Vec<&'static str>> {
        if self.registry.meta_for(model).is_some() {
            return self.registry.efforts_static(model).map(|e| e.to_vec());
        }
        // 表外模型：前缀回退（兼容上游动态新增）
        if model.starts_with("deepseek/")
            || model.starts_with("z-ai/glm")
            || model.starts_with("stealth/ox-alpha")
        {
            Some(vec!["low", "high", "max"])
        } else if model.starts_with("openai/gpt-5.6")
            || model.starts_with("google/gemini-3.8")
            || model.starts_with("anthropic/claude-fable")
        {
            Some(vec!["low", "medium", "high", "xhigh", "max"])
        } else if model.starts_with("meta/muse-spark") {
            Some(vec!["minimal", "low", "medium", "high", "xhigh"])
        } else {
            None
        }
    }

    /// 模型是否可免费使用（元数据；未知模型默认可用，不误伤上游动态新增）
    pub fn model_available(&self, model: &str) -> bool {
        self.registry.model_available(model)
    }

    /// 模型不可用且有回落时返回回落模型；可用 / 未知模型 → None（时间感知）
    pub fn resolve_available(&self, model: &str) -> Option<String> {
        self.resolve_available_at(model, chrono::Utc::now())
    }

    /// 指定时刻：不可用且有回落 → 回落模型；可用 / 未知 → None
    pub fn resolve_available_at(
        &self,
        model: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<String> {
        let meta = self.registry.meta_for_at(model, now)?;
        if meta.available {
            return None;
        }
        meta.fallback
    }

    /// 不可用模型的可读原因（面板展示用）；可用 / 未知模型 → None
    pub fn unavailable_reason(&self, model: &str) -> Option<String> {
        self.unavailable_reason_at(model, chrono::Utc::now())
    }

    /// 指定时刻的可读原因（含 availableAt 文本）
    pub fn unavailable_reason_at(
        &self,
        model: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<String> {
        self.unavailable_detail_at(model, now)
            .map(|(reason, _)| reason)
    }

    /// 指定时刻：不可用原因 + 预计恢复时刻（off_peak_only 窗口内 → Some(ISO)，其余 None）
    pub fn unavailable_detail_at(
        &self,
        model: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<(String, Option<String>)> {
        let meta = self.registry.meta_for_at(model, now)?;
        if meta.available {
            return None;
        }
        let paused = meta.availability != "off_peak_only";
        let mut msg = if paused {
            format!("模型 {model} 已被上游暂停/下架（免费模式不再提供）")
        } else {
            format!("模型 {model} 当前不在可用窗口（上游 DeepSeek 高价窗 00:00–10:00 UTC）")
        };
        if let Some(fb) = &meta.fallback {
            msg.push_str(&format!("；建议改用 {fb}"));
        }
        if let Some(aa) = &meta.available_at {
            msg.push_str(&format!("；预计恢复 {aa}（availableAt）"));
        }
        Some((msg, meta.available_at))
    }

    /// 校正 effort：不支持或超范围时降级到最近支持值
    pub fn clamp_effort(&self, model: &str, requested: &str) -> Option<String> {
        let efforts = self.reasoning_efforts(model)?;
        let requested = requested.to_lowercase();
        if efforts.contains(&requested.as_str()) {
            return Some(requested);
        }
        // 超出范围：取 requests 同档 max → 支持上限
        if requested == "max" || requested == "xhigh" || requested == "high" {
            return Some(efforts.last().unwrap().to_string());
        }
        if requested == "minimal" || requested == "low" {
            return Some(efforts.first().unwrap().to_string());
        }
        Some(efforts.first().unwrap().to_string())
    }
}

/// 超长 tool_result 压缩（token 节省核心）
/// 注意：按字符边界切分——中文字符串上按字节切片会 panic（与 api.rs tail_keep 同类问题）
pub fn compress_tool_result(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        return content.to_string();
    }
    let keep = max_chars / 2;
    // 头部：从 keep 处向前回退到字符边界
    let mut head_end = keep.min(content.len());
    while head_end > 0 && !content.is_char_boundary(head_end) {
        head_end -= 1;
    }
    // 尾部：从 len-keep 处向后推进到字符边界
    let mut tail_start = content.len() - keep;
    while tail_start < content.len() && !content.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    format!(
        "{}\n\n[... 已压缩: 原始 {} 字符，保留首尾 {} 字符 ...]\n\n{}",
        &content[..head_end],
        content.len(),
        max_chars,
        &content[tail_start..]
    )
}

#[allow(dead_code)]
pub fn truncate_to_tokens(s: &str, approx_tokens: usize) -> String {
    // 粗略估算：每 token ≈ 4 字符（英文）
    let max_chars = approx_tokens * 4;
    compress_tool_result(s, max_chars)
}

#[allow(dead_code)]
async fn plausible_model(_registry: &ModelRegistry, _name: &str) -> Result<bool> {
    Ok(true)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ModelRegistry;
    use std::sync::Arc;

    fn router() -> ModelRouter {
        ModelRouter::new(Arc::new(ModelRegistry::new()), RouterConfig::default())
    }

    #[test]
    fn clamp_effort_within_range_kept() {
        let r = router();
        // glm 支持 low/high/max
        assert_eq!(
            r.clamp_effort("z-ai/glm-5.3-flash", "high").as_deref(),
            Some("high")
        );
        assert_eq!(
            r.clamp_effort("z-ai/glm-5.3-flash", "low").as_deref(),
            Some("low")
        );
    }

    #[test]
    fn clamp_effort_over_range_downgrades() {
        let r = router();
        // glm 上限是 max，请求 xhigh → 取上限 max；gpt 支持到 max
        assert_eq!(
            r.clamp_effort("z-ai/glm-5.3-flash", "xhigh").as_deref(),
            Some("max")
        );
        // muse 上限 xhigh，请求 max → xhigh
        assert_eq!(
            r.clamp_effort("meta/muse-spark-x", "max").as_deref(),
            Some("xhigh")
        );
        // muse 下限 minimal，请求 low 有效但 minimal 是首项
        assert_eq!(
            r.clamp_effort("meta/muse-spark-x", "minimal").as_deref(),
            Some("minimal")
        );
    }

    #[test]
    fn clamp_effort_unsupported_model_returns_none() {
        let r = router();
        // solar/minimax/mimo/kimi 不支持 effort
        assert!(r.clamp_effort("upstage/solar-pro4", "max").is_none());
        assert!(r.clamp_effort("minimax/minimax-m3", "high").is_none());
    }

    #[test]
    fn clamp_effort_unknown_value_falls_back() {
        let r = router();
        // 未知档位 → 取支持列表第一项
        assert_eq!(
            r.clamp_effort("z-ai/glm-5.3-flash", "bogus").as_deref(),
            Some("low")
        );
    }

    #[test]
    fn supports_reasoning_prefixes() {
        let r = router();
        assert!(r.supports_reasoning("deepseek/deepseek-v4-flash"));
        assert!(r.supports_reasoning("z-ai/glm-5.3-flash"));
        assert!(!r.supports_reasoning("upstage/solar-pro4"));
    }

    #[test]
    fn compress_short_content_untouched() {
        let s = "short";
        assert_eq!(compress_tool_result(s, 100), "short");
    }

    #[test]
    fn compress_long_content_keeps_head_tail() {
        let s = "a".repeat(1000);
        let out = compress_tool_result(&s, 100);
        assert!(out.len() < 1000);
        assert!(out.contains("已压缩"));
        assert!(out.starts_with("aaa"));
    }

    #[test]
    fn compress_multibyte_content_does_not_panic() {
        // 回归：中文内容按字节切片会触发 char boundary panic
        let s = "中文内容".repeat(500);
        let out = compress_tool_result(&s, 1000);
        assert!(out.contains("已压缩"));
        assert!(out.starts_with("中文"));
        // emoji（4 字节字符）同样安全
        let e = "🎉".repeat(300);
        let out2 = compress_tool_result(&e, 500);
        assert!(out2.contains("已压缩"));
    }

    #[test]
    fn resolve_at_time_aware_fallback() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let reg = Arc::new(ModelRegistry::new());
            reg.init().await;
            let r = ModelRouter::new(reg.clone(), RouterConfig::default());
            // 暂停模型（gemini-3.8）→ 降级链第一条当时可用（glm-5.3）
            assert_eq!(
                r.resolve("google/gemini-3.8-flash").await,
                "z-ai/glm-5.3-flash"
            );
            // 可用模型原样返回
            assert_eq!(r.resolve("z-ai/glm-5.3-flash").await, "z-ai/glm-5.3-flash");
            // 未知模型 → 默认
            assert_eq!(
                r.resolve("no/such-model").await,
                crate::models::DEFAULT_MODEL
            );
        });
    }

    #[test]
    fn resolve_available_at_window_fallback() {
        let reg = ModelRegistry::new();
        // 快照覆盖：deepseek-v4-flash → off_peak_only（fallback 保留 gpt-5.6-luna）
        let snap = r#"{"_source":"t","_vended_at":"2026-09-19","models":[{"id":"deepseek/deepseek-v4-flash","availability":"off_peak_only","catalog":true}]}"#;
        reg.refresh_strategy_from_snapshot(snap).unwrap();
        let r = ModelRouter::new(Arc::new(reg), RouterConfig::default());
        let in_win = chrono::DateTime::parse_from_rfc3339("2026-09-16T03:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let out_win = chrono::DateTime::parse_from_rfc3339("2026-09-16T15:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            r.resolve_available_at("deepseek/deepseek-v4-flash", in_win)
                .as_deref(),
            Some("openai/gpt-5.6-luna")
        );
        assert_eq!(
            r.resolve_available_at("deepseek/deepseek-v4-flash", out_win),
            None
        );
        // 未知模型不抖动
        assert_eq!(r.resolve_available("no/such-model"), None);
    }

    #[test]
    fn unavailable_reason_at_has_parseable_available_at() {
        let reg = ModelRegistry::new();
        let snap = r#"{"_source":"t","_vended_at":"2026-09-19","models":[{"id":"deepseek/deepseek-v4-flash","availability":"off_peak_only","catalog":true}]}"#;
        reg.refresh_strategy_from_snapshot(snap).unwrap();
        let r = ModelRouter::new(Arc::new(reg), RouterConfig::default());
        let in_win = chrono::DateTime::parse_from_rfc3339("2026-09-16T03:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let (reason, aa) = r
            .unavailable_detail_at("deepseek/deepseek-v4-flash", in_win)
            .expect("窗口内应给原因");
        assert!(reason.contains("窗口"), "窗口文案: {reason}");
        let iso = aa.expect("窗口内应有 availableAt");
        assert!(
            chrono::DateTime::parse_from_rfc3339(&iso).is_ok(),
            "ISO 可解析"
        );
        assert!(reason.contains("availableAt"));
    }
}
