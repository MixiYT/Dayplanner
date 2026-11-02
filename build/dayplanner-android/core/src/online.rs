//! Клиент сервера рейтинга. Блокирующие запросы (ureq): вызывайте не из UI-потока.

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use chrono::NaiveDateTime;
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::store::Store;

fn lock(m: &Mutex<Store>) -> Result<MutexGuard<'_, Store>> {
    m.lock().map_err(|_| Error::Invalid("внутренняя ошибка: блокировка хранилища".into()))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build()
}

fn net_err(e: ureq::Error) -> Error {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.get("error").and_then(|x| x.as_str()).map(|s| s.to_string()))
                .unwrap_or(body);
            Error::Net(format!("{code}: {msg}"))
        }
        other => Error::Net(other.to_string()),
    }
}

fn base_url(store: &Mutex<Store>) -> Result<String> {
    let s = lock(store)?;
    let url = s.setting_or("online.server", "")?;
    let url = url.trim().trim_end_matches('/').to_string();
    if url.is_empty() {
        return Err(Error::Invalid("Укажите адрес сервера рейтинга в настройках".into()));
    }
    Ok(url)
}

pub fn register(store: &Mutex<Store>, nickname: &str) -> Result<Value> {
    let nickname = nickname.trim();
    if nickname.chars().count() < 2 || nickname.chars().count() > 24 {
        return Err(Error::Invalid("Ник: от 2 до 24 символов".into()));
    }
    let url = base_url(store)?;
    let resp: Value = agent()
        .post(&format!("{url}/api/register"))
        .send_json(json!({ "nickname": nickname }))
        .map_err(net_err)?
        .into_json()
        .map_err(|e| Error::Net(e.to_string()))?;
    let token = resp
        .get("token")
        .and_then(|t| t.as_str())
        .ok_or_else(|| Error::Net("сервер не вернул токен".into()))?
        .to_string();
    let s = lock(store)?;
    s.set_setting("online.token", &token)?;
    s.set_setting("online.nickname", nickname)?;
    Ok(json!({ "nickname": nickname }))
}

/// Отправляет текущие очки, уровень, серию и достижения на сервер.
pub fn push(store: &Mutex<Store>, now: NaiveDateTime) -> Result<Value> {
    let url = base_url(store)?;
    let (token, room, stats, achievements) = {
        let s = lock(store)?;
        let token = s
            .get_setting("online.token")?
            .ok_or_else(|| Error::Invalid("Сначала зарегистрируйтесь в рейтинге".into()))?;
        let room = s.setting_or("online.room", "")?;
        let stats = s.stats(now)?;
        let ids: Vec<String> = s
            .achievements(now)?
            .into_iter()
            .filter(|a| a.unlocked)
            .map(|a| a.id)
            .collect();
        (token, room, stats, ids)
    };
    let room_value = if room.trim().is_empty() {
        Value::Null
    } else {
        Value::String(room.trim().to_string())
    };
    let body = json!({
        "points": stats.points,
        "level": stats.level,
        "streak": stats.current_streak,
        "done": stats.total_done,
        "achievements": achievements,
        "room": room_value,
    });
    agent()
        .post(&format!("{url}/api/score"))
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(body)
        .map_err(net_err)?
        .into_json()
        .map_err(|e| Error::Net(e.to_string()))
}

/// Есть ли смысл отправлять очки (пользователь зарегистрирован и указан сервер).
pub fn is_configured(store: &Mutex<Store>) -> bool {
    match lock(store) {
        Ok(s) => {
            s.get_setting("online.token").ok().flatten().is_some()
                && !s.setting_or("online.server", "").unwrap_or_default().trim().is_empty()
        }
        Err(_) => false,
    }
}

pub fn leaderboard(store: &Mutex<Store>, period: &str, use_room: bool) -> Result<Value> {
    let url = base_url(store)?;
    let (token, room) = {
        let s = lock(store)?;
        (s.get_setting("online.token")?, s.setting_or("online.room", "")?)
    };
    let period = if period == "all" { "all" } else { "week" };
    let mut req = agent().get(&format!("{url}/api/leaderboard")).query("period", period);
    if use_room && !room.trim().is_empty() {
        req = req.query("room", room.trim());
    }
    if let Some(t) = token {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    req.call().map_err(net_err)?.into_json().map_err(|e| Error::Net(e.to_string()))
}
