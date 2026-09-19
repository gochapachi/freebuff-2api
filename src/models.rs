//! 模型注册表：上游 free-agents.ts 拉取 + 硬编码权威底座
//!
//! 逆向自上游（Freebuff-0.0.98 orchestrator.js）：
//! - SUPPORTED_FREEBUFF_MODELS 清单
//! - 默认免费模型 z-ai/glm-5.3-flash
//! - 每账号 rateLimitsByModel 决定实际可用

use chrono::{DateTime, Datelike, Timelike, Utc, Weekday};
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
    "mimo/mimo-v2.5",
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

#[derive(Debug)]
pub struct ModelRegistry {
    inner: Arc<RwLock<RegistryInner>>,
    /// 运行时策略覆盖（std Mutex 短临界区；供同步 meta_for/路由消费，避免 async 阻塞）
    overrides: std::sync::Mutex<HashMap<String, MetaOverride>>,
}

impl Clone for ModelRegistry {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            overrides: std::sync::Mutex::new(
                self.overrides.lock().map(|g| g.clone()).unwrap_or_default(),
            ),
        }
    }
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
    /// 上游 availability 策略（always / deployment_hours / off_peak_only）
    pub availability: String,
    /// 不可用时的预计恢复时刻（ISO8601 UTC；deployment_hours/未知不编造 → None）
    pub available_at: Option<String>,
}

struct MetaRow {
    id: &'static str,
    agent: &'static str,
    premium: bool,
    multimodal: bool,
    available: bool,
    availability: &'static str,
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
        availability: "always",
        efforts: Some(EFFORTS_GLM),
        fallback: None,
    },
    MetaRow {
        id: "google/gemini-3.1-flash-lite",
        agent: "file-picker",
        premium: false,
        multimodal: true,
        available: true,
        availability: "always",
        efforts: Some(EFFORTS_FULL),
        fallback: None,
    },
    MetaRow {
        id: "google/gemini-3.5-flash-lite",
        agent: ROOT_AGENT_ID,
        premium: false,
        multimodal: true,
        available: true,
        availability: "always",
        efforts: Some(EFFORTS_FULL),
        fallback: None,
    },
    MetaRow {
        id: "deepseek/deepseek-v4-flash",
        agent: ROOT_AGENT_ID,
        premium: false,
        multimodal: false,
        available: true,
        availability: "always",
        efforts: Some(EFFORTS_GLM),
        fallback: Some("openai/gpt-5.6-luna"),
    },
    MetaRow {
        id: "deepseek/deepseek-v4-flash-max",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        availability: "always",
        efforts: Some(EFFORTS_GLM),
        fallback: None,
    },
    MetaRow {
        id: "deepseek/deepseek-v4-pro-max",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        availability: "always",
        efforts: Some(EFFORTS_GLM),
        fallback: None,
    },
    MetaRow {
        id: "openai/gpt-5.6-luna",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: true,
        available: true,
        availability: "always",
        efforts: Some(EFFORTS_FULL),
        fallback: None,
    },
    MetaRow {
        id: "openai/gpt-5.6-luna-es",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        availability: "always",
        efforts: Some(EFFORTS_FULL),
        fallback: None,
    },
    MetaRow {
        id: "openai/gpt-5.6-luna-max",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        availability: "always",
        efforts: Some(EFFORTS_FULL),
        fallback: None,
    },
    MetaRow {
        id: "upstage/solar-pro4",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        availability: "always",
        efforts: None,
        fallback: None,
    },
    MetaRow {
        id: "meta/muse-spark-1.2-contributor",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        availability: "always",
        efforts: Some(EFFORTS_MUSE),
        fallback: Some("deepseek/deepseek-v4-flash"),
    },
    MetaRow {
        id: "anthropic/claude-fable-5",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: true,
        available: true,
        availability: "always",
        efforts: Some(EFFORTS_FULL),
        fallback: None,
    },
    MetaRow {
        id: "crof/kimi-k3-eco",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: true,
        availability: "always",
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
        availability: "always",
        efforts: Some(EFFORTS_FULL),
        fallback: Some("google/gemini-3.1-flash-lite"),
    },
    MetaRow {
        id: "deepseek/deepseek-v4-pro",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: false,
        availability: "always",
        efforts: Some(EFFORTS_GLM),
        fallback: Some("z-ai/glm-5.3-flash"),
    },
    MetaRow {
        id: "minimax/minimax-m3",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: true,
        available: false,
        availability: "always",
        efforts: None,
        fallback: Some("z-ai/glm-5.3-flash"),
    },
    MetaRow {
        id: "meta/muse-spark-1.3-contributor",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: false,
        availability: "always",
        efforts: Some(EFFORTS_MUSE),
        fallback: Some("deepseek/deepseek-v4-flash"),
    },
    MetaRow {
        id: "stealth/ox-alpha",
        agent: ROOT_AGENT_ID,
        premium: false,
        multimodal: true,
        available: false,
        availability: "always",
        efforts: Some(EFFORTS_GLM),
        fallback: Some("z-ai/glm-5.3-flash"),
    },
    MetaRow {
        id: "z-ai/glm-5.2",
        agent: ROOT_AGENT_ID,
        premium: true,
        multimodal: false,
        available: false,
        availability: "always",
        efforts: None,
        fallback: Some("z-ai/glm-5.3-flash"),
    },
    MetaRow {
        id: "mimo/mimo-v2.5",
        agent: ROOT_AGENT_ID,
        premium: false,
        multimodal: true,
        available: true,
        availability: "always",
        efforts: None,
        fallback: None,
    },
];

/// DeepSeek 高价窗（上游 freebuff-models.ts 注释）：00:00–10:00 UTC，半开区间 [0,10)
pub const DEEPSEEK_EXPENSIVE_WINDOW_UTC: (u32, u32) = (0, 10);

/// 北京时间（UTC+8）周末时 DeepSeek 高价窗不生效（上游注释语义）
fn is_beijing_weekend(now: DateTime<Utc>) -> bool {
    let bj = now + chrono::Duration::hours(8);
    matches!(bj.weekday(), Weekday::Sat | Weekday::Sun)
}

/// 当前是否处于 DeepSeek 高价窗（UTC；北京时间周末豁免）
pub fn is_deepseek_expensive_window(now: DateTime<Utc>) -> bool {
    if is_beijing_weekend(now) {
        return false;
    }
    let h = now.hour();
    h >= DEEPSEEK_EXPENSIVE_WINDOW_UTC.0 && h < DEEPSEEK_EXPENSIVE_WINDOW_UTC.1
}

/// 高价窗结束时刻（同 UTC 日 10:00:00，窗口不跨午夜——与上游注释一致）
pub fn deepseek_expensive_window_ends_at(now: DateTime<Utc>) -> DateTime<Utc> {
    now.date_naive()
        .and_hms_opt(10, 0, 0)
        .map(|d| d.and_utc())
        .unwrap_or(now)
}

/// availability 策略 → 指定时刻窗口内是否可用
///
/// - always → true
/// - off_peak_only → !is_deepseek_expensive_window
/// - deployment_hours → true（上游运维窗口，网关无法精确计算，不据此拒绝、
///   也不编造 availableAt，与上游 freebuffModelUnavailableAt 行为对齐）
/// - 其它 → false（未知策略保守拒绝）
pub fn availability_now(availability: &str, now: DateTime<Utc>) -> bool {
    match availability {
        "always" => true,
        "off_peak_only" => !is_deepseek_expensive_window(now),
        "deployment_hours" => true,
        // 审计 L2：未知策略不过度拒绝（按 always 处理），避免上游新增策略值导致模型被静默禁用
        _ => true,
    }
}

/// 运行时策略覆盖（上游快照同步所得；仅覆盖显式提供的字段，保留既有值）
#[derive(Debug, Clone, Default)]
struct MetaOverride {
    availability: Option<String>,
    premium: Option<bool>,
    multimodal: Option<bool>,
    /// Some(Some(阶梯)) / Some(None)=明确无阶梯 / None=未声明（保留）
    efforts: Option<Option<Vec<String>>>,
    fallback: Option<Option<String>>,
}

/// 上游快照单行（camelCase 对齐 fixture）
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotModelMeta {
    id: String,
    #[serde(default)]
    availability: Option<String>,
    #[serde(default)]
    premium: Option<bool>,
    #[serde(default)]
    multimodal: Option<bool>,
    #[serde(default)]
    efforts: Option<Option<Vec<String>>>,
    #[serde(default)]
    unavailable_fallback: Option<Option<String>>,
    #[serde(default)]
    #[allow(dead_code)] // 快照实验性标记，策略层暂不消费
    experimental: Option<bool>,
    /// false = 网关自管/registry 行（不进策略覆盖）
    #[serde(default)]
    catalog: bool,
}

/// 上游快照文件（_source/_vended_at 为元信息）
#[derive(Debug, serde::Deserialize)]
struct SnapshotFile {
    #[serde(rename = "_source")]
    #[allow(dead_code)]
    source: String,
    #[serde(rename = "_vended_at")]
    #[allow(dead_code)]
    vended_at: String,
    models: Vec<SnapshotModelMeta>,
}

fn override_from_snapshot(r: &SnapshotModelMeta) -> MetaOverride {
    MetaOverride {
        availability: r.availability.clone(),
        premium: r.premium,
        multimodal: r.multimodal,
        efforts: r.efforts.clone(),
        fallback: r.unavailable_fallback.clone(),
    }
}

/// 合并两层覆盖（patch 的 Some 字段覆盖 base）
fn merge_override(base: &MetaOverride, patch: &MetaOverride) -> MetaOverride {
    MetaOverride {
        availability: patch
            .availability
            .clone()
            .or_else(|| base.availability.clone()),
        premium: patch.premium.or(base.premium),
        multimodal: patch.multimodal.or(base.multimodal),
        efforts: patch.efforts.clone().or_else(|| base.efforts.clone()),
        fallback: patch.fallback.clone().or_else(|| base.fallback.clone()),
    }
}

/// 静态行 + 运行时覆盖 + 指定时刻 → 最终元数据
fn merge_into_meta(
    id: &str,
    static_row: Option<&MetaRow>,
    over: Option<&MetaOverride>,
    now: DateTime<Utc>,
) -> Option<ModelMeta> {
    let s = static_row;
    let o = over;
    let availability = o
        .and_then(|x| x.availability.clone())
        .or_else(|| s.map(|r| r.availability.to_string()))
        .unwrap_or_else(|| "always".to_string());
    let premium = o
        .and_then(|x| x.premium)
        .or_else(|| s.map(|r| r.premium))
        .unwrap_or(false);
    let multimodal = o
        .and_then(|x| x.multimodal)
        .or_else(|| s.map(|r| r.multimodal))
        .unwrap_or(false);
    let efforts: Option<Vec<String>> = match o.and_then(|x| x.efforts.clone()) {
        Some(inner) => inner,
        None => s
            .and_then(|r| r.efforts)
            .map(|e| e.iter().map(|x| x.to_string()).collect()),
    };
    let fallback: Option<String> = match o.and_then(|x| x.fallback.clone()) {
        Some(inner) => inner,
        None => s.and_then(|r| r.fallback).map(|x| x.to_string()),
    };
    let static_available = s.map(|r| r.available).unwrap_or(true);
    let window_ok = availability_now(&availability, now);
    let available = static_available && window_ok;
    // 审计 M1：仅当"不可用由时间窗导致"（非静态暂停）时才给 availableAt，暂停模型不编造恢复时刻
    let available_at = if static_available && !window_ok && availability == "off_peak_only" {
        Some(deepseek_expensive_window_ends_at(now).to_rfc3339())
    } else {
        None
    };
    Some(ModelMeta {
        id: id.to_string(),
        agent: s
            .map(|r| r.agent.to_string())
            .unwrap_or_else(|| ROOT_AGENT_ID.to_string()),
        premium,
        multimodal,
        available,
        efforts,
        fallback,
        availability,
        available_at,
    })
}

/// 策略字段相等（忽略 available/available_at 时间派生值，用于幂等判定）
fn strategy_eq(a: &ModelMeta, b: &ModelMeta) -> bool {
    a.availability == b.availability
        && a.premium == b.premium
        && a.multimodal == b.multimodal
        && a.efforts == b.efforts
        && a.fallback == b.fallback
}

/// 读取本地快照：FREE_MODELS_SNAPSHOT 环境变量 > cwd/tests/fixtures > 编译期内嵌
pub fn load_local_snapshot() -> Option<String> {
    if let Ok(p) = std::env::var("FREE_MODELS_SNAPSHOT") {
        if let Ok(s) = std::fs::read_to_string(&p) {
            return Some(s);
        }
    }
    if let Ok(s) = std::fs::read_to_string("tests/fixtures/freebuff-models.snapshot.json") {
        return Some(s);
    }
    Some(include_str!("../tests/fixtures/freebuff-models.snapshot.json").to_string())
}

impl ModelRegistry {
    pub fn new() -> Self {
        let inner = RegistryInner::default();
        Self {
            inner: Arc::new(RwLock::new(inner)),
            overrides: std::sync::Mutex::new(HashMap::new()),
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

    /// 同步读模型元数据快照（时间感知；/v1/models 消费）
    pub fn meta_snapshot(&self) -> Vec<ModelMeta> {
        self.meta_snapshot_at(Utc::now())
    }

    /// 指定时刻的元数据快照（供测试与窗口断言）
    pub fn meta_snapshot_at(&self, now: DateTime<Utc>) -> Vec<ModelMeta> {
        let over = self.overrides.lock().map(|g| g.clone()).unwrap_or_default();
        let mut out: Vec<ModelMeta> = Vec::with_capacity(MODEL_META_ROWS.len() + 2);
        for r in MODEL_META_ROWS {
            if let Some(m) = merge_into_meta(r.id, Some(r), over.get(r.id), now) {
                out.push(m);
            }
        }
        let mut extra: Vec<ModelMeta> = Vec::new();
        for (id, o) in &over {
            if !MODEL_META_ROWS.iter().any(|r| r.id == id) {
                if let Some(m) = merge_into_meta(id, None, Some(o), now) {
                    extra.push(m);
                }
            }
        }
        // 审计 NIT1：表外行按 id 排序，输出确定性
        extra.sort_by(|a, b| a.id.cmp(&b.id));
        out.extend(extra);
        out
    }

    /// 同步读单个模型元数据（时间感知；未知模型返回 None）
    pub fn meta_for(&self, id: &str) -> Option<ModelMeta> {
        self.meta_for_at(id, Utc::now())
    }

    /// 指定时刻的单个模型元数据（供测试与窗口断言）
    pub fn meta_for_at(&self, id: &str, now: DateTime<Utc>) -> Option<ModelMeta> {
        let s = MODEL_META_ROWS.iter().find(|r| r.id == id);
        let over = self.overrides.lock().ok().and_then(|g| g.get(id).cloned());
        if s.is_none() && over.is_none() {
            return None;
        }
        merge_into_meta(id, s, over.as_ref(), now)
    }

    /// 同步读模型 efforts 阶梯（'static 切片，供同步路由场景使用）
    pub fn efforts_static(&self, id: &str) -> Option<&'static [&'static str]> {
        MODEL_META_ROWS
            .iter()
            .find(|r| r.id == id)
            .and_then(|r| r.efforts)
    }

    /// 模型当前是否可用（静态暂停 && 时间窗；未知模型默认可用，不误伤上游动态新增）
    pub fn model_available(&self, id: &str) -> bool {
        self.model_available_at(id, Utc::now())
    }

    /// 指定时刻的可用性（供路由时间感知）
    pub fn model_available_at(&self, id: &str, now: DateTime<Utc>) -> bool {
        self.meta_for_at(id, now)
            .map(|m| m.available)
            .unwrap_or(true)
    }

    /// 是否拥有策略元数据（静态表或上游快照覆盖）；false = "未经策略验证"
    pub fn is_known(&self, id: &str) -> bool {
        MODEL_META_ROWS.iter().any(|r| r.id == id)
            || self
                .overrides
                .lock()
                .map(|g| g.contains_key(id))
                .unwrap_or(false)
    }

    /// 解析上游快照 JSON 并合并策略覆盖（新增模型 / 更新 availability/efforts/fallback；不删除硬编码条目）
    ///
    /// 返回 (added, updated)；幂等：再次应用相同快照返回 (0, 0)。
    /// 失败（坏 JSON）返回 Err，由调用方静默降级到静态底座并 warn。
    pub fn refresh_strategy_from_snapshot(&self, json: &str) -> anyhow::Result<(usize, usize)> {
        let file: SnapshotFile =
            serde_json::from_str(json).map_err(|e| anyhow::anyhow!("上游模型快照解析失败: {e}"))?;
        let now = Utc::now();
        let mut guard = self
            .overrides
            .lock()
            .map_err(|_| anyhow::anyhow!("策略覆盖表锁污染"))?;
        let mut added = 0usize;
        let mut updated = 0usize;
        for row in &file.models {
            if !row.catalog {
                continue;
            }
            let static_row = MODEL_META_ROWS.iter().find(|r| r.id == row.id);
            let patch = override_from_snapshot(row);
            // 审计 NIT3：无任何策略字段的行不写入覆盖（避免"空覆盖也算已知"）
            if patch.availability.is_none()
                && patch.premium.is_none()
                && patch.multimodal.is_none()
                && patch.efforts.is_none()
                && patch.fallback.is_none()
            {
                continue;
            }
            let prev_override = guard.get(&row.id).cloned();
            let prev = merge_into_meta(&row.id, static_row, prev_override.as_ref(), now);
            let merged = match &prev_override {
                Some(b) => merge_override(b, &patch),
                None => patch,
            };
            let next = merge_into_meta(&row.id, static_row, Some(&merged), now);
            if static_row.is_none() && prev_override.is_none() {
                added += 1;
            } else if let (Some(a), Some(b)) = (prev, next) {
                if !strategy_eq(&a, &b) {
                    updated += 1;
                }
            }
            guard.insert(row.id.clone(), merged);
        }
        Ok((added, updated))
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

    #[test]
    fn availability_window_inside_weekday_false() {
        // 2026-09-16 周三；03:00 UTC 处于 [00:00,10:00) 高价窗内
        let t = DateTime::parse_from_rfc3339("2026-09-16T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(is_deepseek_expensive_window(t));
        assert!(!availability_now("off_peak_only", t));
        let m = ModelRegistry::new().meta_for_at("z-ai/glm-5.3-flash", t);
        assert!(m.is_some());
    }

    #[test]
    fn availability_window_outside_weekday_true() {
        let t = DateTime::parse_from_rfc3339("2026-09-16T15:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!is_deepseek_expensive_window(t));
        assert!(availability_now("off_peak_only", t));
    }

    #[test]
    fn availability_window_boundary_midnight_in_window() {
        // 00:00 属于高价窗（半开 [00:00, 10:00)）
        let t = DateTime::parse_from_rfc3339("2026-09-16T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(is_deepseek_expensive_window(t));
    }

    #[test]
    fn availability_window_boundary_10_00_exclusive() {
        // 10:00 不在高价窗（半开）
        let t = DateTime::parse_from_rfc3339("2026-09-16T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!is_deepseek_expensive_window(t));
    }

    #[test]
    fn availability_window_beijing_weekend_exempt() {
        // 2026-09-19 是周六；03:00 UTC = 北京 11:00 周六 → 高价窗豁免
        let t = DateTime::parse_from_rfc3339("2026-09-19T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!is_deepseek_expensive_window(t));
        assert!(availability_now("off_peak_only", t));
    }

    #[test]
    fn availability_deployment_hours_true_unknown_lenient() {
        let t = DateTime::parse_from_rfc3339("2026-09-16T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(
            availability_now("deployment_hours", t),
            "deployment_hours 不据此拒绝"
        );
        assert!(
            availability_now("mystery_policy", t),
            "未知策略按可用处理（不过度拒绝）"
        );
    }

    #[test]
    fn off_peak_override_drives_available_and_available_at() {
        // 通过快照把 glm-5.3 的 availability 覆盖为 off_peak_only，验证窗口/恢复时刻
        let reg = ModelRegistry::new();
        let snap = r#"{"_source":"t","_vended_at":"2026-09-19","models":[{"id":"z-ai/glm-5.3-flash","availability":"off_peak_only","catalog":true}]}"#;
        let (added, updated) = reg.refresh_strategy_from_snapshot(snap).unwrap();
        assert_eq!((added, updated), (0, 1), "静态行更新 = 1");
        let in_win = DateTime::parse_from_rfc3339("2026-09-16T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let out_win = DateTime::parse_from_rfc3339("2026-09-16T15:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let m_in = reg.meta_for_at("z-ai/glm-5.3-flash", in_win).unwrap();
        assert!(!m_in.available, "窗口内不可用");
        let aa = m_in
            .available_at
            .as_deref()
            .expect("off_peak 窗口内应给 availableAt");
        assert!(
            DateTime::parse_from_rfc3339(aa).is_ok(),
            "availableAt 可解析: {aa}"
        );
        let m_out = reg.meta_for_at("z-ai/glm-5.3-flash", out_win).unwrap();
        assert!(m_out.available, "窗口外可用");
        assert!(m_out.available_at.is_none(), "窗口外无 availableAt");
    }

    #[test]
    fn refresh_snapshot_merges_new_and_is_idempotent() {
        let reg = ModelRegistry::new();
        let snap = r#"{"_source":"t","_vended_at":"2026-09-19","models":[
            {"id":"fake/new-model-a","availability":"always","premium":true,"multimodal":false,"catalog":true},
            {"id":"z-ai/glm-5.3-flash","efforts":["low","high","max"],"catalog":true}
        ]}"#;
        let (a1, u1) = reg.refresh_strategy_from_snapshot(snap).unwrap();
        assert_eq!((a1, u1), (1, 0), "新增 1、静态无变化");
        assert!(
            reg.meta_for("fake/new-model-a").is_some(),
            "新模型进入 meta"
        );
        assert!(reg.is_known("fake/new-model-a"));
        let (a2, u2) = reg.refresh_strategy_from_snapshot(snap).unwrap();
        assert_eq!((a2, u2), (0, 0), "再次应用幂等");
    }

    #[test]
    fn refresh_snapshot_bad_json_returns_err() {
        let reg = ModelRegistry::new();
        assert!(reg.refresh_strategy_from_snapshot("{ not json").is_err());
    }

    #[test]
    fn refresh_snapshot_preserves_missing_fields() {
        // 快照行缺少 unavailableFallback 时不得覆盖既有 fallback
        let reg = ModelRegistry::new();
        let before = reg.meta_for("deepseek/deepseek-v4-flash").unwrap();
        assert_eq!(before.fallback.as_deref(), Some("openai/gpt-5.6-luna"));
        let snap = r#"{"_source":"t","_vended_at":"2026-09-19","models":[{"id":"deepseek/deepseek-v4-flash","availability":"always","catalog":true}]}"#;
        reg.refresh_strategy_from_snapshot(snap).unwrap();
        let after = reg.meta_for("deepseek/deepseek-v4-flash").unwrap();
        assert_eq!(
            after.fallback.as_deref(),
            Some("openai/gpt-5.6-luna"),
            "缺字段不得清空 fallback"
        );
    }

    #[test]
    fn mimo_meta_is_present_and_correct() {
        let reg = ModelRegistry::new();
        let m = reg.meta_for("mimo/mimo-v2.5").expect("mimo 应有元数据");
        assert!(m.available);
        assert!(!m.premium);
        assert!(m.multimodal);
        assert!(m.efforts.is_none(), "mimo 无 efforts 阶梯");
        assert!(reg.is_known("mimo/mimo-v2.5"));
    }

    #[test]
    fn is_known_marks_dynamic_models_unverified() {
        let reg = ModelRegistry::new();
        assert!(reg.is_known("z-ai/glm-5.3-flash"));
        // 上游 free-agents 动态新增但无静态 meta → 未经策略验证
        assert!(!reg.is_known("google/gemini-2.5-flash-lite"));
    }

    #[test]
    fn vendored_snapshot_applies_cleanly() {
        // 内置/本地快照与静态表对齐 → (0,0) 幂等自检
        let snap = load_local_snapshot().expect("内置快照存在");
        let reg = ModelRegistry::new();
        let (added, updated) = reg.refresh_strategy_from_snapshot(&snap).unwrap();
        assert_eq!(added, 0, "快照不应新增静态表外 catalog 行");
        assert_eq!(updated, 0, "对齐的快照不应产生更新");
    }

    #[test]
    fn meta_snapshot_includes_availability_fields() {
        let reg = ModelRegistry::new();
        let snap = reg.meta_snapshot();
        assert!(snap.len() >= 20, "含 mimo 至少 20 条");
        let v = serde_json::to_value(&snap[0]).unwrap();
        assert!(v.get("availability").is_some(), "meta 含 availability");
        assert!(v.get("available_at").is_some(), "meta 含 available_at");
    }
}
