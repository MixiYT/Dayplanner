//! Синхронизация между устройствами без собственного сервера.
//!
//! Идея: у каждой записи (категория, напоминание, занятие, заметка, отметка выполнения) есть
//! постоянный идентификатор `uid` и время последнего изменения `modified` (миллисекунды).
//! Эти сведения лежат в таблице `sync_meta`, а не в самих таблицах, поэтому обычный код
//! приложения про синхронизацию почти ничего не знает: хранилище лишь вызывает `touch` и
//! `mark_deleted`. Два устройства обмениваются снимками (`Payload`), а слияние простое:
//! для каждой записи побеждает версия с большим `modified`; удаление — тоже запись («надгробие»).
//!
//! Снимки шифруются общим кодом группы (ChaCha20-Poly1305), поэтому их можно передавать через
//! любой посредник: публичный сервис ntfy, локальную сеть или обычный файл.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::model::*;
use crate::store::{comp_meta_key, Store, SEED_CATEGORIES};
use crate::timefmt::{dt, tm};

/// Счётчик локальных изменений: фоновая синхронизация смотрит на него, чтобы отправить правки сразу.
pub static CHANGES: AtomicU64 = AtomicU64::new(0);

/// Порядок применения записей: сначала то, на что другие ссылаются.
const KIND_ORDER: [&str; 5] = ["category", "reminder", "schedule", "note", "completion"];

/// Алфавит кода группы: без похожих символов (0/O, 1/I).
const CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const FILE_PREFIX: &str = "DPSYNC1:";
const MAX_IMPORT_BYTES: usize = 40 * 1024 * 1024;

fn table_of(kind: &str) -> Option<&'static str> {
    match kind {
        "category" => Some("categories"),
        "reminder" => Some("reminders"),
        "schedule" => Some("schedule_entries"),
        "note" => Some("notes"),
        _ => None,
    }
}

fn bad(msg: &str) -> Error {
    Error::Invalid(msg.to_string())
}

// ---------- случайные числа, шестнадцатеричная запись, base64 ----------

pub(crate) fn random_bytes(buf: &mut [u8]) {
    // Fail closed: predictable clock/counter fallbacks must not generate group
    // secrets or AEAD nonces if the OS random source fails.
    getrandom::getrandom(buf).expect("system random source unavailable");
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn random_hex(n_bytes: usize) -> String {
    let mut b = vec![0u8; n_bytes];
    random_bytes(&mut b);
    hex(&b)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for ch in data.chunks(3) {
        let b0 = ch[0] as u32;
        let b1 = *ch.get(1).unwrap_or(&0) as u32;
        let b2 = *ch.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64[((n >> 18) & 63) as usize] as char);
        out.push(B64[((n >> 12) & 63) as usize] as char);
        out.push(if ch.len() > 1 { B64[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if ch.len() > 2 { B64[(n & 63) as usize] as char } else { '=' });
    }
    out
}

pub(crate) fn b64_decode(s: &str) -> Result<Vec<u8>> {
    let mut vals: Vec<u8> = Vec::with_capacity(s.len());
    let mut pad = 0usize;
    for c in s.bytes() {
        match c {
            b'A'..=b'Z' => vals.push(c - b'A'),
            b'a'..=b'z' => vals.push(c - b'a' + 26),
            b'0'..=b'9' => vals.push(c - b'0' + 52),
            b'+' => vals.push(62),
            b'/' => vals.push(63),
            b'=' => pad += 1,
            b' ' | b'\n' | b'\r' | b'\t' => {}
            _ => return Err(bad("Повреждённые данные синхронизации")),
        }
    }
    if pad > 2 || (vals.len() + pad) % 4 != 0 {
        return Err(bad("Повреждённые данные синхронизации"));
    }
    let mut out = Vec::with_capacity(vals.len() / 4 * 3 + 3);
    for ch in vals.chunks(4) {
        if ch.len() < 2 {
            return Err(bad("Повреждённые данные синхронизации"));
        }
        let mut n = 0u32;
        for (i, v) in ch.iter().enumerate() {
            n |= (*v as u32) << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if ch.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if ch.len() > 3 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

// Для тестов в каталоге tests (эти функции внутренние).
#[doc(hidden)]
pub fn b64_encode_for_tests(d: &[u8]) -> String {
    b64_encode(d)
}
#[doc(hidden)]
pub fn b64_decode_for_tests(s: &str) -> Result<Vec<u8>> {
    b64_decode(s)
}

// ---------- код группы и шифрование ----------

/// Новый случайный код группы вида `ABCD-EFGH-JKLM-NPQR` (16 символов, 80 бит).
pub fn generate_code() -> String {
    let mut b = [0u8; 16];
    random_bytes(&mut b);
    let chars: Vec<char> = b.iter().map(|x| CODE_ALPHABET[(*x & 31) as usize] as char).collect();
    chars.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-")
}

/// Приводит введённый код к каноническому виду или объясняет, что не так.
pub fn normalize_code(input: &str) -> Result<String> {
    let chars: Vec<char> = input
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if chars.len() != 16 || !chars.iter().all(|c| CODE_ALPHABET.contains(&(*c as u8))) {
        return Err(bad("Код группы состоит из 16 букв и цифр, например ABCD-EFGH-JKLM-NPQR"));
    }
    Ok(chars.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-"))
}

/// Ключи, выведенные из кода группы. Сам код наружу не уходит.
#[derive(Clone)]
pub struct GroupKeys {
    pub key: [u8; 32],
    /// Имя «канала» на сервере ntfy.
    pub topic: String,
    /// Метка группы для поиска устройств в локальной сети.
    pub lan: String,
}

pub fn derive_keys(code: &str) -> GroupKeys {
    let h = |label: &str| -> [u8; 32] {
        let mut d = Sha256::new();
        d.update(label.as_bytes());
        d.update(b":");
        d.update(code.as_bytes());
        d.finalize().into()
    };
    GroupKeys {
        key: h("dayplanner/sync/key/v1"),
        topic: format!("dp{}", &hex(&h("dayplanner/sync/topic/v1"))[..30]),
        lan: hex(&h("dayplanner/sync/lan/v1"))[..16].to_string(),
    }
}

pub fn seal(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce = [0u8; 12];
    random_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plain)
        .map_err(|_| bad("Не удалось зашифровать данные"))?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn open(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 12 + 16 {
        return Err(bad("Повреждённые данные синхронизации"));
    }
    let (nonce, ct) = data.split_at(12);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| bad("Не удалось расшифровать: неверный код группы или повреждённые данные"))
}

// ---------- форматы обмена ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub kind: String,
    pub uid: String,
    pub modified: i64,
    pub deleted: bool,
    #[serde(default)]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockedRow {
    pub id: String,
    pub at: String,
    pub points: i64,
}

/// Снимок для обмена. `msg`: "data" — данные, "hello" — устройство только что вступило в группу
/// и просит остальных прислать всё.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payload {
    pub v: u32,
    pub device: String,
    pub msg: String,
    #[serde(default)]
    pub records: Vec<Record>,
    #[serde(default)]
    pub unlocked: Vec<UnlockedRow>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MergeStats {
    /// Сколько записей принято (новее или новые).
    pub applied: usize,
    /// Сколько записей уже было актуальным.
    pub skipped: usize,
    /// Сколько записей отброшено как некорректные.
    pub rejected: usize,
}

impl MergeStats {
    pub fn add(&mut self, o: &MergeStats) {
        self.applied += o.applied;
        self.skipped += o.skipped;
        self.rejected += o.rejected;
    }
}

#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Код группы; `None` — синхронизация не настроена.
    pub code: Option<String>,
    pub ntfy: bool,
    pub lan: bool,
    pub server: String,
    pub device: String,
}

pub const DEFAULT_NTFY_SERVER: &str = "https://ntfy.sh";

// ---------- метаданные и слияние ----------

impl Store {
    /// Подмена часов в тестах (миллисекунды). В приложении не используется.
    pub fn set_test_clock(&self, ms: Option<i64>) {
        self.clock.set(ms);
    }

    /// Время для отметки изменения: настоящие часы, но каждое следующее значение строго больше.
    pub(crate) fn stamp(&self) -> i64 {
        let now = self.clock.get().unwrap_or_else(|| Utc::now().timestamp_millis());
        let v = now.max(self.last_stamp.get() + 1);
        self.last_stamp.set(v);
        v
    }

    /// Наибольшая выданная отметка времени: всё, что изменено на этом устройстве, не позже неё.
    pub fn sync_high_water(&self) -> i64 {
        self.last_stamp.get()
    }

    /// Следующий свободный числовой id. Номера удалённых записей не используются повторно:
    /// иначе новая запись «унаследовала» бы чужую историю и надгробие.
    pub(crate) fn next_id(&self, kind: &str, table: &str) -> Result<i64> {
        let sql = format!(
            "SELECT MAX((SELECT COALESCE(MAX(id),0) FROM {table}),
                        (SELECT COALESCE(MAX(CAST(key AS INTEGER)),0) FROM sync_meta WHERE kind=?1))"
        );
        let m: i64 = self.conn.query_row(&sql, params![kind], |r| r.get(0))?;
        Ok(m + 1)
    }

    pub(crate) fn ids_where_category(&self, table: &str, category: i64) -> Result<Vec<i64>> {
        let mut st = self.conn.prepare(&format!("SELECT id FROM {table} WHERE category_id=?1"))?;
        let rows = st.query_map(params![category], |r| r.get::<_, i64>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    fn meta_uid_for(&self, kind: &str, key: &str) -> Result<String> {
        if kind == "completion" {
            // uid отметки: «uid напоминания|время срабатывания», одинаков на всех устройствах.
            let (rid, at) = key.split_once('|').ok_or_else(|| bad("неверный ключ отметки"))?;
            let rid: i64 = rid.parse().map_err(|_| bad("неверный ключ отметки"))?;
            let ruid = self.reminder_uid(rid)?;
            return Ok(format!("{ruid}|{at}"));
        }
        Ok(random_hex(8))
    }

    /// uid напоминания по числовому id; для давно удалённых напоминаний создаёт надгробие.
    fn reminder_uid(&self, rid: i64) -> Result<String> {
        let key = rid.to_string();
        if let Some(u) = self
            .conn
            .query_row("SELECT uid FROM sync_meta WHERE kind='reminder' AND key=?1", params![key], |r| r.get::<_, String>(0))
            .optional()?
        {
            return Ok(u);
        }
        let uid = random_hex(8);
        let ts = self.stamp();
        self.conn.execute(
            "INSERT INTO sync_meta(kind,key,uid,modified,deleted) VALUES ('reminder',?1,?2,?3,1)",
            params![key, uid, ts],
        )?;
        Ok(uid)
    }

    /// Запись создана или изменена.
    pub(crate) fn touch(&self, kind: &str, key: &str) -> Result<()> {
        let ts = self.stamp();
        let n = self.conn.execute(
            "UPDATE sync_meta SET modified=?3, deleted=0 WHERE kind=?1 AND key=?2",
            params![kind, key, ts],
        )?;
        if n == 0 {
            let uid = self.meta_uid_for(kind, key)?;
            self.conn.execute(
                "INSERT INTO sync_meta(kind,key,uid,modified,deleted) VALUES (?1,?2,?3,?4,0)",
                params![kind, key, uid, ts],
            )?;
        }
        CHANGES.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Запись удалена: остаётся «надгробие», чтобы удаление дошло до других устройств.
    pub(crate) fn mark_deleted(&self, kind: &str, key: &str) -> Result<()> {
        let ts = self.stamp();
        let n = self.conn.execute(
            "UPDATE sync_meta SET modified=?3, deleted=1 WHERE kind=?1 AND key=?2",
            params![kind, key, ts],
        )?;
        if n == 0 {
            let uid = self.meta_uid_for(kind, key)?;
            self.conn.execute(
                "INSERT INTO sync_meta(kind,key,uid,modified,deleted) VALUES (?1,?2,?3,?4,1)",
                params![kind, key, uid, ts],
            )?;
        }
        CHANGES.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Выдаёт постоянные идентификаторы записям, появившимся до синхронизации.
    pub(crate) fn ensure_meta(&self) -> Result<()> {
        for (kind, table) in [
            ("category", "categories"),
            ("reminder", "reminders"),
            ("schedule", "schedule_entries"),
            ("note", "notes"),
        ] {
            let sql = format!(
                "SELECT id FROM {table} WHERE CAST(id AS TEXT) NOT IN (SELECT key FROM sync_meta WHERE kind='{kind}') ORDER BY id"
            );
            let mut missing: Vec<i64> = Vec::new();
            {
                let mut st = self.conn.prepare(&sql)?;
                let rows = st.query_map([], |r| r.get::<_, i64>(0))?;
                for r in rows {
                    missing.push(r?);
                }
            }
            for id in missing {
                let key = id.to_string();
                let mut uid = random_hex(8);
                let mut ts: Option<i64> = None;
                if kind == "category" {
                    // Стандартные категории получают общий uid и «древнее» время: любая правка пользователя сильнее.
                    let (name, color, icon): (String, String, String) = self.conn.query_row(
                        "SELECT name,color,icon FROM categories WHERE id=?1",
                        params![id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )?;
                    for (n, c, i, seed_uid) in SEED_CATEGORIES {
                        if n == name && c == color && i == icon {
                            let taken: i64 = self.conn.query_row(
                                "SELECT COUNT(*) FROM sync_meta WHERE kind='category' AND uid=?1",
                                params![seed_uid],
                                |r| r.get(0),
                            )?;
                            if taken == 0 {
                                uid = seed_uid.to_string();
                                ts = Some(1);
                            }
                            break;
                        }
                    }
                }
                let ts = match ts {
                    Some(t) => t,
                    None => self.stamp(),
                };
                self.conn.execute(
                    "INSERT INTO sync_meta(kind,key,uid,modified,deleted) VALUES (?1,?2,?3,?4,0)",
                    params![kind, key, uid, ts],
                )?;
            }
        }
        let mut missing: Vec<String> = Vec::new();
        {
            let mut st = self.conn.prepare(
                "SELECT reminder_id || '|' || at FROM completions
                 WHERE (reminder_id || '|' || at) NOT IN (SELECT key FROM sync_meta WHERE kind='completion')",
            )?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?;
            for r in rows {
                missing.push(r?);
            }
        }
        for key in missing {
            let uid = self.meta_uid_for("completion", &key)?;
            let ts = self.stamp();
            self.conn.execute(
                "INSERT INTO sync_meta(kind,key,uid,modified,deleted) VALUES ('completion',?1,?2,?3,0)",
                params![key, uid, ts],
            )?;
        }
        let top: i64 = self.conn.query_row("SELECT COALESCE(MAX(modified),0) FROM sync_meta", [], |r| r.get(0))?;
        if top > self.last_stamp.get() {
            self.last_stamp.set(top);
        }
        Ok(())
    }

    // ----- выгрузка -----

    fn uid_of(&self, kind: &str, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT uid FROM sync_meta WHERE kind=?1 AND key=?2 AND deleted=0",
                params![kind, key],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
    }

    /// Запись с заменой числовых ссылок на uid.
    fn entity_value<T: Serialize>(&self, item: &T, category: Option<i64>) -> Result<Value> {
        let mut v = serde_json::to_value(item)?;
        if let Some(o) = v.as_object_mut() {
            o.remove("id");
            o.remove("category_id");
            let cu = match category {
                Some(c) => self.uid_of("category", &c.to_string())?,
                None => None,
            };
            o.insert("category_uid".into(), cu.map(Value::String).unwrap_or(Value::Null));
        }
        Ok(v)
    }

    /// Все записи, изменённые после `since` (0 — всё, включая удаления).
    pub fn export_records(&self, since: i64) -> Result<Vec<Record>> {
        let cats: HashMap<i64, Category> = self.list_categories()?.into_iter().map(|c| (c.id, c)).collect();
        let rems: HashMap<i64, Reminder> = self.list_reminders(true)?.into_iter().map(|r| (r.id, r)).collect();
        let sched: HashMap<i64, ScheduleEntry> = self.list_schedule()?.into_iter().map(|e| (e.id, e)).collect();
        let notes: HashMap<i64, Note> = self.list_notes(None, None)?.into_iter().map(|n| (n.id, n)).collect();
        let comps: HashMap<String, Completion> = self
            .all_completions()?
            .into_iter()
            .map(|c| (comp_meta_key(c.reminder_id, &dt::format(&c.at)), c))
            .collect();

        let mut metas: Vec<(String, String, String, i64, bool)> = Vec::new();
        {
            let mut st = self
                .conn
                .prepare("SELECT kind,key,uid,modified,deleted FROM sync_meta WHERE modified > ?1")?;
            let rows = st.query_map(params![since], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)? != 0))
            })?;
            for r in rows {
                metas.push(r?);
            }
        }

        let mut out = Vec::new();
        for (kind, key, uid, modified, deleted) in metas {
            if deleted {
                out.push(Record { kind, uid, modified, deleted: true, data: Value::Null });
                continue;
            }
            let id: i64 = key.parse().unwrap_or(0);
            let data = match kind.as_str() {
                "category" => cats.get(&id).map(|c| json!({ "name": c.name, "color": c.color, "icon": c.icon })),
                "reminder" => match rems.get(&id) {
                    Some(r) => Some(self.entity_value(r, r.category_id)?),
                    None => None,
                },
                "schedule" => match sched.get(&id) {
                    Some(e) => Some(self.entity_value(e, e.category_id)?),
                    None => None,
                },
                "note" => match notes.get(&id) {
                    Some(n) => Some(self.entity_value(n, n.category_id)?),
                    None => None,
                },
                "completion" => comps.get(&key).map(|c| {
                    json!({
                        "kind": if c.kind == CompletionKind::Skipped { "skipped" } else { "done" },
                        "completed_at": dt::format(&c.completed_at),
                        "points": c.points,
                    })
                }),
                _ => None,
            };
            if let Some(data) = data {
                out.push(Record { kind, uid, modified, deleted: false, data });
            }
        }
        Ok(out)
    }

    pub fn export_unlocked(&self) -> Result<Vec<UnlockedRow>> {
        let mut st = self.conn.prepare("SELECT id,at,points FROM unlocked")?;
        let rows = st.query_map([], |r| {
            Ok(UnlockedRow { id: r.get(0)?, at: r.get(1)?, points: r.get(2)? })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn make_payload(&self, since: i64, msg: &str) -> Result<Payload> {
        Ok(Payload {
            v: 2,
            device: self.sync_config()?.device,
            msg: msg.to_string(),
            records: self.export_records(since)?,
            unlocked: self.export_unlocked()?,
        })
    }

    // ----- загрузка и слияние -----

    fn key_of_alive(&self, kind: &str, uid: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT key FROM sync_meta WHERE kind=?1 AND uid=?2 AND deleted=0",
                params![kind, uid],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
    }

    /// Числовой id напоминания по uid (в том числе удалённого; неизвестному заводится надгробие).
    fn reminder_id_for_uid(&self, uid: &str) -> Result<i64> {
        if let Some(k) = self
            .conn
            .query_row("SELECT key FROM sync_meta WHERE kind='reminder' AND uid=?1", params![uid], |r| r.get::<_, String>(0))
            .optional()?
        {
            return k.parse().map_err(|_| bad("неверный ключ напоминания"));
        }
        let id = self.next_id("reminder", "reminders")?;
        self.conn.execute(
            "INSERT INTO sync_meta(kind,key,uid,modified,deleted) VALUES ('reminder',?1,?2,0,1)",
            params![id.to_string(), uid],
        )?;
        Ok(id)
    }

    fn completion_key_from_uid(&self, uid: &str) -> Result<String> {
        let (ruid, at) = uid.split_once('|').ok_or_else(|| bad("неверный uid отметки"))?;
        let at = dt::parse(at).map_err(Error::Invalid)?;
        let rid = self.reminder_id_for_uid(ruid)?;
        Ok(comp_meta_key(rid, &dt::format(&at)))
    }

    fn entity_from<T: DeserializeOwned>(&self, data: &Value, id: i64) -> Result<T> {
        let mut v = data.clone();
        let obj = v.as_object_mut().ok_or_else(|| bad("запись синхронизации повреждена"))?;
        let category: Option<i64> = match obj.get("category_uid").and_then(|x| x.as_str()) {
            Some(u) => self.key_of_alive("category", u)?.and_then(|k| k.parse::<i64>().ok()),
            None => None,
        };
        obj.remove("category_uid");
        obj.insert("id".into(), Value::from(id));
        obj.insert("category_id".into(), category.map(Value::from).unwrap_or(Value::Null));
        serde_json::from_value(v).map_err(|e| Error::Invalid(format!("запись синхронизации повреждена: {e}")))
    }

    fn upsert_meta(&self, kind: &str, key: &str, uid: &str, modified: i64, deleted: bool) -> Result<()> {
        // Часы другого устройства могут спешить: наши следующие правки должны быть новее принятых.
        if modified > self.last_stamp.get() {
            self.last_stamp.set(modified);
        }
        self.conn.execute(
            "INSERT INTO sync_meta(kind,key,uid,modified,deleted) VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(kind,key) DO UPDATE SET uid=excluded.uid, modified=excluded.modified, deleted=excluded.deleted",
            params![kind, key, uid, modified, deleted as i64],
        )?;
        Ok(())
    }

    fn delete_row(&self, kind: &str, key: &str) -> Result<()> {
        if kind == "completion" {
            let (rid, at) = key.split_once('|').ok_or_else(|| bad("неверный ключ отметки"))?;
            let rid: i64 = rid.parse().map_err(|_| bad("неверный ключ отметки"))?;
            self.conn.execute("DELETE FROM completions WHERE reminder_id=?1 AND at=?2", params![rid, at])?;
            return Ok(());
        }
        let table = table_of(kind).ok_or_else(|| bad("неизвестный вид записи"))?;
        let id: i64 = key.parse().map_err(|_| bad("неверный ключ записи"))?;
        self.conn.execute(&format!("DELETE FROM {table} WHERE id=?1"), params![id])?;
        if kind == "category" {
            // Остальные устройства сбросили категорию и у связанных записей; здесь — на случай, если их правки не дошли.
            for t in ["reminders", "schedule_entries", "notes"] {
                self.conn.execute(&format!("UPDATE {t} SET category_id=NULL WHERE category_id=?1"), params![id])?;
            }
        }
        Ok(())
    }

    fn raw_put_reminder(&self, r: &Reminder) -> Result<()> {
        let repeat = serde_json::to_string(&r.repeat)?;
        let until = r.repeat_until.map(|d| d.format("%Y-%m-%d").to_string());
        self.conn.execute(
            "INSERT OR REPLACE INTO reminders(id,title,notes,icon,category_id,start_at,alert,repeat_rule,repeat_until,grace_minutes,archived,attachments)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                r.id,
                r.title.trim(),
                r.notes,
                r.icon,
                r.category_id,
                dt::format(&r.start),
                r.alert.as_str(),
                repeat,
                until,
                r.grace_minutes,
                r.archived as i64,
                serde_json::to_string(&r.attachments)?
            ],
        )?;
        Ok(())
    }

    fn raw_put_schedule(&self, e: &ScheduleEntry) -> Result<()> {
        let weekday: Option<i64> = e.weekday.map(|w| w as i64);
        let on_date = e.date.map(|d| d.format("%Y-%m-%d").to_string());
        self.conn.execute(
            "INSERT OR REPLACE INTO schedule_entries(id,title,weekday,on_date,start_time,end_time,color,icon,location,category_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                e.id,
                e.title.trim(),
                weekday,
                on_date,
                tm::format(&e.start),
                tm::format(&e.end),
                e.color,
                e.icon,
                e.location,
                e.category_id
            ],
        )?;
        Ok(())
    }

    fn raw_put_note(&self, n: &Note) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO notes(id,title,body,color,pinned,category_id,created_at,updated_at,attachments,icon)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                n.id,
                n.title.trim(),
                n.body,
                n.color.clone().filter(|c| !c.is_empty()),
                n.pinned as i64,
                n.category_id,
                dt::format(&n.created),
                dt::format(&n.updated),
                serde_json::to_string(&n.attachments)?,
                n.icon
            ],
        )?;
        Ok(())
    }

    /// Применяет одну запись. `Ok(false)` — локальная версия не старше, ничего не менялось.
    fn apply_record(&self, rec: &Record) -> Result<bool> {
        let kind = rec.kind.as_str();
        let local: Option<(String, i64)> = self
            .conn
            .query_row(
                "SELECT key,modified FROM sync_meta WHERE kind=?1 AND uid=?2",
                params![kind, rec.uid],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?;
        if let Some((_, m)) = &local {
            if rec.modified <= *m {
                return Ok(false);
            }
        }

        // Сначала разбираем данные: если запись повреждена, база остаётся нетронутой.
        enum Parsed {
            Deleted,
            Category(String, String, String),
            Reminder(Reminder),
            Schedule(ScheduleEntry),
            Note(Note),
            Completion(String, String, i64),
        }
        let key_hint: i64 = local.as_ref().and_then(|(k, _)| k.parse::<i64>().ok()).unwrap_or(0);
        let parsed = if rec.deleted {
            Parsed::Deleted
        } else {
            let text = |name: &str| -> Result<String> {
                rec.data
                    .get(name)
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| bad("запись синхронизации повреждена"))
            };
            match kind {
                "category" => {
                    let name = text("name")?;
                    if name.trim().is_empty() {
                        return Err(bad("пустое имя категории"));
                    }
                    Parsed::Category(name.trim().to_string(), text("color")?, text("icon")?)
                }
                "reminder" => {
                    let r: Reminder = self.entity_from(&rec.data, key_hint)?;
                    r.validate()?;
                    Parsed::Reminder(r)
                }
                "schedule" => {
                    let e: ScheduleEntry = self.entity_from(&rec.data, key_hint)?;
                    e.validate()?;
                    Parsed::Schedule(e)
                }
                "note" => {
                    let n: Note = self.entity_from(&rec.data, key_hint)?;
                    n.validate()?;
                    Parsed::Note(n)
                }
                "completion" => {
                    let k = text("kind")?;
                    let at = text("completed_at")?;
                    dt::parse(&at).map_err(Error::Invalid)?;
                    let points = rec.data.get("points").and_then(|x| x.as_i64()).unwrap_or(0);
                    Parsed::Completion(if k == "skipped" { "skipped".into() } else { "done".into() }, at, points)
                }
                _ => return Err(bad("неизвестный вид записи")),
            }
        };

        let key: String = match (&local, kind) {
            (Some((k, _)), _) => k.clone(),
            (None, "completion") => self.completion_key_from_uid(&rec.uid)?,
            (None, _) => self.next_id(kind, table_of(kind).unwrap_or("notes"))?.to_string(),
        };
        let id: i64 = key.parse().unwrap_or(0);

        match parsed {
            Parsed::Deleted => self.delete_row(kind, &key)?,
            Parsed::Category(name, color, icon) => {
                self.conn.execute(
                    "INSERT OR REPLACE INTO categories(id,name,color,icon) VALUES (?1,?2,?3,?4)",
                    params![id, name, color, icon],
                )?;
            }
            Parsed::Reminder(mut r) => {
                r.id = id;
                self.raw_put_reminder(&r)?;
            }
            Parsed::Schedule(mut e) => {
                e.id = id;
                self.raw_put_schedule(&e)?;
            }
            Parsed::Note(mut n) => {
                n.id = id;
                self.raw_put_note(&n)?;
            }
            Parsed::Completion(k, at, points) => {
                let (rid, when) = key.split_once('|').ok_or_else(|| bad("неверный ключ отметки"))?;
                let rid: i64 = rid.parse().map_err(|_| bad("неверный ключ отметки"))?;
                self.conn.execute(
                    "INSERT OR REPLACE INTO completions(reminder_id,at,kind,completed_at,points) VALUES (?1,?2,?3,?4,?5)",
                    params![rid, when, k, at, points],
                )?;
            }
        }
        self.upsert_meta(kind, &key, &rec.uid, rec.modified, rec.deleted)?;
        Ok(true)
    }

    /// Сливает чужие записи с локальными. Всё или ничего: при ошибке базы изменения откатываются.
    pub fn merge(&self, records: &[Record], unlocked: &[UnlockedRow]) -> Result<MergeStats> {
        self.merge_mode(records, unlocked, false)
    }

    pub(crate) fn merge_strict(&self, records: &[Record], unlocked: &[UnlockedRow]) -> Result<MergeStats> {
        self.merge_mode(records, unlocked, true)
    }

    fn merge_mode(&self, records: &[Record], unlocked: &[UnlockedRow], strict: bool) -> Result<MergeStats> {
        let tx = self.conn.unchecked_transaction()?;
        let mut stats = MergeStats::default();
        let mut sorted: Vec<&Record> = records.iter().collect();
        sorted.sort_by_key(|r| KIND_ORDER.iter().position(|k| *k == r.kind).unwrap_or(99));
        for rec in sorted {
            if !KIND_ORDER.contains(&rec.kind.as_str()) || rec.uid.is_empty() || rec.uid.len() > 200 || rec.modified < 0 {
                stats.rejected += 1;
                continue;
            }
            match self.apply_record(rec) {
                Ok(true) => stats.applied += 1,
                Ok(false) => stats.skipped += 1,
                Err(Error::Invalid(_)) | Err(Error::Json(_)) => stats.rejected += 1,
                Err(e) => return Err(e),
            }
        }
        for u in unlocked {
            if u.id.is_empty() || u.id.len() > 64 || dt::parse(&u.at).is_err() {
                stats.rejected += 1;
                continue;
            }
            // Достижение, полученное на любом устройстве, считается полученным; время берём раннее.
            let n = self.conn.execute(
                "INSERT OR IGNORE INTO unlocked(id,at,points) VALUES (?1,?2,?3)",
                params![u.id, u.at, u.points],
            )?;
            if n > 0 {
                stats.applied += 1;
            } else {
                self.conn.execute("UPDATE unlocked SET at=?2 WHERE id=?1 AND at > ?2", params![u.id, u.at])?;
            }
        }
        if strict && stats.rejected > 0 { return Err(bad("Копия содержит повреждённые записи; изменения не применены")); }
        tx.commit()?;
        Ok(stats)
    }

    // ----- настройки группы -----

    pub fn sync_config(&self) -> Result<SyncConfig> {
        let device = match self.get_setting("sync.device")? {
            Some(d) if !d.is_empty() => d,
            _ => {
                let d = random_hex(8);
                self.set_setting("sync.device", &d)?;
                d
            }
        };
        let code = self.get_setting("sync.code")?.filter(|c| !c.is_empty());
        let flag = |k: &str| -> Result<bool> { Ok(self.get_setting(k)?.map(|v| v != "false").unwrap_or(true)) };
        let server = self
            .get_setting("sync.server")?
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_NTFY_SERVER.to_string());
        Ok(SyncConfig { code, ntfy: flag("sync.ntfy")?, lan: flag("sync.lan")?, server, device })
    }

    fn reset_sync_cursors(&self) -> Result<()> {
        for k in ["sync.pushed", "sync.since", "sync.seen", "sync.hello", "sync.error", "sync.full_at"] {
            self.delete_setting(k)?;
        }
        Ok(())
    }

    /// Создаёт группу: новый код, остальные устройства вводят его у себя.
    pub fn sync_create_group(&self) -> Result<String> {
        let code = generate_code();
        self.set_setting("sync.code", &code)?;
        self.reset_sync_cursors()?;
        Ok(code)
    }

    /// Вступает в группу по коду; сразу просит остальных прислать данные.
    pub fn sync_join_group(&self, input: &str) -> Result<String> {
        let code = normalize_code(input)?;
        self.set_setting("sync.code", &code)?;
        self.reset_sync_cursors()?;
        self.set_setting("sync.hello", "pending")?;
        Ok(code)
    }

    pub fn sync_leave_group(&self) -> Result<()> {
        self.delete_setting("sync.code")?;
        self.reset_sync_cursors()?;
        for k in ["sync.last_ok", "sync.last_lan"] {
            self.delete_setting(k)?;
        }
        Ok(())
    }

    pub fn sync_set_transports(&self, ntfy: bool, lan: bool, server: &str) -> Result<()> {
        let server = server.trim().trim_end_matches('/');
        if !server.is_empty() && !(server.starts_with("https://") || server.starts_with("http://")) {
            return Err(bad("Адрес сервера ntfy должен начинаться с https://"));
        }
        self.set_setting("sync.ntfy", if ntfy { "true" } else { "false" })?;
        self.set_setting("sync.lan", if lan { "true" } else { "false" })?;
        self.set_setting("sync.server", if server.is_empty() { DEFAULT_NTFY_SERVER } else { server })?;
        Ok(())
    }

    pub fn sync_status(&self) -> Result<Value> {
        let c = self.sync_config()?;
        let num = |k: &str| -> Result<Option<i64>> { Ok(self.get_setting(k)?.and_then(|s| s.parse::<i64>().ok())) };
        Ok(json!({
            "enabled": c.code.is_some(),
            "code": c.code,
            "ntfy": c.ntfy,
            "lan": c.lan,
            "server": c.server,
            "device": c.device,
            "last_ok": num("sync.last_ok")?,
            "last_lan": num("sync.last_lan")?,
            "error": self.get_setting("sync.error")?.filter(|s| !s.is_empty()),
        }))
    }

    // ----- файл -----

    /// Текст для файла синхронизации. Если группа настроена, данные зашифрованы её кодом.
    pub fn sync_export_text(&self) -> Result<String> {
        let payload = self.make_payload(0, "data")?;
        let json = serde_json::to_string(&payload)?;
        match self.sync_config()?.code {
            Some(code) => {
                let sealed = seal(&derive_keys(&code).key, json.as_bytes())?;
                Ok(format!("{FILE_PREFIX}{}", b64_encode(&sealed)))
            }
            None => Ok(json),
        }
    }

    /// Сливает содержимое файла синхронизации (обычного или зашифрованного кодом группы).
    pub fn sync_import_text(&self, text: &str) -> Result<MergeStats> {
        let text = text.trim().trim_start_matches('\u{feff}');
        if text.len() > MAX_IMPORT_BYTES {
            return Err(bad("Файл слишком большой"));
        }
        let json: Vec<u8> = if let Some(rest) = text.strip_prefix(FILE_PREFIX) {
            let code = self
                .sync_config()?
                .code
                .ok_or_else(|| bad("Файл зашифрован кодом группы: сначала вступите в группу, введя код"))?;
            open(&derive_keys(&code).key, &b64_decode(rest)?)?
        } else {
            text.as_bytes().to_vec()
        };
        let payload: Payload = serde_json::from_slice(&json).map_err(|_| bad("Это не файл синхронизации ежедневника"))?;
        self.apply_payload(&payload)
    }

    pub fn apply_payload(&self, p: &Payload) -> Result<MergeStats> {
        if p.v != 1 && p.v != 2 {
            return Err(bad("Файл создан более новой версией приложения: обновите приложение"));
        }
        if p.v == 1 {
            // An old client does not know about attachments and would silently
            // drop them when editing/deleting a previously synced record.
            for r in &p.records {
                if !matches!(r.kind.as_str(), "note" | "reminder") || r.data.get("attachments").is_some() { continue; }
                if let Some(id) = self.key_of_alive(&r.kind, &r.uid)?.and_then(|s| s.parse::<i64>().ok()) {
                    let has_files = if r.kind == "note" { !self.get_note(id)?.attachments.is_empty() } else { !self.get_reminder(id)?.attachments.is_empty() };
                    if has_files { return Err(bad("На другом устройстве старая версия приложения. Обновите её до 0.2.0, чтобы не потерять вложения")); }
                }
            }
        }
        self.merge(&p.records, &p.unlocked)
    }
}
