//! Единая точка входа для интерфейса: `dispatch("команда", {аргументы})`.
//! Интерфейс и Tauri-обёртка не знают о внутренностях ядра — только об этих командах.

use std::sync::{Mutex, MutexGuard};

use chrono::{NaiveDate, NaiveDateTime};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::model::{Category, Note, Reminder, ScheduleEntry};
use crate::online;
use crate::store::Store;
use crate::sync_net;
use crate::timefmt::dt;

/// Команды, после которых нужно пересчитать будильники и виджеты.
pub const MUTATING: &[&str] = &[
    "categories.save",
    "categories.delete",
    "reminders.save",
    "reminders.delete",
    "reminders.archive",
    "schedule.save",
    "schedule.delete",
    "complete",
    "skip",
    "undo",
    "settings.set",
];

fn lock(m: &Mutex<Store>) -> Result<MutexGuard<'_, Store>> {
    m.lock().map_err(|_| Error::Invalid("внутренняя ошибка: блокировка хранилища".into()))
}

fn get<T: DeserializeOwned>(args: &Value, key: &str) -> Result<T> {
    let v = args.get(key).cloned().unwrap_or(Value::Null);
    serde_json::from_value(v).map_err(|e| Error::Invalid(format!("параметр «{key}»: {e}")))
}

fn get_dt(args: &Value, key: &str) -> Result<NaiveDateTime> {
    let s: String = get(args, key)?;
    dt::parse(&s).map_err(Error::Invalid)
}

fn to_v<T: Serialize>(t: T) -> Result<Value> {
    Ok(serde_json::to_value(t)?)
}

/// Выполняет команду интерфейса. `now` — локальное время устройства.
pub fn dispatch(store: &Mutex<Store>, now: NaiveDateTime, cmd: &str, args: Value) -> Result<Value> {
    if cmd == "security.status" { return lock(store)?.lock_status(); }
    if cmd == "security.unlock" { return lock(store)?.unlock_app(args["password"].as_str().unwrap_or("")); }
    if cmd == "security.password" { return lock(store)?.set_password(args["current"].as_str().unwrap_or(""), args["password"].as_str().unwrap_or("")); }
    lock(store)?.require_unlocked()?;
    if let Some(rest) = cmd.strip_prefix("pairing.") { return crate::pairing::dispatch(store,rest,&args); }
    if let Some(rest) = cmd.strip_prefix("shared.") { return lock(store)?.shared_dispatch(rest,&args); }
    if let Some(rest) = cmd.strip_prefix("dropbox.") {
        let result = crate::dropbox::dispatch(store, rest, &args);
        if result.is_ok() && matches!(rest,"category_run"|"attach") { sync_net::notify_changed(); }
        return result;
    }
    if let Some(rest) = cmd.strip_prefix("online.") {
        return online_cmd(store, now, rest, &args);
    }
    if let Some(rest) = cmd.strip_prefix("sync.") {
        return sync_cmd(store, rest, &args);
    }
    let s = lock(store)?;
    let undoable = crate::features::UNDOABLE.contains(&cmd);
    let before = if undoable { Some(s.action_snapshot()?) } else { None };
    if undoable { s.conn.execute_batch("SAVEPOINT ui_action")?; }
    let result = (|| { match cmd {
        "security.lock" => { s.unlocked.set(false); s.lock_status() },
        "history.status" => s.undo_status(),
        "history.undo" => s.undo_action(),
        "icons.get" => Ok(s.private_get("icons")?.unwrap_or(json!({}))),
        "icons.import" => { let icons=crate::features::import_icon_pack(args["data"].as_str().unwrap_or(""))?; s.private_put("icons",&icons)?; Ok(icons) },
        "icons.reset" => { s.delete_setting("private.icons")?; Ok(Value::Null) },
        "icons.image" => { let bytes=crate::sync::b64_decode(args["data"].as_str().unwrap_or(""))?; Ok(json!({"icon":crate::features::optimize_icon(&bytes)?})) },
        "attachments.prepare" => {
            use sha2::{Digest,Sha256};
            let data=args["data"].as_str().unwrap_or("");
            if data.len()>14*1024*1024 { return Err(crate::features::invalid("Файл больше 10 МБ")); }
            let bytes=crate::sync::b64_decode(data)?;
            let file=crate::model::Attachment { id:crate::sync::hex(&Sha256::digest(bytes)), name:args["name"].as_str().unwrap_or("").into(), mime:args["mime"].as_str().unwrap_or("application/octet-stream").into(), data:data.into() };
            crate::features::validate_attachments(std::slice::from_ref(&file))?;
            to_v(file)
        },
        "backup.export" => Ok(json!({"text":s.backup_text()?,"encrypted":s.sync_config()?.code.is_some()})),
        "backup.restore" => s.restore_backup(args["text"].as_str().unwrap_or("")),
        "now" => to_v(dt::format(&now)),

        "categories.list" => to_v(s.list_categories()?),
        "categories.save" => {
            let c: Category = get(&args, "category")?;
            to_v(s.save_category(&c)?)
        }
        "categories.delete" => {
            s.delete_category(get(&args, "id")?)?;
            Ok(Value::Null)
        }

        "reminders.list" => {
            let archived: Option<bool> = get(&args, "archived")?;
            to_v(s.list_reminders(archived.unwrap_or(false))?)
        }
        "reminders.save" => {
            let r: Reminder = get(&args, "reminder")?;
            to_v(s.save_reminder(&r)?)
        }
        "reminders.delete" => {
            s.delete_reminder(get(&args, "id")?)?;
            Ok(Value::Null)
        }
        "reminders.archive" => {
            let archived: bool = get(&args, "archived")?;
            s.set_archived(get(&args, "id")?, archived)?;
            Ok(Value::Null)
        }

        "schedule.list" => to_v(s.list_schedule()?),
        "schedule.save" => {
            let e: ScheduleEntry = get(&args, "entry")?;
            to_v(s.save_schedule(&e)?)
        }
        "schedule.delete" => {
            s.delete_schedule(get(&args, "id")?)?;
            Ok(Value::Null)
        }

        "notes.list" => {
            let query: Option<String> = get(&args, "query")?;
            let category_id: Option<i64> = get(&args, "category_id")?;
            to_v(s.list_notes(query.as_deref(), category_id)?)
        }
        "notes.save" => {
            let n: Note = get(&args, "note")?;
            to_v(s.save_note(&n, now)?)
        }
        "notes.delete" => {
            s.delete_note(get(&args, "id")?)?;
            Ok(Value::Null)
        }

        "day" => {
            let date: NaiveDate = get(&args, "date")?;
            to_v(s.day_view(date, now)?)
        }
        "week" => {
            let date: NaiveDate = get(&args, "date")?;
            to_v(s.week_view(date, now)?)
        }
        "pending" => to_v(s.pending(now)?),
        "postpone" => {
            let id=get(&args,"reminder_id")?;
            let at=get_dt(&args,"at")?;
            let until=get_dt(&args,"until")?;
            if until <= now { return Err(crate::features::invalid("Новый срок должен быть в будущем")); }
            let mut r=s.get_reminder(id)?;
            let one_time=matches!(r.repeat,crate::model::Repeat::None);
            r.id=0; r.start=until; r.repeat=crate::model::Repeat::None; r.repeat_until=None; r.archived=false;
            let saved=s.save_reminder(&r)?;
            s.skip(id,at,now)?;
            if one_time { s.set_archived(id,true)?; }
            to_v(saved)
        },

        "complete" => {
            let id: i64 = get(&args, "reminder_id")?;
            to_v(s.complete(id, get_dt(&args, "at")?, now)?)
        }
        "skip" => {
            let id: i64 = get(&args, "reminder_id")?;
            s.skip(id, get_dt(&args, "at")?, now)?;
            Ok(Value::Null)
        }
        "undo" => {
            let id: i64 = get(&args, "reminder_id")?;
            s.undo(id, get_dt(&args, "at")?)?;
            Ok(Value::Null)
        }

        // Для системной интеграции: что планировать в будильнике и что рисовать в виджетах.
        "alarms" => {
            let days: Option<i64> = get(&args, "days")?;
            let limit: Option<usize> = get(&args, "limit")?;
            to_v(s.upcoming_alarms(now, days.unwrap_or(30).clamp(0, 60), limit.unwrap_or(200).min(400))?)
        }
        "widget" => to_v(s.widget_snapshot(now)?),

        "stats" => to_v(s.stats(now)?),
        "achievements" => to_v(s.achievements(now)?),

        "settings.get" => to_v(s.settings_with_defaults()?),
        "settings.set" => {
            let key: String = get(&args, "key")?;
            let value: String = get(&args, "value")?;
            if key.is_empty() || key.len() > 64 || key == "online.token" || key.starts_with("private.") || key.starts_with("dropbox.") || value.len() > 4096 {
                return Err(Error::Invalid("недопустимый ключ настройки".into()));
            }
            s.set_setting(&key, &value)?;
            Ok(Value::Null)
        }

        other => Err(Error::Invalid(format!("неизвестная команда: {other}"))),
    } })();
    let result = result.and_then(|v| { if let Some(before)=&before { s.remember_action(before,cmd)?; } Ok(v) });
    if undoable {
        if result.is_ok() { s.conn.execute_batch("RELEASE ui_action")?; }
        else { s.conn.execute_batch("ROLLBACK TO ui_action; RELEASE ui_action")?; }
    }
    result
}

/// Сетевые команды: хранилище блокируется только на время чтения/записи, не на время запроса.
fn online_cmd(store: &Mutex<Store>, now: NaiveDateTime, cmd: &str, args: &Value) -> Result<Value> {
    match cmd {
        "register" => {
            let nickname: String = get(args, "nickname")?;
            online::register(store, &nickname)
        }
        "push" => online::push(store, now),
        "leaderboard" => {
            let period: Option<String> = get(args, "period")?;
            let room_only: Option<bool> = get(args, "room_only")?;
            online::leaderboard(store, period.as_deref().unwrap_or("week"), room_only.unwrap_or(false))
        }
        "logout" => {
            let s = lock(store)?;
            s.delete_setting("online.token")?;
            Ok(json!({ "ok": true }))
        }
        other => Err(Error::Invalid(format!("неизвестная команда: online.{other}"))),
    }
}

/// Команды синхронизации. Сетевые не держат блокировку хранилища на время запроса.
fn sync_cmd(store: &Mutex<Store>, cmd: &str, args: &Value) -> Result<Value> {
    match cmd {
        "status" => {
            let mut v = lock(store)?.sync_status()?;
            if let Some(o) = v.as_object_mut() {
                o.insert("lan_addr".into(), json!(sync_net::local_address()));
            }
            Ok(v)
        }
        "create" => {
            let code = lock(store)?.sync_create_group()?;
            sync_net::wake();
            Ok(json!({ "code": code }))
        }
        "join" => {
            let code: String = get(args, "code")?;
            let code = lock(store)?.sync_join_group(&code)?;
            sync_net::wake();
            Ok(json!({ "code": code }))
        }
        "leave" => {
            lock(store)?.sync_leave_group()?;
            Ok(Value::Null)
        }
        "config" => {
            let ntfy: bool = get(args, "ntfy")?;
            let lan: bool = get(args, "lan")?;
            let server: Option<String> = get(args, "server")?;
            lock(store)?.sync_set_transports(ntfy, lan, server.as_deref().unwrap_or(""))?;
            sync_net::wake();
            Ok(Value::Null)
        }
        "export" => Ok(json!({ "text": lock(store)?.sync_export_text()? })),
        "import" => {
            let text: String = get(args, "text")?;
            let stats = lock(store)?.sync_import_text(&text)?;
            sync_net::notify_changed();
            to_v(stats)
        }
        "run" => sync_net::run_now(store),
        "lan_connect" => {
            let addr: String = get(args, "addr")?;
            sync_net::connect_addr(store, &addr)
        }
        other => Err(Error::Invalid(format!("неизвестная команда: sync.{other}"))),
    }
}
