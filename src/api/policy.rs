//! 跑步策略：POST /api/v70103/runModePolicy。
//! 返回 data.runRuleModel.minDistance（提交时 selDistance 用它）与 data.policy。

use super::client::{get_field, ApiClient};
use serde_json::{json, Value};

pub const POLICY_PATH: &str = "/api/v70103/runModePolicy";

pub struct PolicyInfo {
    pub timestamp: i64,
    pub policy: i64,
    pub min_distance: i64,
    pub valid_time: i64,
}

/// body：{"runMode":<run_mode>,"ruleUpdateTime":0,"geoFenceUpdateTime":0,"selectUnid":<unid>,"operateType":0}
/// run_mode: 校园跑(打卡点)=1, 户外自由跑=2（默认值，可由 RunMode 覆盖）
pub fn fetch_policy(client: &mut ApiClient, run_mode: i64) -> Result<PolicyInfo, String> {
    let unid = client
        .login
        .as_ref()
        .map(|s| s.unid.clone())
        .unwrap_or_default();
    let select_unid = unid.parse::<i64>().unwrap_or(0);
    let body = json!({
        "runMode": run_mode,
        "ruleUpdateTime": 0,
        "geoFenceUpdateTime": 0,
        "selectUnid": select_unid,
        "operateType": 0,
    })
    .to_string();
    let biz = client.call("POST", POLICY_PATH, &body, &[])?;
    let timestamp = get_field(&biz, "timestamp")
        .and_then(|t| t.as_i64())
        .ok_or("policy 响应缺 timestamp")?;
    let policy = get_field(&biz, "policy").and_then(|t| t.as_i64()).unwrap_or(0);
    let rule = get_field(&biz, "runRuleModel").cloned().unwrap_or(Value::Null);
    Ok(PolicyInfo {
        timestamp,
        policy,
        min_distance: rule.get("minDistance").and_then(|t| t.as_i64()).unwrap_or(1000),
        valid_time: rule.get("validTime").and_then(|t| t.as_i64()).unwrap_or(0),
    })
}

/// 原始探测：按指定 runMode 拉取策略，返回完整解密业务 JSON（业务错误不拦截，
/// 便于枚举「计分跑 / 自由跑」等模式对应的参数）。
pub fn fetch_policy_raw(client: &mut ApiClient, run_mode: i64) -> Result<Value, String> {
    let unid = client
        .login
        .as_ref()
        .map(|s| s.unid.clone())
        .unwrap_or_default();
    let select_unid = unid.parse::<i64>().unwrap_or(0);
    let body = json!({
        "runMode": run_mode,
        "ruleUpdateTime": 0,
        "geoFenceUpdateTime": 0,
        "selectUnid": select_unid,
        "operateType": 0,
    })
    .to_string();
    let url = format!("{}{}", super::model::HOST, POLICY_PATH);
    let out = client.envelope_request("POST", &url, &body, crate::crypto::header::UA_IOS, &[])?;
    Ok(out.decrypted.map(|d| d.business).unwrap_or(Value::Null))
}
