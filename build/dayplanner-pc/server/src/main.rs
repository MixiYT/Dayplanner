//! Сервер рейтинга пользователей.
//!
//! Запуск: `cargo run -p planner-server --release`
//! Переменные окружения: PORT (по умолчанию 8787), DATA_FILE (leaderboard.json).
//!
//! Важно: очки считает клиент, сервер их только хранит и сравнивает. Простая защита
//! от накрутки есть (ограничен рост за один запрос), но это не замена честной игре.
//! Для публичного запуска поставьте перед сервером HTTPS-прокси (Caddy, nginx).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Datelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

const MAX_USERS: usize = 100_000;
const MAX_JUMP_PER_UPDATE: i64 = 5_000;
const MAX_POINTS: i64 = 10_000_000;

#[derive(Clone, Serialize, Deserialize)]
struct User {
    id: String,
    nickname: String,
    token: String,
    points: i64,
    level: u32,
    streak: u32,
    done: u32,
    achievements: Vec<String>,
    room: Option<String>,
    /// Неделя, к которой относится `week_start_points`, например "2026-W44".
    week_key: String,
    /// Очки на начало недели: недельный счёт = points - week_start_points.
    week_start_points: i64,
    updated: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Db {
    users: Vec<User>,
}

struct AppState {
    db: Mutex<Db>,
    path: PathBuf,
}

type Shared = Arc<AppState>;
type ApiError = (StatusCode, Json<Value>);
type ApiResult = Result<Json<Value>, ApiError>;

fn err(code: StatusCode, msg: &str) -> ApiError {
    (code, Json(json!({ "error": msg })))
}

fn week_key() -> String {
    let w = Utc::now().iso_week();
    format!("{}-W{:02}", w.year(), w.week())
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let v = headers.get("authorization")?.to_str().ok()?;
    v.strip_prefix("Bearer ").map(|s| s.trim().to_string())
}

fn normalize_room(room: &Option<String>) -> Option<String> {
    room.as_ref()
        .map(|r| r.trim().to_lowercase())
        .filter(|r| !r.is_empty())
        .map(|r| r.chars().take(32).collect())
}

fn save(state: &AppState, db: &Db) {
    let tmp = state.path.with_extension("tmp");
    if let Ok(text) = serde_json::to_string(db) {
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &state.path);
        }
    }
}

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, Db>, ApiError> {
    state
        .db
        .lock()
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "внутренняя ошибка"))
}

// ---------- регистрация ----------

#[derive(Deserialize)]
struct RegisterReq {
    nickname: String,
}

async fn register(State(state): State<Shared>, Json(req): Json<RegisterReq>) -> ApiResult {
    let nickname = req.nickname.trim().to_string();
    let len = nickname.chars().count();
    if !(2..=24).contains(&len) || nickname.chars().any(|c| c.is_control()) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "Ник: от 2 до 24 символов"));
    }
    let mut db = lock(&state)?;
    if db.users.len() >= MAX_USERS {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "Сервер переполнен"));
    }
    if db.users.iter().any(|u| u.nickname.to_lowercase() == nickname.to_lowercase()) {
        return Err(err(StatusCode::CONFLICT, "Этот ник уже занят"));
    }
    let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let user = User {
        id: uuid::Uuid::new_v4().simple().to_string(),
        nickname: nickname.clone(),
        token: token.clone(),
        points: 0,
        level: 1,
        streak: 0,
        done: 0,
        achievements: Vec::new(),
        room: None,
        week_key: week_key(),
        week_start_points: 0,
        updated: String::new(),
    };
    let id = user.id.clone();
    db.users.push(user);
    save(&state, &db);
    Ok(Json(json!({ "user_id": id, "nickname": nickname, "token": token })))
}

// ---------- обновление очков ----------

#[derive(Deserialize)]
struct ScoreReq {
    points: i64,
    level: u32,
    streak: u32,
    done: u32,
    #[serde(default)]
    achievements: Vec<String>,
    room: Option<String>,
}

async fn score(State(state): State<Shared>, headers: HeaderMap, Json(req): Json<ScoreReq>) -> ApiResult {
    let token = bearer(&headers).ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Нужна авторизация"))?;
    if req.points < 0 || req.points > MAX_POINTS {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "Недопустимое число очков"));
    }
    let mut db = lock(&state)?;
    let user = db
        .users
        .iter_mut()
        .find(|u| u.token == token)
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Неизвестный пользователь"))?;

    if !user.updated.is_empty() && req.points - user.points > MAX_JUMP_PER_UPDATE {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "Слишком резкий рост очков"));
    }

    let current_week = week_key();
    if user.week_key != current_week {
        user.week_key = current_week;
        user.week_start_points = user.points;
    }
    user.points = req.points;
    user.level = req.level.min(10_000);
    user.streak = req.streak.min(100_000);
    user.done = req.done;
    user.achievements = req.achievements.into_iter().take(64).collect();
    user.room = normalize_room(&req.room);
    user.updated = Utc::now().to_rfc3339();
    let weekly = (user.points - user.week_start_points).max(0);
    let result = json!({ "points": user.points, "weekly": weekly });
    save(&state, &db);
    Ok(Json(result))
}

// ---------- рейтинг ----------

#[derive(Deserialize)]
struct BoardQuery {
    period: Option<String>,
    room: Option<String>,
    limit: Option<usize>,
}

fn user_score(u: &User, weekly: bool, current_week: &str) -> i64 {
    if !weekly {
        u.points
    } else if u.week_key == current_week {
        (u.points - u.week_start_points).max(0)
    } else {
        // Неделя сменилась, а обновлений ещё не было: за эту неделю очков нет.
        0
    }
}

async fn leaderboard(State(state): State<Shared>, headers: HeaderMap, Query(q): Query<BoardQuery>) -> ApiResult {
    let weekly = q.period.as_deref() != Some("all");
    let room = normalize_room(&q.room);
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    let token = bearer(&headers);
    let current_week = week_key();

    let db = lock(&state)?;
    let mut rows: Vec<(&User, i64)> = db
        .users
        .iter()
        .filter(|u| u.updated.len() > 0)
        .filter(|u| room.is_none() || u.room == room)
        .map(|u| (u, user_score(u, weekly, &current_week)))
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.nickname.cmp(&b.0.nickname)));

    let entry = |rank: usize, u: &User, score: i64| {
        json!({
            "rank": rank,
            "nickname": u.nickname,
            "points": score,
            "level": u.level,
            "streak": u.streak,
            "achievements": u.achievements.len(),
            "me": token.as_deref() == Some(u.token.as_str()),
        })
    };

    let entries: Vec<Value> = rows
        .iter()
        .take(limit)
        .enumerate()
        .map(|(i, (u, s))| entry(i + 1, u, *s))
        .collect();
    let me = rows
        .iter()
        .enumerate()
        .find(|(_, (u, _))| token.as_deref() == Some(u.token.as_str()))
        .map(|(i, (u, s))| entry(i + 1, u, *s));

    let period_name = if weekly { "week" } else { "all" };
    Ok(Json(json!({
        "period": period_name,
        "total": rows.len(),
        "entries": entries,
        "me": me,
    })))
}

async fn me(State(state): State<Shared>, headers: HeaderMap) -> ApiResult {
    let token = bearer(&headers).ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Нужна авторизация"))?;
    let db = lock(&state)?;
    let u = db
        .users
        .iter()
        .find(|u| u.token == token)
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Неизвестный пользователь"))?;
    Ok(Json(json!({
        "nickname": u.nickname,
        "points": u.points,
        "level": u.level,
        "streak": u.streak,
        "room": u.room,
    })))
}

async fn health() -> &'static str {
    "ok"
}

#[tokio::main]
async fn main() {
    let path = PathBuf::from(std::env::var("DATA_FILE").unwrap_or_else(|_| "leaderboard.json".into()));
    let db: Db = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let state: Shared = Arc::new(AppState { db: Mutex::new(db), path });

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/register", post(register))
        .route("/api/score", post(score))
        .route("/api/leaderboard", get(leaderboard))
        .route("/api/me", get(me))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8787);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("не удалось занять порт");
    println!("Сервер рейтинга: http://{addr}");
    axum::serve(listener, app).await.expect("ошибка сервера");
}
