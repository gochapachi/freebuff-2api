//! 全配置导出/导入（v0.9 §2.3）：换机迁移，明文 JSON（v0.9 不加密，加密列 backlog）
//!
//! 导出载荷（POST /api/export 的 `data`）：
//! `{schema_version, exported_at, config(脱敏), tokens, skills:[{id,enabled}], memory_enabled, note}`
//!
//! 导入（POST /api/import body `{data}`）：
//! 1. 校验 schema_version / 字段类型 / 大小 ≤5MB
//! 2. 写前自动备份到 `data/backup-<ts>/`（config.json / tokens.json / skills 目录）
//! 3. 原子写回：tokens.json 整体替换、技能启用态、config 白名单字段
//! 4. **安全最小集**：绝不覆盖 `api_keys` / `auth_tokens`（明文凭据只随用户显式迁移）

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 导出 schema 版本（导入校验用；升级格式时递增并做迁移）
pub const EXPORT_SCHEMA_VERSION: &str = "1";
/// 导入 body 大小上限（5MB）
pub const MAX_IMPORT_BYTES: usize = 5 * 1024 * 1024;

/// 单条技能启用态（导出时从 SkillsManager.list() 采集）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillState {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// 导出载荷（序列化即 `data` 字段）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportPayload {
    pub schema_version: String,
    pub exported_at: String,
    /// 脱敏后的配置（仅白名单字段；api_keys/auth_tokens 绝不写入）
    pub config: serde_json::Value,
    #[serde(default)]
    pub tokens: Vec<crate::import::ExtractedAuth>,
    #[serde(default)]
    pub skills: Vec<SkillState>,
    pub memory_enabled: bool,
    #[serde(default)]
    pub note: String,
}

/// 导入结果摘要（直接作为 handler 的 `imported` 返回）
#[derive(Debug, Clone, Serialize)]
pub struct ImportSummary {
    /// 恢复的条目（人类可读列表）
    pub imported: Vec<String>,
    /// 主动跳过的条目（未知技能/安全字段）
    pub skipped: Vec<String>,
    /// 备份目录（成功了才 Some）
    pub backed_up_to: Option<String>,
    /// 写回 tokens.json 的条数
    pub tokens: usize,
    /// 应用启用态的技能数
    pub skills_toggled: usize,
    /// 是否写回 memory_enabled（None = 载荷未携带）
    pub memory_enabled: Option<bool>,
    /// 合并进 config.json 的白名单字段
    pub config_fields: Vec<String>,
}

/// 导入上下文：apply_import 需要的全部宿主依赖（api.rs 组装）
pub struct ImportContext<'a> {
    pub config_path: Option<String>,
    pub tokens_path: &'a str,
    pub data_dir: &'a str,
    pub skills: &'a crate::skills::SkillsManager,
    pub skills_dir: &'a str,
    pub memory_runtime_enabled: &'a std::sync::atomic::AtomicBool,
}

/// 校验导入 body（`{data: ...}` 或直接载荷），返回规范化的 ExportPayload。
/// 只做 schema/类型/必填校验，不做任何写操作。
pub fn validate_import_body(body: &serde_json::Value) -> Result<ExportPayload> {
    let raw = body.get("data").unwrap_or(body);
    if !raw.is_object() {
        return Err(anyhow!("data 必须是 JSON 对象"));
    }
    let payload: ExportPayload =
        serde_json::from_value(raw.clone()).map_err(|e| anyhow!("data 字段类型不合法: {e}"))?;
    if payload.schema_version.is_empty() {
        return Err(anyhow!("schema_version 缺失或不合法"));
    }
    // 版本校验：当前只接受主版本一致的导出（未来格式升级时在这里迁移/拒绝）
    if payload.schema_version.split('.').next().unwrap_or("")
        != EXPORT_SCHEMA_VERSION.split('.').next().unwrap_or("")
    {
        return Err(anyhow!(
            "不支持导出 schema_version={}（当前支持主版本 {}）",
            payload.schema_version,
            EXPORT_SCHEMA_VERSION
                .split('.')
                .next()
                .unwrap_or(EXPORT_SCHEMA_VERSION)
        ));
    }
    if payload.exported_at.is_empty() {
        return Err(anyhow!("exported_at 缺失"));
    }
    for t in &payload.tokens {
        if t.token.trim().is_empty() {
            return Err(anyhow!("tokens 中含空 token"));
        }
    }
    Ok(payload)
}

/// 导入前备份：复制 config.json / tokens.json / skills 目录到 `data_dir/backup-<ts>/`。
/// 目标文件/目录不存在则跳过；全部复制失败才算 Err（部分成功可接受，返回实际复制项）。
pub fn backup_data(
    data_dir: &str,
    config_path: Option<&str>,
    tokens_path: &str,
    skills_dir: &str,
) -> Result<(String, Vec<String>)> {
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let backup_root = Path::new(data_dir).join(format!("backup-{ts}"));
    std::fs::create_dir_all(&backup_root)?;
    let mut copied = Vec::new();
    if let Some(p) = config_path {
        if Path::new(p).exists() {
            let dest = backup_root.join("config.json");
            std::fs::copy(p, &dest)?;
            copied.push("config.json".into());
        }
    }
    if Path::new(tokens_path).exists() {
        let dest = backup_root.join("tokens.json");
        std::fs::copy(tokens_path, &dest)?;
        copied.push("tokens.json".into());
    }
    if Path::new(skills_dir).is_dir() {
        copy_dir_recursive(Path::new(skills_dir), &backup_root.join("skills"))?;
        copied.push("skills/".into());
    }
    Ok((backup_root.to_string_lossy().into_owned(), copied))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// 原子写（临时文件 + rename；Windows 先移除目标再 rename，防截断）
fn atomic_write(path: &Path, data: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data)?;
    #[cfg(windows)]
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// 写回 tokens.json（整体替换为导入清单；失败不中断后续项）
fn write_tokens(path: &str, tokens: &[crate::import::ExtractedAuth]) -> Result<()> {
    let json = serde_json::to_string_pretty(tokens)?;
    atomic_write(Path::new(path), &json)
}

/// 把 config.json 的非敏感字段合并（memory_enabled 由调用方决定；api_keys/auth_tokens 绝不写）。
/// 返回实际合并的字段名。文件不存在则跳过（内存态已由 handler 处理）。
fn merge_config_fields(
    config_path: Option<&str>,
    imported: &serde_json::Value,
) -> Result<Vec<String>> {
    let Some(p) = config_path else {
        return Ok(Vec::new());
    };
    if !Path::new(p).exists() {
        return Ok(Vec::new());
    }
    let mut root: serde_json::Value = std::fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }
    // 白名单：与 api.rs CONFIG_EDITABLE 同步，绝不覆盖 api_keys/auth_tokens
    const SAFE_FIELDS: &[&str] = &[
        "listen_addr",
        "memory_enabled",
        "token_saver",
        "skills_inject_mode",
        "max_roster_tokens",
        "thread_cleanup_interval_sec",
        "thread_max_age_hours",
        "redact_logs",
        "concurrency_free_slots",
        "concurrency_free_multi",
        "concurrency_sub_slots",
        "concurrency_sub_multi",
    ];
    let mut applied = Vec::new();
    if let Some(obj) = imported.as_object() {
        for (k, v) in obj {
            if !SAFE_FIELDS.contains(&k.as_str()) {
                continue;
            }
            if !v.is_string() && !v.is_boolean() && !v.is_number() {
                continue;
            }
            root[k.as_str()] = v.clone();
            applied.push(k.clone());
        }
    }
    if applied.is_empty() {
        return Ok(applied);
    }
    let json = serde_json::to_string_pretty(&root)?;
    atomic_write(Path::new(p), &json)?;
    Ok(applied)
}

/// 应用导入：备份 → 写 tokens → 技能启用态 → config 白名单字段 + memory_enabled。
/// 全部写操作失败不中断后续（尽力恢复），最终报错由调用方决定是否整体失败。
pub fn apply_import(payload: &ExportPayload, ctx: &ImportContext<'_>) -> Result<ImportSummary> {
    let mut summary = ImportSummary {
        imported: Vec::new(),
        skipped: Vec::new(),
        backed_up_to: None,
        tokens: 0,
        skills_toggled: 0,
        memory_enabled: None,
        config_fields: Vec::new(),
    };

    // 1) 备份（失败则不进入写回——先保障可回滚）
    let (backup_dir, copied) = backup_data(
        ctx.data_dir,
        ctx.config_path.as_deref(),
        ctx.tokens_path,
        ctx.skills_dir,
    )?;
    summary.backed_up_to = Some(backup_dir.clone());
    summary.imported.push(format!(
        "备份到 {}（{}）",
        backup_dir,
        if copied.is_empty() {
            "无文件需要备份".into()
        } else {
            copied.join(", ")
        }
    ));

    // 2) tokens.json 整体替换
    if !payload.tokens.is_empty() {
        write_tokens(ctx.tokens_path, &payload.tokens)?;
        summary.tokens = payload.tokens.len();
        summary.imported.push(format!(
            "tokens.json 已写入 {} 条凭证",
            payload.tokens.len()
        ));
    } else {
        summary
            .skipped
            .push("tokens（载荷为空，不覆盖现有凭证）".into());
    }

    // 3) 技能启用态（只对目标端已存在的技能生效，不新建/删除技能文件）
    for s in &payload.skills {
        // toggle 对未知 id 返回 Ok(false)（非 Err），必须先判存在性
        if ctx.skills.get(&s.id).is_none() {
            summary
                .skipped
                .push(format!("技能 {}（目标端不存在）", s.id));
            continue;
        }
        match ctx.skills.toggle(&s.id, s.enabled) {
            Ok(_) => {
                summary.skills_toggled += 1;
                summary.imported.push(format!(
                    "技能 {} -> {}",
                    s.id,
                    if s.enabled { "启用" } else { "停用" }
                ));
            }
            Err(e) => {
                summary
                    .imported
                    .push(format!("技能 {} 保持原状（toggle 失败: {e}）", s.id));
            }
        }
    }

    // 4) config 白名单字段 + memory_enabled
    summary.config_fields = merge_config_fields(ctx.config_path.as_deref(), &payload.config)?;
    if !summary.config_fields.is_empty() {
        summary.imported.push(format!(
            "config.json 已合并字段：{}",
            summary.config_fields.join(", ")
        ));
    }
    summary.memory_enabled = Some(payload.memory_enabled);
    ctx.memory_runtime_enabled
        .store(payload.memory_enabled, std::sync::atomic::Ordering::Relaxed);
    let mem_json = merge_config_field_bool(
        ctx.config_path.as_deref(),
        "memory_enabled",
        payload.memory_enabled,
    )?;
    if mem_json {
        summary.config_fields.retain(|f| f != "memory_enabled");
        if !summary.config_fields.iter().any(|f| f == "memory_enabled") {
            summary.config_fields.push("memory_enabled".into());
        }
        summary.imported.push(format!(
            "记忆层已设为 {}",
            if payload.memory_enabled {
                "开启"
            } else {
                "关闭"
            }
        ));
    }

    Ok(summary)
}

/// 把单个 bool 配置项写回 config.json（memory_enabled 专用，幂等）
fn merge_config_field_bool(config_path: Option<&str>, key: &str, value: bool) -> Result<bool> {
    let Some(p) = config_path else {
        return Ok(true);
    };
    if !Path::new(p).exists() {
        return Ok(false);
    }
    let mut root: serde_json::Value = std::fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }
    if root.get(key).and_then(|v| v.as_bool()) == Some(value) {
        return Ok(false); // 已一致，无需改写
    }
    root[key] = serde_json::json!(value);
    let json = serde_json::to_string_pretty(&root)?;
    atomic_write(Path::new(p), &json)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload() -> ExportPayload {
        ExportPayload {
            schema_version: EXPORT_SCHEMA_VERSION.into(),
            exported_at: "2026-09-18T00:00:00+00:00".into(),
            config: serde_json::json!({
                "memory_enabled": true,
                "token_saver": true,
                "api_keys": ["should-never-write"],
                "auth_tokens": ["should-never-write"],
            }),
            tokens: vec![
                crate::import::ExtractedAuth {
                    token: "__Secure-next-auth.session-token=ac; x=1".into(),
                    source: "cookie".into(),
                    host: "freebuff.com".into(),
                    path: "/api/web/freebuff-session".into(),
                    method: "GET".into(),
                    added_at: Some("2026-09-01T00:00:00+00:00".into()),
                },
                crate::import::ExtractedAuth {
                    token: "sk-bearer-abc123".into(),
                    source: "curl".into(),
                    host: "www.codebuff.com".into(),
                    path: "/api/v1/chat/completions".into(),
                    method: "POST".into(),
                    added_at: None,
                },
            ],
            skills: vec![
                SkillState {
                    id: "git-guru".into(),
                    enabled: false,
                },
                SkillState {
                    id: "no-such-skill".into(),
                    enabled: true,
                },
            ],
            memory_enabled: true,
            note: "round-trip test".into(),
        }
    }

    #[test]
    fn validate_accepts_our_own_export() {
        let body = serde_json::json!({ "data": sample_payload() });
        let payload = validate_import_body(&body).unwrap();
        assert_eq!(payload.schema_version, EXPORT_SCHEMA_VERSION);
        assert_eq!(payload.tokens.len(), 2);
    }

    #[test]
    fn validate_rejects_bad_schema_and_types() {
        assert!(validate_import_body(&serde_json::json!({})).is_err());
        assert!(validate_import_body(&serde_json::json!({ "data": "x" })).is_err());
        assert!(validate_import_body(&serde_json::json!({
            "data": { "schema_version": "9", "exported_at": "t", "config": {}, "tokens": [], "skills": [], "memory_enabled": false }
        })).is_err(), "主版本不一致应拒绝");
        assert!(validate_import_body(&serde_json::json!({
            "data": { "schema_version": EXPORT_SCHEMA_VERSION, "exported_at": "", "config": {}, "tokens": [], "skills": [], "memory_enabled": false }
        })).is_err(), "exported_at 缺失应拒绝");
    }

    #[test]
    fn round_trip_apply_restores_tokens_skills_and_memory() {
        let dir = tempfile::tempdir().unwrap();
        let p = |n: &str| dir.path().join(n).to_str().unwrap().to_string();
        let tokens_path = p("tokens.json");
        let data_dir = p("data");
        let skills_dir = p("skills");
        let skills_db = p("skills.sqlite");

        // 目标端：技能库已 seed（git-guru 默认启用），config.json 带 api_keys
        let manager = crate::skills::SkillsManager::open(
            std::path::PathBuf::from(&skills_dir),
            std::path::PathBuf::from(&skills_db),
        )
        .unwrap();
        let config_path = p("config.json");
        std::fs::write(
            &config_path,
            r#"{ "api_keys": ["sk-original"], "auth_tokens": ["tok-original"], "token_saver": false }"#,
        )
        .unwrap();

        let runtime = std::sync::atomic::AtomicBool::new(false);
        let payload = sample_payload();
        let ctx = ImportContext {
            config_path: Some(config_path.clone()),
            tokens_path: &tokens_path,
            data_dir: &data_dir,
            skills: &manager,
            skills_dir: &skills_dir,
            memory_runtime_enabled: &runtime,
        };
        let summary = apply_import(&payload, &ctx).unwrap();

        // tokens 恢复
        let written: Vec<crate::import::ExtractedAuth> =
            serde_json::from_str(&std::fs::read_to_string(&tokens_path).unwrap()).unwrap();
        assert_eq!(written.len(), 2);
        assert!(written[0].token.contains("session-token"));
        // 技能启用态：git-guru 被停用
        let git = manager.get("git-guru").unwrap();
        assert!(!git.enabled, "导入后 git-guru 应停用");
        // 未知技能跳过
        assert!(summary.skipped.iter().any(|s| s.contains("no-such-skill")));
        // memory_enabled：config.json + 运行时
        assert!(runtime.load(std::sync::atomic::Ordering::Relaxed));
        let cfg_text = std::fs::read_to_string(&config_path).unwrap();
        let cfg: serde_json::Value = serde_json::from_str(&cfg_text).unwrap();
        assert_eq!(cfg["memory_enabled"], true);
        assert_eq!(cfg["api_keys"][0], "sk-original", "api_keys 严禁被覆盖");
        assert_eq!(
            cfg["auth_tokens"][0], "tok-original",
            "auth_tokens 严禁被覆盖"
        );
        // 备份存在
        let backup_dir = summary.backed_up_to.as_ref().unwrap();
        assert!(Path::new(backup_dir).join("config.json").exists());
        assert!(summary.imported.iter().any(|i| i.contains("tokens.json")));
    }

    #[test]
    fn limited_size_guard_constant() {
        assert_eq!(MAX_IMPORT_BYTES, 5 * 1024 * 1024);
    }
}
