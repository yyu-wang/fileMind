//! 云提供商输入校验（原 `commands/cloud_providers.rs` 拆出）。
//!
//! P-07：防止表单脏值流入 DB / URL 转发。

pub(super) fn validate_display_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("PROV-110:供应商名称不能为空".to_string());
    }
    if trimmed.chars().count() > 128 {
        return Err("PROV-111:供应商名称不能超过 128 字符".to_string());
    }
    Ok(())
}

pub(super) fn validate_remark(remark: &str) -> Result<(), String> {
    if remark.chars().count() > 255 {
        return Err("PROV-112:备注不能超过 255 字符".to_string());
    }
    Ok(())
}

/// 校验可选官网：空串/None → None；合法 https → Some(raw)；http 仅允许本地回环。
pub(super) fn validate_optional_website(website: Option<&str>) -> Result<Option<String>, String> {
    let Some(raw) = website else { return Ok(None) };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let Some(rest) = trimmed.strip_prefix("https://") else {
        if let Some(rest_local) = trimmed.strip_prefix("http://") {
            // 本地代理开发调试用：只放行 localhost / 127.0.0.1 + 端口
            let host_part = rest_local.split_once('/').map_or(rest_local, |(h, _)| h);
            let host = host_part.split(':').next().unwrap_or(host_part);
            if host == "localhost" || host == "127.0.0.1" {
                return Ok(Some(trimmed.to_string()));
            }
        }
        return Err(
            "PROV-120:官网链接必须是 https://，仅本地调试允许 http://localhost".to_string(),
        );
    };
    if rest.is_empty() {
        return Err("PROV-121:官网链接缺少域名".to_string());
    }
    Ok(Some(trimmed.to_string()))
}

/// 校验 `base_url`：必须 `https://host[/prefix]`，允许本地 http；不许 `/` 结尾。
pub(super) fn validate_base_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim().to_string();
    if trimmed.is_empty() {
        return Err("PROV-130:请求地址不能为空".to_string());
    }
    if trimmed.ends_with('/') {
        return Err("PROV-131:请求地址不能以 '/' 结尾（提示：填写到域名+前缀即可）".to_string());
    }
    let Some(rest) = trimmed.strip_prefix("https://") else {
        if let Some(rest_local) = trimmed.strip_prefix("http://") {
            let host_part = rest_local.split_once('/').map_or(rest_local, |(h, _)| h);
            let host = host_part.split(':').next().unwrap_or(host_part);
            if host != "localhost" && host != "127.0.0.1" {
                return Err(
                    "PROV-132:请求地址仅允许 https://，本地中转才可使用 http://localhost"
                        .to_string(),
                );
            }
            return Ok(trimmed);
        }
        return Err("PROV-133:请求地址必须是 https:// 开头的 URL".to_string());
    };
    if rest.is_empty() {
        return Err("PROV-134:请求地址缺少域名".to_string());
    }
    Ok(trimmed)
}
