use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::{Error, Result};
use crate::model::*;
use crate::timefmt::{dt, tm};

/// Хранилище на SQLite. Одно и то же на Android и на ПК.
pub struct Store {
    pub(crate) conn: Connection,
    /// Подмена часов для тестов (миллисекунды); в работе всегда `None`.
    pub(crate) clock: Cell<Option<i64>>,
    /// Последняя выданная отметка времени: отметки строго возрастают.
    pub(crate) last_stamp: Cell<i64>,
    pub(crate) private_key: [u8; 32],
    pub(crate) unlocked: Cell<bool>,
    pub(crate) unlock_failures: Cell<u32>,
    pub(crate) unlock_after: Cell<i64>,
}

const SCHEMA: &str = "
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS categories(
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    color TEXT NOT NULL,
    icon TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS reminders(
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    icon TEXT NOT NULL,
    category_id INTEGER,
    start_at TEXT NOT NULL,
    alert TEXT NOT NULL,
    repeat_rule TEXT NOT NULL,
    repeat_until TEXT,
    grace_minutes INTEGER NOT NULL DEFAULT 60,
    archived INTEGER NOT NULL DEFAULT 0,
    attachments TEXT NOT NULL DEFAULT '[]'
);
CREATE TABLE IF NOT EXISTS schedule_entries(
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    weekday INTEGER,
    on_date TEXT,
    start_time TEXT NOT NULL,
    end_time TEXT NOT NULL,
    color TEXT,
    icon TEXT NOT NULL,
    location TEXT NOT NULL DEFAULT '',
    category_id INTEGER
);
CREATE TABLE IF NOT EXISTS completions(
    reminder_id INTEGER NOT NULL,
    at TEXT NOT NULL,
    kind TEXT NOT NULL,
    completed_at TEXT NOT NULL,
    points INTEGER NOT NULL,
    PRIMARY KEY(reminder_id, at)
);
CREATE TABLE IF NOT EXISTS unlocked(
    id TEXT PRIMARY KEY,
    at TEXT NOT NULL,
    points INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS notes(
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    color TEXT,
    pinned INTEGER NOT NULL DEFAULT 0,
    category_id INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    attachments TEXT NOT NULL DEFAULT '[]'
);
CREATE TABLE IF NOT EXISTS sync_meta(
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    uid TEXT NOT NULL,
    modified INTEGER NOT NULL,
    deleted INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(kind, key)
);
CREATE UNIQUE INDEX IF NOT EXISTS sync_meta_uid ON sync_meta(kind, uid);
CREATE TABLE IF NOT EXISTS kv(
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

/// Настройки по умолчанию (оформление и т. п.).
pub const DEFAULT_SETTINGS: &[(&str, &str)] = &[
    ("theme", "material"),
    ("accent", "#6750A4"),
    ("mode", "auto"),
    ("bg", "aurora"),
    ("bg_intensity", "60"),
    ("online.server", ""),
    ("online.room", ""),
    ("online.nickname", ""),
];

/// Категории по умолчанию. Постоянные идентификаторы нужны, чтобы при синхронизации
/// стандартные категории на двух устройствах слились, а не удвоились.
pub const SEED_CATEGORIES: [(&str, &str, &str, &str); 5] = [
    ("Работа", "#4285F4", "💼", "seed-work"),
    ("Учёба", "#9C27B0", "🎓", "seed-study"),
    ("Здоровье", "#E53935", "❤️", "seed-health"),
    ("Дом", "#43A047", "🏠", "seed-home"),
    ("Личное", "#FB8C00", "⭐", "seed-personal"),
];

/// Ключ записи выполнения в таблице метаданных синхронизации.
pub(crate) fn comp_meta_key(id: i64, at: &str) -> String {
    format!("{id}|{at}")
}

pub fn comp_key(id: i64, at: &NaiveDateTime) -> (i64, String) {
    (id, dt::format(at))
}

fn conv<T>(r: std::result::Result<T, String>) -> rusqlite::Result<T> {
    r.map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into()))
}

fn parse_date(s: &str) -> std::result::Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| e.to_string())
}

const REMINDER_COLS: &str =
    "id,title,notes,icon,category_id,start_at,alert,repeat_rule,repeat_until,grace_minutes,archived,attachments";

fn row_reminder(row: &Row) -> rusqlite::Result<Reminder> {
    let start: String = row.get(5)?;
    let alert: String = row.get(6)?;
    let repeat: String = row.get(7)?;
    let until: Option<String> = row.get(8)?;
    let archived: i64 = row.get(10)?;
    let repeat_until = match until {
        Some(s) => Some(conv(parse_date(&s))?),
        None => None,
    };
    Ok(Reminder {
        id: row.get(0)?,
        title: row.get(1)?,
        notes: row.get(2)?,
        icon: row.get(3)?,
        category_id: row.get(4)?,
        start: conv(dt::parse(&start))?,
        alert: AlertKind::parse(&alert),
        repeat: serde_json::from_str(&repeat).unwrap_or(Repeat::None),
        repeat_until,
        grace_minutes: row.get(9)?,
        archived: archived != 0,
        attachments: serde_json::from_str(&row.get::<_, String>(11)?).map_err(|e| rusqlite::Error::FromSqlConversionFailure(11, rusqlite::types::Type::Text, e.into()))?,
    })
}

const NOTE_COLS: &str = "id,title,body,color,pinned,category_id,created_at,updated_at,attachments,icon";

fn row_note(row: &Row) -> rusqlite::Result<Note> {
    let pinned: i64 = row.get(4)?;
    let created: String = row.get(6)?;
    let updated: String = row.get(7)?;
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        body: row.get(2)?,
        icon: row.get(9)?,
        color: row.get(3)?,
        pinned: pinned != 0,
        category_id: row.get(5)?,
        created: conv(dt::parse(&created))?,
        updated: conv(dt::parse(&updated))?,
        attachments: serde_json::from_str(&row.get::<_, String>(8)?).map_err(|e| rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, e.into()))?,
    })
}

const SCHEDULE_COLS: &str = "id,title,weekday,on_date,start_time,end_time,color,icon,location,category_id";

fn row_schedule(row: &Row) -> rusqlite::Result<ScheduleEntry> {
    let weekday: Option<i64> = row.get(2)?;
    let on_date: Option<String> = row.get(3)?;
    let st: String = row.get(4)?;
    let en: String = row.get(5)?;
    let date = match on_date {
        Some(s) => Some(conv(parse_date(&s))?),
        None => None,
    };
    Ok(ScheduleEntry {
        id: row.get(0)?,
        title: row.get(1)?,
        weekday: weekday.map(|w| w as u8),
        date,
        start: conv(tm::parse(&st))?,
        end: conv(tm::parse(&en))?,
        color: row.get(6)?,
        icon: row.get(7)?,
        location: row.get(8)?,
        category_id: row.get(9)?,
    })
}

fn row_completion(row: &Row) -> rusqlite::Result<Completion> {
    let at: String = row.get(1)?;
    let kind: String = row.get(2)?;
    let done_at: String = row.get(3)?;
    Ok(Completion {
        reminder_id: row.get(0)?,
        at: conv(dt::parse(&at))?,
        kind: if kind == "skipped" { CompletionKind::Skipped } else { CompletionKind::Done },
        completed_at: conv(dt::parse(&done_at))?,
        points: row.get(4)?,
    })
}

impl Store {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Store> {
        let path = path.as_ref();
        let dir: PathBuf = path.parent().unwrap_or_else(|| Path::new(".")).into();
        Store::init(Connection::open(path)?, Some(&dir))
    }

    pub fn open_in_memory() -> Result<Store> {
        Store::init(Connection::open_in_memory()?, None)
    }

    fn init(conn: Connection, dir: Option<&Path>) -> Result<Store> {
        conn.execute_batch(SCHEMA)?;
        let has_icon: i64 = conn.query_row("SELECT COUNT(*) FROM pragma_table_info('notes') WHERE name='icon'", [], |r|r.get(0))?;
        if has_icon == 0 { conn.execute("ALTER TABLE notes ADD COLUMN icon TEXT NOT NULL DEFAULT 'builtin:note'", [])?; }
        crate::shared::init(&conn)?;
        for table in ["reminders", "notes"] {
            let has: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name='attachments'"), [], |r| r.get(0))?;
            if has == 0 { conn.execute(&format!("ALTER TABLE {table} ADD COLUMN attachments TEXT NOT NULL DEFAULT '[]'"), [])?; }
        }
        let store = Store { conn, clock: Cell::new(None), last_stamp: Cell::new(0), private_key: crate::features::load_private_key(dir)?, unlocked: Cell::new(false), unlock_failures: Cell::new(0), unlock_after: Cell::new(0) };
        store.seed_categories()?;
        // Записи, созданные до появления синхронизации, получают постоянные идентификаторы.
        store.ensure_meta()?;
        Ok(store)
    }

    fn seed_categories(&self) -> Result<()> {
        let n: i64 = self.conn.query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))?;
        // Заново стандартные категории не создаём, если пользователь удалил их все (остались надгробия).
        let had: i64 = self.conn.query_row("SELECT COUNT(*) FROM sync_meta WHERE kind='category'", [], |r| r.get(0))?;
        if n == 0 && had == 0 {
            for (name, color, icon, _uid) in SEED_CATEGORIES {
                self.conn.execute(
                    "INSERT INTO categories(name,color,icon) VALUES (?1,?2,?3)",
                    params![name, color, icon],
                )?;
            }
        }
        Ok(())
    }

    // ---------- категории ----------

    pub fn list_categories(&self) -> Result<Vec<Category>> {
        let mut st = self.conn.prepare("SELECT id,name,color,icon FROM categories ORDER BY id")?;
        let rows = st.query_map([], |r| {
            Ok(Category { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, icon: r.get(3)? })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn save_category(&self, c: &Category) -> Result<Category> {
        crate::features::validate_icon(&c.icon)?;
        let name = c.name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("Название категории не может быть пустым".into()));
        }
        let mut out = c.clone();
        out.name = name.to_string();
        if c.id == 0 {
            let id = self.next_id("category", "categories")?;
            self.conn.execute(
                "INSERT INTO categories(id,name,color,icon) VALUES (?1,?2,?3,?4)",
                params![id, out.name, out.color, out.icon],
            )?;
            out.id = id;
        } else {
            let n = self.conn.execute(
                "UPDATE categories SET name=?1,color=?2,icon=?3 WHERE id=?4",
                params![out.name, out.color, out.icon, out.id],
            )?;
            if n == 0 {
                return Err(Error::NotFound(format!("категория {}", c.id)));
            }
        }
        self.touch("category", &out.id.to_string())?;
        Ok(out)
    }

    pub fn delete_category(&self, id: i64) -> Result<()> {
        // Записи, у которых сбросится категория, тоже считаются изменёнными (для синхронизации).
        for (kind, table) in [("reminder", "reminders"), ("schedule", "schedule_entries"), ("note", "notes")] {
            let ids = self.ids_where_category(table, id)?;
            self.conn.execute(&format!("UPDATE {table} SET category_id=NULL WHERE category_id=?1"), params![id])?;
            for i in ids {
                self.touch(kind, &i.to_string())?;
            }
        }
        let n = self.conn.execute("DELETE FROM categories WHERE id=?1", params![id])?;
        if n > 0 {
            self.mark_deleted("category", &id.to_string())?;
        }
        Ok(())
    }

    // ---------- напоминания ----------

    pub fn list_reminders(&self, include_archived: bool) -> Result<Vec<Reminder>> {
        let sql = if include_archived {
            format!("SELECT {REMINDER_COLS} FROM reminders ORDER BY start_at")
        } else {
            format!("SELECT {REMINDER_COLS} FROM reminders WHERE archived=0 ORDER BY start_at")
        };
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map([], row_reminder)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn get_reminder(&self, id: i64) -> Result<Reminder> {
        let sql = format!("SELECT {REMINDER_COLS} FROM reminders WHERE id=?1");
        self.conn
            .query_row(&sql, params![id], row_reminder)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("напоминание {id}")))
    }

    pub fn save_reminder(&self, r: &Reminder) -> Result<Reminder> {
        r.validate()?;
        let repeat = serde_json::to_string(&r.repeat)?;
        let until = r.repeat_until.map(|d| d.format("%Y-%m-%d").to_string());
        let start = dt::format(&r.start);
        let archived: i64 = if r.archived { 1 } else { 0 };
        let attachments = serde_json::to_string(&r.attachments)?;
        let mut out = r.clone();
        out.title = r.title.trim().to_string();
        if r.id == 0 {
            let id = self.next_id("reminder", "reminders")?;
            self.conn.execute(
                "INSERT INTO reminders(id,title,notes,icon,category_id,start_at,alert,repeat_rule,repeat_until,grace_minutes,archived,attachments)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![id, out.title, r.notes, r.icon, r.category_id, start, r.alert.as_str(), repeat, until, r.grace_minutes, archived, attachments],
            )?;
            out.id = id;
        } else {
            let n = self.conn.execute(
                "UPDATE reminders SET title=?1,notes=?2,icon=?3,category_id=?4,start_at=?5,alert=?6,repeat_rule=?7,
                 repeat_until=?8,grace_minutes=?9,archived=?10,attachments=?12 WHERE id=?11",
                params![out.title, r.notes, r.icon, r.category_id, start, r.alert.as_str(), repeat, until, r.grace_minutes, archived, r.id, attachments],
            )?;
            if n == 0 {
                return Err(Error::NotFound(format!("напоминание {}", r.id)));
            }
        }
        self.touch("reminder", &out.id.to_string())?;
        Ok(out)
    }

    pub fn set_archived(&self, id: i64, archived: bool) -> Result<()> {
        let flag: i64 = if archived { 1 } else { 0 };
        let n = self.conn.execute("UPDATE reminders SET archived=?1 WHERE id=?2", params![flag, id])?;
        if n > 0 {
            self.touch("reminder", &id.to_string())?;
        }
        Ok(())
    }

    /// Удаляет напоминание. История выполнений (очки) сохраняется.
    pub fn delete_reminder(&self, id: i64) -> Result<()> {
        let n = self.conn.execute("DELETE FROM reminders WHERE id=?1", params![id])?;
        if n > 0 {
            self.mark_deleted("reminder", &id.to_string())?;
        }
        Ok(())
    }

    // ---------- заметки ----------

    /// Заметки: сначала закреплённые, затем по времени изменения (новые выше).
    /// `query` ищет в заголовке и тексте без учёта регистра (и для кириллицы тоже).
    pub fn list_notes(&self, query: Option<&str>, category_id: Option<i64>) -> Result<Vec<Note>> {
        let sql = format!("SELECT {NOTE_COLS} FROM notes ORDER BY pinned DESC, updated_at DESC, id DESC");
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map([], row_note)?;
        let q = query.map(|q| q.trim().to_lowercase()).filter(|q| !q.is_empty());
        let mut out = Vec::new();
        for r in rows {
            let n = r?;
            if let Some(c) = category_id {
                if n.category_id != Some(c) {
                    continue;
                }
            }
            if let Some(q) = &q {
                if !n.title.to_lowercase().contains(q.as_str()) && !n.body.to_lowercase().contains(q.as_str()) {
                    continue;
                }
            }
            out.push(n);
        }
        Ok(out)
    }

    pub fn get_note(&self, id: i64) -> Result<Note> {
        let sql = format!("SELECT {NOTE_COLS} FROM notes WHERE id=?1");
        self.conn
            .query_row(&sql, params![id], row_note)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("заметка {id}")))
    }

    /// Создаёт или обновляет заметку. Время создания сохраняется, время изменения ставится `now`.
    pub fn save_note(&self, n: &Note, now: NaiveDateTime) -> Result<Note> {
        n.validate()?;
        let mut out = n.clone();
        out.title = n.title.trim().to_string();
        out.color = n.color.clone().filter(|c| !c.is_empty());
        out.updated = now;
        let pinned: i64 = if n.pinned { 1 } else { 0 };
        if n.id == 0 {
            out.created = now;
            let id = self.next_id("note", "notes")?;
            self.conn.execute(
                "INSERT INTO notes(id,title,body,color,pinned,category_id,created_at,updated_at,attachments,icon) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![id, out.title, out.body, out.color, pinned, out.category_id, dt::format(&out.created), dt::format(&now), serde_json::to_string(&out.attachments)?, out.icon],
            )?;
            out.id = id;
        } else {
            out.created = self.get_note(n.id)?.created;
            self.conn.execute(
                "UPDATE notes SET title=?1,body=?2,color=?3,pinned=?4,category_id=?5,updated_at=?6,attachments=?8,icon=?9 WHERE id=?7",
                params![out.title, out.body, out.color, pinned, out.category_id, dt::format(&now), out.id, serde_json::to_string(&out.attachments)?, out.icon],
            )?;
        }
        self.touch("note", &out.id.to_string())?;
        Ok(out)
    }

    pub fn delete_note(&self, id: i64) -> Result<()> {
        let n = self.conn.execute("DELETE FROM notes WHERE id=?1", params![id])?;
        if n > 0 {
            self.mark_deleted("note", &id.to_string())?;
        }
        Ok(())
    }

    // ---------- расписание ----------

    pub fn list_schedule(&self) -> Result<Vec<ScheduleEntry>> {
        let sql = format!("SELECT {SCHEDULE_COLS} FROM schedule_entries ORDER BY start_time");
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map([], row_schedule)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn save_schedule(&self, e: &ScheduleEntry) -> Result<ScheduleEntry> {
        e.validate()?;
        let mut out = e.clone();
        out.title = e.title.trim().to_string();
        let weekday: Option<i64> = e.weekday.map(|w| w as i64);
        let on_date = e.date.map(|d| d.format("%Y-%m-%d").to_string());
        let st = tm::format(&e.start);
        let en = tm::format(&e.end);
        if e.id == 0 {
            let id = self.next_id("schedule", "schedule_entries")?;
            self.conn.execute(
                "INSERT INTO schedule_entries(id,title,weekday,on_date,start_time,end_time,color,icon,location,category_id)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![id, out.title, weekday, on_date, st, en, e.color, e.icon, e.location, e.category_id],
            )?;
            out.id = id;
        } else {
            let n = self.conn.execute(
                "UPDATE schedule_entries SET title=?1,weekday=?2,on_date=?3,start_time=?4,end_time=?5,color=?6,
                 icon=?7,location=?8,category_id=?9 WHERE id=?10",
                params![out.title, weekday, on_date, st, en, e.color, e.icon, e.location, e.category_id, e.id],
            )?;
            if n == 0 {
                return Err(Error::NotFound(format!("запись расписания {}", e.id)));
            }
        }
        self.touch("schedule", &out.id.to_string())?;
        Ok(out)
    }

    pub fn delete_schedule(&self, id: i64) -> Result<()> {
        let n = self.conn.execute("DELETE FROM schedule_entries WHERE id=?1", params![id])?;
        if n > 0 {
            self.mark_deleted("schedule", &id.to_string())?;
        }
        Ok(())
    }

    // ---------- выполнение ----------

    pub fn get_completion(&self, id: i64, at: &NaiveDateTime) -> Result<Option<Completion>> {
        Ok(self
            .conn
            .query_row(
                "SELECT reminder_id,at,kind,completed_at,points FROM completions WHERE reminder_id=?1 AND at=?2",
                params![id, dt::format(at)],
                row_completion,
            )
            .optional()?)
    }

    pub fn put_completion(&self, c: &Completion) -> Result<()> {
        let kind = match c.kind {
            CompletionKind::Done => "done",
            CompletionKind::Skipped => "skipped",
        };
        self.conn.execute(
            "INSERT OR REPLACE INTO completions(reminder_id,at,kind,completed_at,points) VALUES (?1,?2,?3,?4,?5)",
            params![c.reminder_id, dt::format(&c.at), kind, dt::format(&c.completed_at), c.points],
        )?;
        self.touch("completion", &comp_meta_key(c.reminder_id, &dt::format(&c.at)))?;
        Ok(())
    }

    pub fn delete_completion(&self, id: i64, at: &NaiveDateTime) -> Result<()> {
        let n = self.conn.execute(
            "DELETE FROM completions WHERE reminder_id=?1 AND at=?2",
            params![id, dt::format(at)],
        )?;
        if n > 0 {
            self.mark_deleted("completion", &comp_meta_key(id, &dt::format(at)))?;
        }
        Ok(())
    }

    /// Записи выполнения для срабатываний в промежутке [from, to).
    pub fn completions_between(
        &self,
        from: NaiveDateTime,
        to: NaiveDateTime,
    ) -> Result<HashMap<(i64, String), Completion>> {
        let mut st = self.conn.prepare(
            "SELECT reminder_id,at,kind,completed_at,points FROM completions WHERE at >= ?1 AND at < ?2",
        )?;
        let rows = st.query_map(params![dt::format(&from), dt::format(&to)], row_completion)?;
        let mut out = HashMap::new();
        for r in rows {
            let c = r?;
            out.insert(comp_key(c.reminder_id, &c.at), c);
        }
        Ok(out)
    }

    pub fn all_completions(&self) -> Result<Vec<Completion>> {
        let mut st = self
            .conn
            .prepare("SELECT reminder_id,at,kind,completed_at,points FROM completions ORDER BY at")?;
        let rows = st.query_map([], row_completion)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // ---------- достижения ----------

    /// id достижения → момент получения.
    pub fn unlocked_map(&self) -> Result<HashMap<String, String>> {
        let mut st = self.conn.prepare("SELECT id,at FROM unlocked")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = HashMap::new();
        for r in rows {
            let (id, at) = r?;
            out.insert(id, at);
        }
        Ok(out)
    }

    pub fn unlock(&self, id: &str, at: &NaiveDateTime, points: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO unlocked(id,at,points) VALUES (?1,?2,?3)",
            params![id, dt::format(at), points],
        )?;
        Ok(())
    }

    pub fn achievement_points(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COALESCE(SUM(points),0) FROM unlocked", [], |r| r.get(0))?)
    }

    // ---------- настройки ----------

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM kv WHERE key=?1", params![key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO kv(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        self.conn.execute("DELETE FROM kv WHERE key=?1", params![key])?;
        Ok(())
    }

    pub fn setting_or(&self, key: &str, default: &str) -> Result<String> {
        Ok(self.get_setting(key)?.unwrap_or_else(|| default.to_string()))
    }

    /// Все настройки с учётом значений по умолчанию (без секретов).
    pub fn settings_with_defaults(&self) -> Result<HashMap<String, String>> {
        let mut out: HashMap<String, String> =
            DEFAULT_SETTINGS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let mut st = self.conn.prepare("SELECT key,value FROM kv")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for r in rows {
            let (k, v) = r?;
            if k != "online.token" && !k.starts_with("private.") {
                out.insert(k, v);
            }
        }
        out.insert("online.registered".to_string(), self.get_setting("online.token")?.is_some().to_string());
        Ok(out)
    }
}
