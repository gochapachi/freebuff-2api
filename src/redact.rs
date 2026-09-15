//! 日志/遥测脱敏（v0.8）：把敏感凭证片段在写入日志总线/遥测前替换为 `***`。
//!
//! 默认开启（`config.redact_logs`，环境变量 `REDACT_LOGS` 可关）。
//! 脱敏对象：
//! - `__Secure-next-auth.session-token=...` / `session-token=...` 等 Cookie 值
//! - `authorization: Bearer xxx` / 请求体里的 `"authorization":"..."`
//! - 明文 Bearer / `sk-` 开头的密钥串
//!
//! 只做"片段级"替换，保留可读上下文（如错误原因前缀），不破坏日志结构。

/// 是否启用脱敏（进程级缓存判断放调用方——避免每次日志都读环境变量）
pub const DEFAULT_REDACT_LOGS: bool = true;

/// 对一段文本做脱敏：替换 Cookie 值、Bearer 令牌、authorization 值。
pub fn redact(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut out = text.to_string();
    // 1) Cookie 值（next-auth 三件套）：`name=value` → `name=***`
    for marker in [
        "__Secure-next-auth.session-token",
        "__Host-next-auth.session-token",
        "next-auth.session-token",
        "session-token",
        ".next-auth.callback-url",
        "callback-url",
        "next-auth.csrf-token",
        "csrf-token",
    ] {
        out = redact_assigned_value(&out, marker);
    }
    // 2) authorization 头/字段值
    for pat in ["authorization:", "authorization\"", "Authorization:"] {
        out = redact_bearer_after(&out, pat);
    }
    // 3) 裸 Bearer token（出现在消息正文/错误体时）
    out = redact_bearer_after(&out, "Bearer ");
    // 4) sk- 开头的密钥串（OpenAI 风格，len >= 20 才认为是真 key）
    out = redact_sk_tokens(&out);
    // 5) 裸 JWT（CSP/DEPTH 兜底）：无 marker/Bearer 前缀、以 eyJ 开头（JWT header 的
    //    base64url 特征）的三段式令牌 —— Freebuff 的 web cookie 值本身就是 JWT，
    //    若上游错误体/日志中出现不带 marker 的裸 JWT（防御纵深），此处兜底替换。
    out = redact_jwt(&out);
    out
}

/// 把 `eyJ<seg1>.<seg2>.<seg3>` 形状的裸 JWT 替换为 `eyJ***`（保留前缀便于辨认）。
/// 要求：每段都是 base64url 字符（A-Za-z0-9_-），总长 >= 60 才认为是真 JWT，避免误伤普通文本。
fn redact_jwt(text: &str) -> String {
    if !text.contains("eyJ") {
        return text.to_string();
    }
    let b64 = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("eyJ") {
        out.push_str(&rest[..pos]);
        let seg = |s: &str| s.chars().take_while(|c| b64(*c)).count();
        let after = &rest[pos..];
        let seg1 = seg(after);
        if seg1 >= 10 {
            if let Some(dot1) = after.get(seg1..).and_then(|s| s.strip_prefix('.')) {
                let seg2 = seg(dot1);
                if seg2 >= 10 {
                    if let Some(dot2) = dot1.get(seg2..).and_then(|s| s.strip_prefix('.')) {
                        let seg3 = seg(dot2);
                        // 三段总长 >= 60 且末段非空 → 判定为 JWT
                        if seg1 + seg2 + seg3 >= 60 && seg3 >= 10 {
                            let end = seg1 + 1 + seg2 + 1 + seg3;
                            out.push_str("eyJ***");
                            rest = &after[end.min(after.len())..];
                            continue;
                        }
                    }
                }
            }
        }
        // 不是 JWT：把已扫过的"eyJ"逐字保留，继续前进一个字符（防止死循环）
        out.push_str("eyJ");
        rest = &after[3..];
    }
    out.push_str(rest);
    out
}

/// 把 `marker=...` 形式的赋值值替换为 `***`（保留键名与 = 号）
fn redact_assigned_value(text: &str, marker: &str) -> String {
    if !text.contains(marker) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(marker) {
        out.push_str(&rest[..pos + marker.len()]);
        let after = &rest[pos + marker.len()..];
        // 期望紧跟 `=`
        if let Some(eq_rest) = after.strip_prefix('=') {
            out.push('=');
            // 取到下一个分隔符（; 空格 & " ' 换行 或结束）为止作为值
            let end = eq_rest
                .find([';', ' ', '&', '"', '\'', '\n', '\r'])
                .unwrap_or(eq_rest.len());
            out.push_str("***");
            rest = &eq_rest[end..];
        } else {
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// 把 `prefix xxx`（prefix 后到分隔符/行尾）替换：保留 prefix，值改 `***`
fn redact_bearer_after(text: &str, prefix: &str) -> String {
    if !text.contains(prefix) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(prefix) {
        out.push_str(&rest[..pos + prefix.len()]);
        let after = &rest[pos + prefix.len()..];
        let end = after
            .find([';', ' ', '\n', '\r', '"', ',', '}'])
            .unwrap_or(after.len());
        let val = &after[..end];
        // 只有值看起来像 token 才脱敏（避免把普通英文单词误伤，如 "Bearer token 不存在"）
        if val.len() >= 12 && val.chars().any(|c| c.is_ascii_alphanumeric()) {
            out.push_str("***");
        } else {
            out.push_str(val);
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// 把 `sk-...` 长串（>=20 字符）替换为 `sk-***`
fn redact_sk_tokens(text: &str) -> String {
    if !text.contains("sk-") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("sk-") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 3..];
        // sk- 后到分隔符
        let end = after
            .find([';', ' ', '\n', '\r', '"', ',', '}', '='])
            .unwrap_or(after.len());
        let tail = &after[..end];
        if tail.len() >= 17 {
            out.push_str("sk-***");
            rest = &after[end..];
        } else {
            out.push_str("sk-");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_cookie_values() {
        let s = "Cookie: __Secure-next-auth.session-token=abc123def; other=1";
        let out = redact(s);
        assert!(!out.contains("abc123def"), "Cookie 值必须被脱敏: {out}");
        assert!(
            out.contains("__Secure-next-auth.session-token=***"),
            "保留键名: {out}"
        );
        assert!(out.contains("other=1"), "非敏感 cookie 保留: {out}");
    }

    #[test]
    fn redacts_bearer_authorization() {
        let s = "authorization: Bearer sk-verysecretlongtoken123456";
        let out = redact(s);
        assert!(
            !out.contains("sk-verysecretlongtoken123456"),
            "Bearer 值必须脱敏: {out}"
        );
        assert!(out.contains("Bearer ***"), "保留 Bearer 前缀: {out}");
    }

    #[test]
    fn redacts_authorization_json_field() {
        let s = r#"{"authorization":"Bearer abcdefghijklmnopqrstuvwxyz123"}"#;
        let out = redact(s);
        assert!(
            !out.contains("abcdefghijklmnopqrstuvwxyz123"),
            "JSON 内 authorization 必须脱敏: {out}"
        );
    }

    #[test]
    fn redacts_sk_style_tokens() {
        let s = "key=sk-proj-abcdefghijklmnopqrstuvwxyz1234567890";
        let out = redact(s);
        assert!(
            !out.contains("sk-proj-abcdefghijklmnopqrstuvwxyz"),
            "sk- 长串必须脱敏: {out}"
        );
        assert!(out.contains("sk-***"), "保留 sk- 前缀: {out}");
    }

    #[test]
    fn keeps_normal_text_untouched() {
        let s = "上游排队 waiting_room，请稍候 15 秒重试";
        assert_eq!(redact(s), s);
        let s2 = "Bearer token 不存在"; // 值太短不像 token → 保留
        assert_eq!(redact(s2), s2);
    }

    #[test]
    fn empty_input_ok() {
        assert_eq!(redact(""), "");
    }

    #[test]
    fn redacts_bare_jwt_without_marker() {
        // 裸 JWT（无 marker/Bearer 前缀）：eyJ 三段式，总长 >= 60 判定为 JWT 并脱敏
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        assert!(jwt.len() >= 60);
        let out = redact(jwt);
        assert!(!out.contains("eyJhbGci"), "裸 JWT 必须脱敏: {out}");
        assert!(out.starts_with("eyJ***"), "保留 eyJ 前缀: {out}");
        // 较短 base64 串（如普通文本含 eyJxxx.xx.x）不误伤
        let short = "随便提到的 eyJab.xyz.abc";
        assert!(redact(short).contains("eyJab"), "短串不误伤");
        // 正常中文文本不受影响
        let zh = "上游排队 waiting_room，请稍候";
        assert_eq!(redact(zh), zh);
    }

    #[test]
    fn jwt_inside_text_is_redacted_but_context_kept() {
        let s = "错误：凭证已过期 token=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let out = redact(s);
        assert!(out.contains("凭证已过期"), "保留上下文");
        assert!(!out.contains("eyJhbGci"), "JWT 被脱敏: {out}");
    }
}
