//! 全链编排：policy → 实时点位 → 轨迹生成 → 提交 → OBS → 详情验证。
//! 由 UI 后台线程调用，log 闭包回传日志。

use super::client::ApiClient;
use super::model::Session;
use super::points;
use super::policy::fetch_policy;
use super::records::fetch_one_record;
use super::submit::{submit_record, SubmitParams, SubmitResult};
use crate::track::generator::build as gen_track;
use crate::track::wire::{build_obs_object, five_point_wrapper, obs_keys};
use crate::location::Coordinate;
use serde_json::Value;

/// 跑步模式：打卡点跑 vs 户外自由跑。
/// 每种模式携带对应的协议参数：policy_run_mode（拉策略用 runMode）、submit_sport_type（提交用 sportType）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// 打卡点跑：拉取实时点位，轨迹绕点位环生成，提交带五点轨迹。
    Checkpoint,
    /// 户外自由跑：不拉取点位，轨迹绕设备锚点自由生成，提交不带五点轨迹。
    Free,
}

impl RunMode {
    /// 拉策略 `/api/v70103/runModePolicy` 时传的 runMode 值。
    ///
    /// 实测（probe runMode 0..=6）：
    ///   - 1 = 校园打卡点跑：唯一返回 error=11016「不在有效跑步时间内」的模式，
    ///     需要 runAreaModels（打卡点区域）和有效时间窗口才能成功。
    ///   - 0/2/3-6 = 自由跑：全部返回 error=10000 成功，runAreaModels=null、
    ///     geoFence=null、freedomElectronicFence=0，服务器无区别。固定取 2。
    pub fn policy_run_mode(self) -> i64 {
        match self {
            RunMode::Checkpoint => 1,
            RunMode::Free => 2,
        }
    }

    /// 提交跑步记录时 body 里的 sportType。
    ///
    /// 实测（probe sportType 1..=6 + 官方 APP 验证）：
    ///   - sportType=1：计分跑（计入学期成绩）
    ///   - sportType=4：自由跑（不计入学期成绩，独立分类） ← 本次验证通过
    ///   - sportType=6：户外跑（另一个独立分类）
    ///   - sportType=2/5：服务器拒绝「uid 不存在」
    ///   - sportType=3：签名失败（有特殊 roomId 签名规则）
    pub fn submit_sport_type(self) -> i64 {
        match self {
            RunMode::Checkpoint => 1,
            RunMode::Free => 4,
        }
    }

    pub fn is_free(self) -> bool {
        matches!(self, RunMode::Free)
    }
}

#[derive(Clone, Copy)]
pub struct RunParams {
    /// 距离（米）与时长（秒）已由 UI 参数解析。
    pub dist: f64,
    pub dur: i64,
    /// 开始时间（毫秒）。
    pub start_ms: i64,
    pub face_check: i64,
    /// 用户手动填写的绝对海拔（米）；None 使用生成器海拔。
    pub manual_altitude: Option<f64>,
    /// 用户手动填写的海拔范围；与单值字段兼容，范围优先。
    pub manual_altitude_range: Option<crate::track::altitude::AltitudeRange>,
    pub seed: u64,
    /// 跑步模式。
    pub mode: RunMode,
}

pub struct RunOutcome {
    pub result: SubmitResult,
    pub obs_ok: usize,
    pub detail_ok: bool,
}

fn sleep_secs(s: u64) {
    std::thread::sleep(std::time::Duration::from_secs(s));
}

/// 跑步全链。
pub fn run_full_flow(
    client: &mut ApiClient,
    params: &RunParams,
    log: &mut dyn FnMut(&str),
) -> Result<RunOutcome, String> {
    let sess: Session = client.login.clone().ok_or("未登录")?;

    // ① policy
    log(&format!("[policy] 拉取跑步策略（runMode={}）…", params.mode.policy_run_mode()));
    let pol = fetch_policy(client, params.mode.policy_run_mode())?;
    log(&format!(
        "√ [policy] ts={} policy={} minDistance={} validTime={}",
        pol.timestamp, pol.policy, pol.min_distance, pol.valid_time
    ));
    sleep_secs(2);

    // ② 定位锚点（两种模式都必须有合法锚点）
    if client.identity.has_unconfigured_default_location() {
        return Err("请先在设备信息页填写本次跑步所在城市和定位锚点，不能使用大连默认配置".into());
    }
    let anchor: Coordinate = client.identity.anchor_coordinate()?;
    let free_run = params.mode.is_free();

    // ③ 点位：打卡点跑拉取，自由跑跳过
    let pts: Vec<Value>;
    let pts_bd: Vec<(f64, f64)>;
    if free_run {
        log("[points] 户外自由跑：跳过打卡点，轨迹将绕锚点自由生成…");
        pts = Vec::new();
        pts_bd = Vec::new();
    } else {
        log("[points] 拉取实时点位…");
        pts = points::fetch_points(client, anchor, log)?;
        if pts.is_empty() {
            return Err("实时点位为空 —— 拒绝本地样本兜底".into());
        }
        log(&format!("√ [points] {} 个点位", pts.len()));
        for p in pts.iter().take(5) {
            log(&format!(
                "  [points] {} BD=({:.6},{:.6}) GCJ=({},{})",
                p.get("pointName").and_then(|v| v.as_str()).unwrap_or(""),
                p.get("lat").and_then(|v| v.as_f64()).unwrap_or(0.0),
                p.get("lon").and_then(|v| v.as_f64()).unwrap_or(0.0),
                p.get("glat").map(|v| v.to_string()).unwrap_or_default(),
                p.get("glon").map(|v| v.to_string()).unwrap_or_default(),
            ));
        }
        pts_bd = points::points_bd(&pts);
    }

    // ④ 轨迹生成（打卡点拟合环 / 自由跑锚点环）
    // 平均配速须落在有效窗口内（否则逐点速度无法全窗内），越界时修正时长
    let mut params = *params;
    let avg = params.dist / params.dur as f64;
    let fixed_avg = avg.clamp(
        crate::track::generator::SPEED_FLOOR + 0.1,
        crate::track::generator::SPEED_CEIL - 0.1,
    );
    if (fixed_avg - avg).abs() > 1e-6 {
        let fixed_dur = (params.dist / fixed_avg).round() as i64;
        log(&format!(
            "[track] 平均配速 {} m/s 超出有效窗口，时长 {} -> {}s",
            (avg * 100.0).round() / 100.0,
            params.dur,
            fixed_dur
        ));
        params.dur = fixed_dur;
    }
    if free_run {
        log(&format!(
            "[track] 生成自由跑轨迹 {:.0}m / {}s（绕锚点闭合环）…",
            params.dist, params.dur
        ));
    } else {
        log(&format!(
            "[track] 生成轨迹 {:.0}m / {}s（{} 点位拟合环）…",
            params.dist,
            params.dur,
            pts_bd.len()
        ));
    }
    // 随机 0-4 秒偏移（终端上报的 flag 与首点差 <5s），轨迹/提交/OBS/五点统一使用
    let start_ms = params.start_ms + (rand::random::<i64>() % 5) * 1000;
    let mut track = if free_run {
        crate::track::generator::build_free(
            params.dist,
            params.dur,
            params.seed,
            (anchor.latitude, anchor.longitude),
            start_ms,
        )
    } else {
        gen_track(params.dist, params.dur, params.seed, (anchor.latitude, anchor.longitude), start_ms, &pts_bd)
    };
    if let Some(range) = params.manual_altitude_range {
        crate::track::altitude::override_bd_a_range(&mut track, range)?;
        log(&format!("√ [track] 已将海拔曲线映射到 {:.2}-{:.2}m，覆盖 {} 个点，爬升/圈数据将按覆盖值计算", range.min_m, range.max_m, track.locations.len()));
    } else if let Some(altitude_m) = params.manual_altitude {
        crate::track::altitude::override_bd_a(&mut track, altitude_m)?;
        log(&format!("√ [track] 已用手动海拔 {:.2}m 覆盖 {} 个点，爬升/圈数据将按覆盖值计算", altitude_m, track.locations.len()));
    }
    log(&format!(
        "√ [track] {} 点 totalDis={:.0}m steps={} 起点={}",
        track.locations.len(),
        track.totalDistance,
        track.totalSteps,
        chrono::Local.timestamp_millis_opt(params.start_ms).single()
            .map(|t| t.format("%H:%M:%S").to_string()).unwrap_or_default(),
    ));

    // ⑤ 五点 wrapper（跑完态）；自由跑无打卡点，提交时省略五点
    let five = if free_run {
        String::new()
    } else {
        five_point_wrapper(&pts, track.startTime)
    };
    let _ = &five;

    // ⑥ 提交
    let sp = SubmitParams {
        track,
        uid: sess.uid,
        selected_unid: sess.unid.parse().unwrap_or(0),
        policy: pol.policy,
        policy_ts: pol.timestamp,
        min_distance: pol.min_distance,
        weight: if sess.weight > 0.0 { sess.weight } else { 68.0 },
        face_check: params.face_check,
        five_point_json: five,
        address: client.identity.city.clone(),
        free_run,
        sport_type: params.mode.submit_sport_type(),
    };
    log(&format!(
        "[record] 提交跑步记录（sportType={}，{}）…",
        sp.sport_type,
        if free_run { "户外自由跑" } else { "校园打卡点跑" }
    ));
    let result = submit_record(client, &sp, log)?;
    sleep_secs(1);

    // ⑦ OBS 上传（双 key）
    log("[obs] 上传 OBS 对象（gzip+base64，10 键）…");
    // 从提交结果回填 track.startTime（含随机秒偏移），保证 body/OBS/flag 全链一致
    let mut track_for_obs = sp.track.clone();
    track_for_obs.startTime = result.start_ms;
    let obj = build_obs_object(&track_for_obs, result.rrid, &result.uuid, sess.uid, &pts);
    let payload = obj.to_string().into_bytes();
    let keys = obs_keys(&track_for_obs, result.rrid, &result.uuid);
    let obs_ok = super::obs::upload_both_keys(client, &keys, &payload, log);
    if obs_ok == 2 {
        log("√ [obs] 双 key 上传成功");
    } else {
        log(&format!("⚠ [obs] 上传成功 {obs_ok}/2"));
    }

    // ⑧ 详情验证
    sleep_secs(2);
    log("[verify] 拉取详情验证…");
    let detail_ok = match fetch_one_record(client, result.rrid) {
        Ok(d) => {
            log(&format!(
                "√ [verify] rrid={} complete={:?} dis={:?} time={:?}",
                result.rrid,
                d.get("complete").and_then(|v| v.as_bool()),
                d.get("totalDis"),
                d.get("totalTime"),
            ));
            if let Ok(mut slot) = VERIFY_DETAIL.lock() {
                slot.replace(d.clone());
            }
            true
        }
        Err(e) => {
            log(&format!("⚠ [verify] 详情拉取失败（提交已成功 rrid={}）：{e}", result.rrid));
            false
        }
    };
    Ok(RunOutcome { result, obs_ok, detail_ok })
}

/// AI 提交流（UI 线程用）。
pub fn run_ai_submit(
    client: &mut ApiClient,
    sport_id: i64,
    mode: super::ai::AiMode,
    log: &mut dyn FnMut(&str),
) -> Result<Value, String> {
    log(&format!("[ai] 提交 sportId={sport_id} mode={mode:?}…"));
    let biz = super::ai::upload(client, sport_id, mode, None)?;
    log("√ [ai] 提交成功");
    Ok(biz)
}

/// AI 列表（UI 线程用）。
pub fn run_ai_list(client: &mut ApiClient, log: &mut dyn FnMut(&str)) -> Result<Vec<super::ai::AiSport>, String> {
    log("[ai] 拉取项目列表…");
    let list = super::ai::fetch_list(client)?;
    log(&format!("√ [ai] {} 个项目", list.len()));
    Ok(list)
}

/// 记录列表（UI 线程用）。
pub fn run_records(client: &mut ApiClient, log: &mut dyn FnMut(&str)) -> Result<Vec<super::records::RecordRow>, String> {
    log("[records] 拉取跑步记录…");
    let rows = super::records::fetch_records(client)?;
    log(&format!("√ [records] {} 条记录", rows.len()));
    Ok(rows)
}

/// 运行模式探测：枚举 runMode 拉取策略并打印完整解密响应（定位「计分跑 / 自由跑」参数用）。
pub fn run_probe(client: &mut ApiClient, log: &mut dyn FnMut(&str)) {
    log("[probe] 枚举 runMode 0..=6 的跑步策略…");
    for rm in 0i64..=6 {
        match super::policy::fetch_policy_raw(client, rm) {
            Ok(v) => log(&format!(
                "[probe] runMode={rm} → {}",
                serde_json::to_string(&v).unwrap_or_default()
            )),
            Err(e) => log(&format!("[probe] runMode={rm} → Err: {e}")),
        }
    }
    log("[probe] 枚举 sportType 1..=6（用 totalDis=100 让服务器业务拒绝，不产生有效记录）…");
    let sess = match client.login.clone() {
        Some(s) => s,
        None => { log("[probe] 未登录，跳过 sportType 枚举"); return; }
    };
    let now = crate::crypto::envelope::now_ms();
    let unid = sess.unid.parse::<i64>().unwrap_or(0);
    for st in 1i64..=6 {
        let body = serde_json::json!({
            "allLocJson": "",
            "sportType": st,
            "policy": 0,
            "totalTime": 60,
            "startTime": now - 60000,
            "stopTime": now,
            "getPrize": false,
            "status": 0,
            "uuid": format!("PROBE{}{}", st, now),
            "uid": sess.uid,
            "selectedUnid": unid,
            "selRunTime": 60,
            "selDistance": 1000,
            "totalDis": 100,
            "speed": 1000,
            "validDis": 100,
            "validTime": 60,
            "complete": true,
            "unCompleteReason": 0,
            "calorie": 5,
            "totalSteps": 60,
            "avgStepFreq": 60,
            "useMobilityTools": 0,
            "faceCheck": 0,
            "totalAscent": 0,
            "avgPower": 100,
            "speedPerTenSec": [],
            "stepsPerTenSec": [],
            "isUpload": false,
            "more": false,
            "latitude": 30.0,
            "longitude": 120.0,
            "maxRunTime": 0,
            "minSteps": 0,
            "errorCode": 0,
            "geeToken": "",
            "unauthorized": 0,
            "themeId": 0,
            "goalId": (),
            "address": "probe",
        });
        let body_val = Value::Object(body.as_object().unwrap().clone());
        let sig = crate::crypto::sign::signature(&body_val, false);
        let orig = crate::crypto::sign::original_sign(&body_val, false);
        let mut obj = body.as_object().unwrap().clone();
        obj.insert("signature".into(), Value::String(sig));
        obj.insert("originalSign".into(), Value::String(orig));
        match client.envelope_request(
            "POST",
            &format!("{}/api/v70260/runnings/save/record", super::model::HOST),
            &Value::Object(obj).to_string(),
            crate::crypto::header::UA_ANDROID,
            &[],
        ) {
            Ok(resp) => {
                let biz = resp.decrypted.map(|d| d.business).unwrap_or(Value::Null);
                log(&format!("[probe] sportType={st} → {}", serde_json::to_string(&biz).unwrap_or_default()));
            }
            Err(e) => log(&format!("[probe] sportType={st} → Err: {e}")),
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    log("[probe] semester 对比：runMode=1 vs runMode=2 的个人完成度…");
    let _ = super::semester::query(client, log);
}

use chrono::TimeZone as _;

/// 最近一次详情验证的完整响应（达标判定明细在 reasonList）。
pub static VERIFY_DETAIL: std::sync::Mutex<Option<Value>> = std::sync::Mutex::new(None);
