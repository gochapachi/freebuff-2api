//! 模型注册表：上游 free-agents.ts 拉取 + 硬编码权威底座
//!
//! 逆向自上游（Freebuff-0.0.98 orchestrator.js）：
//! - SUPPORTED_FREEBUFF_MODELS 清单
//! - 默认免费模型 z-ai/glm-5.3-flash
//! - 每账号 rateLimitsByModel 决定实际可用

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 硬编码权威底座（上游实测可用）
pub const ROOT_AGENT_ID: &str = "base2-free";

pub const HARDCODED_MODELS: &[&str] = &[
    "z-ai/glm-5.3-flash",
    "google/gemini-3.8-flash",
    "google/gemini-3.1-flash-lite",
    "google/gemini-3.5-flash-lite",
    "deepseek/deepseek-v4-flash",
    "deepseek/deepseek-v4-flash-max",
    "deepseek/deepseek-v4-pro",
    "deepseek/deepseek-v4-pro-max",
    "minimax/minimax-m3",
    "openai/gpt-5.6-luna",
    "openai/gpt-5.6-luna-es",
    "openai/gpt-5.6-luna-max",
    "upstage/solar-pro4",
    "meta/muse-spark-1.2-contributor",
    "meta/muse-spark-1.3-contributor",
    "anthropic/claude-fable-5",
    "stealth/ox-alpha",
    "crof/kimi-k3-eco",
    "z-ai/glm-5.2",
];

/// 子代理 agent 映射（run 层级）
pub const SUB_AGENTS: &[(&str, &str)] = &[
    ("file-picker", "google/gemini-3.1-flash-lite"),
    ("researcher-web", "google/gemini-3.8-flash"),
    ("basher", "google/gemini-3.8-flash"),
    ("browser-use", "google/gemini-3.8-flash"),
];

/// 默认模型
pub const DEFAULT_MODEL: &str = "z-ai/glm-5.3-flash";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub agent: String,
    pub premium: bool,
}

#[derive(Debug, Clone)]
pub struct ModelRegistry {
    inner: Arc<RwLock<RegistryInner>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RegistryInner {
    /// model -> agent
    model_to_agent: HashMap<String, String>,
    /// agent -> models
    agent_models: HashMap<String, Vec<String>>,
    all_models: Vec<String>,
    updated_at: Option<String>,
}

/// 模型元数据（权威静态表，对齐上游 freebuff-models.ts 当前快照）
///
/// 字段来源：
/// - `available`：上游 `FREEBUFF_PAUSED_FREE_MODEL_IDS`（免费模式已暂停/下架 = false）
/// - `efforts`：上游每模型的 reasoningEffort 阶梯（None = 不支持思考档位，调用方应剥离字段）
/// - `multimodal`：上游每模型的 multimodal 标志（仅作面板图片上传可用性提示，非强制）
/// - `fallback`：上游 unavailableFallback，无则取我方产品选择（注释标注）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMeta {
    pub id: String,
    pub agent: String,
    pub premium: bool,
    pub multimodal: bool,
    /// 是否可被免费模式使用
    pub available: bool,
    /// 支持的 reasoning_effort 阶梯（None = 不支持）
    pub efforts: Option<Vec<String>>,
    /// 不可用时的回落模型
    pub fallback: Option<String>,
}

struct MetaRow {
    id: &'static str,
    agent: &'static str,
    premium: bool,
    multimodal: bool,
    available: bool,
    efforts: Option<&'static [&'static str]>,
    fallback: Option<&'static str>,
}

const EFFORTS_GLM: &[&str] = &["low", "high", "max"];
const EFFORTS_FULL: &[&str] = &["low", "medium", "high", "xhigh", "max"];
const EFFORTS_MUSE: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];

/// 静态权威元数据表（覆盖 HARDCODED_MODELS 全部条目）
const MODEL_META_ROWS: &[MetaRow] = &[
    // —— 免费模式可用 ——
    MetaRow {
        id: "z-ai/glm-5.3-flash",
        agent: ROOT_AGENT_ID,
        premium: false,
        multimodal: true,
        available: true,
        efforts: Some(EFFORTS_GLM),
        fallback: None,
    },
    MetaRow {
        id: "google/gemini-3.1-flash-lite",
        agent: "file-picker",
        premium: false,
        multimodal: true,
        available: true,
        efforts: Some(EFFORTS_FULL),
        fallback: None,
    },
    MetaRow {
        id: "google/gemini-3.5-flash-lite",
        agent: ROOT_AGENT_ID,
        premium: false,
        multimodal: true,
        available: true,
        efforts: Some(EFFORTS_FULL),
        fallback: None,
    },
    MetaRow {
        id: "deepseek/deepseek-v4-flash",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        efforts: Some(EFFORTS_GLM),
        fallback: Some("openai/gpt-5.6-luna"),
    },
    MetaRow {
        id: "deepseek/deepseek-v4-flash-max",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        efforts: Some(EFFORTS_GLM),
        fallback: None,
    },
    MetaRow {
        id: "deepseek/deepseek-v4-pro-max",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        efforts: Some(EFFORTS_GLM),
        fallback: None,
    },
    MetaRow {
        id: "openai/gpt-5.6-luna",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: true,
        available: true,
        efforts: Some(EFFORTS_FULL),
        fallback: None,
    },
    MetaRow {
        id: "openai/gpt-5.6-luna-es",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        efforts: Some(EFFORTS_FULL),
        fallback: None,
    },
    MetaRow {
        id: "openai/gpt-5.6-luna-max",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        efforts: Some(EFFORTS_FULL),
        fallback: None,
    },
    MetaRow {
        id: "upstage/solar-pro4",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        efforts: None,
        fallback: None,
    },
    MetaRow {
        id: "meta/muse-spark-1.2-contributor",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        efforts: Some(EFFORTS_MUSE),
        fallback: Some("deepseek/deepseek-v4-flash"),
    },
    MetaRow {
        id: "anthropic/claude-fable-5",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        efforts: Some(EFFORTS_FULL),
        fallback: None,
    },
    MetaRow {
        id: "crof/kimi-k3-eco",
        agent: ROOT_AGENT_ID,
        premium: false,
        multimodal: false,
        available: true,
        efforts: None,
        fallback: None,
    },
    // —— 上游已暂停/下架（FREEBUFF_PAUSED_FREE_MODEL_IDS），保留识别能力并给回落 ——
    MetaRow {
        id: "google/gemini-3.8-flash",
        agent: "researcher-web",
        premium: true,
        multimodal: true,
        available: false,
        efforts: Some(EFFORTS_FULL),
        fallback: Some("google/gemini-3.1-flash-lite"),
    },
    MetaRow {
        id: "deepseek/deepseek-v4-pro",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: false,
        efforts: Some(EFFORTS_GLM),
        fallback: Some("z-ai/glm-5.3-flash"),
    },
    MetaRow {
        id: "minimax/minimax-m3",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: true,
        available: false,
        efforts: None,
        fallback: Some("z-ai/glm-5.3-flash"),
    },
    MetaRow {
        id: "meta/muse-spark-1.3-contributor",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: false,
        efforts: Some(EFFORTS_MUSE),
        fallback: Some("deepseek/deepseek-v4-flash"),
    },
    MetaRow {
        id: "stealth/ox-alpha",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: false,
        efforts: Some(EFFORTS_GLM),
        fallback: Some("z-ai/glm-5.3-flash"),
    },
    MetaRow {
        id: "z-ai/glm-5.2",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: true,
        available: false,
        efforts: None,
        fallback: Some("z-ai/glm-5.3-flash"),
    },
];

fn meta_row_to_meta(r: &MetaRow) -> ModelMeta {
    ModelMeta {
        id: r.id.to_string(),
        agent: r.agent.to_string(),
        premium: r.premium,
        multimodal: r.multimodal,
        available: r.available,
        efforts: r.efforts.map(|e| e.iter().map(|s| s.to_string()).collect()),
        fallback: r.fallback.map(|s| s.to_string()),
    }
}

impl ModelRegistry {
    pub fn new() -> Self {
        let inner = RegistryInner::default();
        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    /// 以硬编码为底座初始化
    pub async fn init(&self) {
        let mut inner = self.inner.write().await;
        for m in HARDCODED_MODELS {
            inner
                .model_to_agent
                .entry(m.to_string())
                .or_insert_with(|| ROOT_AGENT_ID.to_string());
        }
        inner
            .agent_models
            .entry(ROOT_AGENT_ID.to_string())
            .or_insert_with(|| HARDCODED_MODELS.iter().map(|s| s.to_string()).collect());
        for (agent, model) in SUB_AGENTS {
            inner
                .model_to_agent
                .entry(model.to_string())
                .or_insert_with(|| agent.to_string());
            inner
                .agent_models
                .entry(agent.to_string())
                .or_insert_with(|| vec![model.to_string()]);
        }
        let mut all: Vec<String> = inner.model_to_agent.keys().cloned().collect();
        all.sort();
        inner.all_models = all;
        inner.updated_at = Some(now_iso());
    }

    /// 拉取上游 free-agents.ts 增量补充
    pub async fn refresh_from_upstream(
        &self,
        client: &reqwest::Client,
    ) -> Result<(usize, usize), anyhow::Error> {
        const SRC: &str = "https://raw.githubusercontent.com/CodebuffAI/codebuff/main/common/src/constants/free-agents.ts";
        let resp = client.get(SRC).send().await?;
        if !resp.status().is_success() {
            return Ok((0, 0));
        }
        let text = resp.text().await?;
        let parsed = parse_free_agents(&text);
        if parsed.is_empty() {
            return Ok((0, 0));
        }
        let mut inner = self.inner.write().await;
        let mut added = 0;
        let mut removed = 0;
        // 合并硬编码底座（权威）+ 上游最新解析
        let mut merged = hardcoded_fallback_map();
        for (agent, models) in parsed {
            merged.entry(agent).or_default().extend(models);
        }
        // 重建 model→agent：以上游为准，不在上游也不在硬编码的移除
        let mut new_model_to_agent = std::collections::HashMap::new();
        for (agent, models) in &merged {
            for model in models {
                if !new_model_to_agent.contains_key(model) {
                    new_model_to_agent.insert(model.clone(), agent.clone());
                }
            }
        }
        // 计算增/删
        for m in new_model_to_agent.keys() {
            if !inner.model_to_agent.contains_key(m) {
                added += 1;
            }
        }
        for m in inner.model_to_agent.keys() {
            if !new_model_to_agent.contains_key(m) {
                removed += 1;
                tracing::warn!("模型 {m} 已从上游移除，同时不在硬编码底座，从注册表下架");
            }
        }
        inner.model_to_agent = new_model_to_agent;
        inner.agent_models = merged;
        let mut all: Vec<String> = inner.model_to_agent.keys().cloned().collect();
        all.sort();
        inner.all_models = all;
        inner.updated_at = Some(now_iso());
        Ok((added, removed))
    }

    pub async fn has_model(&self, model: &str) -> bool {
        self.inner.read().await.model_to_agent.contains_key(model)
    }

    pub async fn agent_for(&self, model: &str) -> Option<String> {
        self.inner.read().await.model_to_agent.get(model).cloned()
    }

    pub async fn models(&self) -> Vec<String> {
        self.inner.read().await.all_models.clone()
    }

    /// 同步读取（供非 async 场景，如路由解析）
    pub fn models_sync(&self) -> Vec<String> {
        // fallback：从硬编码清单取
        HARDCODED_MODELS.iter().map(|s| s.to_string()).collect()
    }

    /// 同步读模型元数据快照（静态权威表，不依赖 RwLock；供 /v1/models 消费）
    pub fn meta_snapshot(&self) -> Vec<ModelMeta> {
        MODEL_META_ROWS.iter().map(meta_row_to_meta).collect()
    }

    /// 同步读单个模型元数据（未知模型返回 None）
    pub fn meta_for(&self, id: &str) -> Option<ModelMeta> {
        MODEL_META_ROWS
            .iter()
            .find(|r| r.id == id)
            .map(meta_row_to_meta)
    }

    /// 同步读模型 efforts 阶梯（'static 切片，供同步路由场景使用）
    pub fn efforts_static(&self, id: &str) -> Option<&'static [&'static str]> {
        MODEL_META_ROWS
            .iter()
            .find(|r| r.id == id)
            .and_then(|r| r.efforts)
    }

    /// 模型是否可免费使用（静态权威表；未知模型默认可用，不误伤上游动态新增）
    pub fn model_available(&self, id: &str) -> bool {
        self.meta_for(id).map(|m| m.available).unwrap_or(true)
    }

    pub async fn snapshot(&self) -> ModelRegistrySnapshot {
        let inner = self.inner.read().await;
        ModelRegistrySnapshot {
            model_count: inner.model_to_agent.len(),
            agent_count: inner.agent_models.len(),
            all_models: inner.all_models.clone(),
            updated_at: inner.updated_at.clone(),
        }
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRegistrySnapshot {
    pub model_count: usize,
    pub agent_count: usize,
    pub all_models: Vec<String>,
    pub updated_at: Option<String>,
}

/// 硬编码底座 map（权威不随上游消失）
fn hardcoded_fallback_map() -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    map.insert(
        ROOT_AGENT_ID.to_string(),
        HARDCODED_MODELS.iter().map(|s| s.to_string()).collect(),
    );
    for (agent, model) in SUB_AGENTS {
        map.entry(agent.to_string())
            .or_default()
            .push(model.to_string());
    }
    map
}

/// 解析上游 free-agents.ts 的 agent→models 映射
pub fn parse_free_agents(source: &str) -> HashMap<String, Vec<String>> {
    // 支持三种形态：new Set([...]) / 数组 [...] / 常量引用（无法解析，跳过）
    let block = Regex::new(r"'([^']+)':\s*(?:new\s+Set\(\s*)?\[([^\]]*)\]").unwrap();
    let model = Regex::new(r"'([^']+)'").unwrap();
    let mut result = HashMap::new();
    for cap in block.captures_iter(source) {
        let agent = cap[1].to_string();
        let models_str = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let models: Vec<String> = model
            .captures_iter(models_str)
            .map(|m| m[1].to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !models.is_empty() {
            result.insert(agent, models);
        }
    }
    result
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_table_covers_all_hardcoded_models() {
        let meta = MODEL_META_ROWS.iter().map(|r| r.id).collect::<Vec<_>>();
        for m in HARDCODED_MODELS {
            assert!(meta.contains(m), "缺失元数据：{m}");
        }
    }

    #[test]
    fn meta_for_known_and_unknown() {
        let reg = ModelRegistry::new();
        assert!(reg.meta_for("z-ai/glm-5.3-flash").is_some());
        assert!(reg.meta_for("no/such-model").is_none());
        let m = reg.meta_for("z-ai/glm-5.3-flash").unwrap();
        assert!(m.available);
        assert_eq!(
            m.efforts.as_deref(),
            Some(&["low".to_string(), "high".to_string(), "max".to_string()][..])
        );
    }

    #[test]
    fn paused_models_unavailable_only() {
        let reg = ModelRegistry::new();
        let paused = [
            "google/gemini-3.8-flash",
            "deepseek/deepseek-v4-pro",
            "minimax/minimax-m3",
            "meta/muse-spark-1.3-contributor",
            "stealth/ox-alpha",
            "z-ai/glm-5.2",
        ];
        for id in paused {
            assert!(!reg.model_available(id), "{id} 应为不可用");
        }
        for id in [
            "z-ai/glm-5.3-flash",
            "deepseek/deepseek-v4-flash",
            "openai/gpt-5.6-luna",
        ] {
            assert!(reg.model_available(id), "{id} 应可用");
        }
        assert!(reg.model_available("unknown/x"), "未知模型默认可用");
    }

    #[test]
    fn meta_snapshot_serializes_fields() {
        let reg = ModelRegistry::new();
        let snap = reg.meta_snapshot();
        assert_eq!(snap.len(), MODEL_META_ROWS.len());
        let json = serde_json::to_value(&snap).unwrap();
        assert!(json.is_array());
        let first = &json[0];
        for k in [
            "id",
            "agent",
            "premium",
            "multimodal",
            "available",
            "efforts",
            "fallback",
        ] {
            assert!(first.get(k).is_some(), "缺少字段 {k}");
        }
    }
}
