//! Portable extras: attachments, raster icon packs, local lock and conflict-safe undo.
use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::path::Path;
use chrono::Utc;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use crate::{Store, error::{Error, Result}, model::Attachment, sync::{self, Record}};

pub fn invalid(message: &str) -> Error { Error::Invalid(message.into()) }
pub fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::getrandom(&mut bytes).map_err(|_| invalid("Недоступен системный генератор случайных чисел"))?;
    Ok(bytes)
}

pub(crate) fn load_private_key(dir: Option<&Path>) -> Result<[u8; 32]> {
    let Some(dir) = dir else { return random(); };
    let path = dir.join(".dayplanner-private-key");
    if path.exists() {
        return std::fs::read(path).map_err(|_| invalid("Не удалось прочитать локальный ключ"))?
            .try_into().map_err(|_| invalid("Повреждён локальный ключ; восстановите его из приватной копии"));
    }
    let key = random()?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let mut file = options.open(path).map_err(|_| invalid("Не удалось создать локальный ключ"))?;
    file.write_all(&key).map_err(|_| invalid("Не удалось сохранить локальный ключ"))?;
    file.sync_all().map_err(|_| invalid("Не удалось сохранить локальный ключ"))?;
    Ok(key)
}

pub fn validate_attachments(files: &[Attachment]) -> Result<()> {
    if files.len() > 10 { return Err(invalid("Не больше 10 вложений на запись")); }
    let mut total = 0;
    let mut ids = HashSet::new();
    for file in files {
        if file.name.trim().is_empty() || file.name.len() > 240 || file.name.chars().any(|c| c.is_control() || "/\\".contains(c)) {
            return Err(invalid("Недопустимое имя вложения"));
        }
        if file.data.len() > 14 * 1024 * 1024 { return Err(invalid("Вложение больше 10 МБ")); }
        let bytes = sync::b64_decode(&file.data)?;
        if bytes.len() > 10 * 1024 * 1024 { return Err(invalid("Вложение больше 10 МБ")); }
        let hash = sync::hex(&Sha256::digest(&bytes));
        if hash != file.id || !ids.insert(&file.id) { return Err(invalid("Повреждённое или повторное вложение")); }
        if file.mime.len() > 100 || file.mime.chars().any(char::is_control) { return Err(invalid("Недопустимый тип вложения")); }
        total += bytes.len();
    }
    if total > 20 * 1024 * 1024 { return Err(invalid("Вложения записи больше 20 МБ")); }
    Ok(())
}

pub fn validate_icon(value: &str) -> Result<()> {
    if let Some(encoded) = value.strip_prefix("data:image/png;base64,") {
        if encoded.len() > 350_000 { return Err(invalid("Иконка слишком большая")); }
        let bytes = sync::b64_decode(encoded)?;
        let reader = image::io::Reader::with_format(Cursor::new(bytes.as_slice()), image::ImageFormat::Png);
        let (w, h) = reader.into_dimensions().map_err(|_| invalid("Повреждённая PNG-иконка"))?;
        if w == 0 || h == 0 || w > 192 || h > 192 { return Err(invalid("Иконка должна быть не больше 192×192")); }
        image::load_from_memory_with_format(&bytes,image::ImageFormat::Png).map_err(|_| invalid("Повреждённая PNG-иконка"))?;
    } else if value.len() > 64 || value.chars().any(char::is_control) {
        return Err(invalid("Иконка: эмодзи или загруженный PNG"));
    }
    Ok(())
}

pub fn optimize_icon(bytes: &[u8]) -> Result<String> {
    if bytes.len() > 5 * 1024 * 1024 { return Err(invalid("Иконка больше 5 МБ")); }
    let mut reader = image::io::Reader::new(Cursor::new(bytes)).with_guessed_format().map_err(|_| invalid("Неизвестный формат иконки"))?;
    if !matches!(reader.format(), Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP)) {
        return Err(invalid("Иконки: PNG, JPEG или WebP; SVG не принимаются"));
    }
    let mut limits = image::io::Limits::default();
    limits.max_image_width = Some(4096); limits.max_image_height = Some(4096); limits.max_alloc = Some(80 * 1024 * 1024);
    reader.limits(limits);
    let icon = reader.decode().map_err(|_| invalid("Повреждённая или слишком большая иконка"))?.thumbnail(192, 192);
    let mut output = Cursor::new(Vec::new());
    icon.write_to(&mut output, image::ImageOutputFormat::Png).map_err(|_| invalid("Не удалось обработать иконку"))?;
    Ok(format!("data:image/png;base64,{}", sync::b64_encode(output.get_ref())))
}

/// Read in memory, never extract archive member paths to disk.
pub fn import_icon_pack(encoded: &str) -> Result<Value> {
    if encoded.len() > 28 * 1024 * 1024 { return Err(invalid("Архив больше 20 МБ")); }
    let bytes = sync::b64_decode(encoded)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid("Нужен ZIP-архив иконок"))?;
    if archive.len() > 64 { return Err(invalid("Не больше 64 файлов в архиве")); }
    let supported = ["today", "list", "note", "trophy", "settings", "plusS", "auth", "update", "nazad", "basket1", "redact", "info", "logout", "undo"];
    let mut icons = BTreeMap::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|_| invalid("Повреждённый ZIP"))?;
        if file.is_dir() { continue; }
        total = total.saturating_add(file.size());
        if total > 20 * 1024 * 1024 || file.size() > 5 * 1024 * 1024 { return Err(invalid("Распакованный архив слишком большой")); }
        let name = file.name().replace('\\', "/");
        if name.split('/').any(|p| p == "..") || name.starts_with('/') || name.contains(':') { return Err(invalid("Недопустимый путь в ZIP")); }
        let stem = Path::new(&name).file_stem().and_then(|p| p.to_str()).unwrap_or("").to_string();
        if !supported.contains(&stem.as_str()) { continue; }
        if icons.contains_key(&stem) { return Err(invalid("Повторное имя иконки в ZIP")); }
        let mut data = Vec::new();
        (&mut file).take(5 * 1024 * 1024 + 1).read_to_end(&mut data).map_err(|_| invalid("Не удалось прочитать иконку"))?;
        icons.insert(stem, optimize_icon(&data)?);
    }
    if icons.is_empty() { return Err(invalid("В ZIP нет поддерживаемых иконок: settings.png, plusS.png, today.png и т. д.")); }
    Ok(json!(icons))
}

pub const UNDOABLE: &[&str] = &["categories.save", "categories.delete", "reminders.save", "reminders.delete", "reminders.archive", "notes.save", "notes.delete", "schedule.save", "schedule.delete", "complete", "skip", "undo", "postpone"];

impl Store {
    pub(crate) fn private_put(&self, name: &str, value: &Value) -> Result<()> {
        let nonce = random::<12>()?;
        use chacha20poly1305::{aead::{Aead, KeyInit}, ChaCha20Poly1305, Nonce};
        let cipher = ChaCha20Poly1305::new_from_slice(&self.private_key).map_err(|_| invalid("Ошибка локального ключа"))?;
        let mut bytes = nonce.to_vec();
        bytes.extend(cipher.encrypt(Nonce::from_slice(&nonce), serde_json::to_vec(value)?.as_slice()).map_err(|_| invalid("Ошибка шифрования"))?);
        self.set_setting(&format!("private.{name}"), &sync::b64_encode(&bytes))
    }
    pub(crate) fn private_get(&self, name: &str) -> Result<Option<Value>> {
        let Some(text) = self.get_setting(&format!("private.{name}"))? else { return Ok(None); };
        Ok(Some(serde_json::from_slice(&sync::open(&self.private_key, &sync::b64_decode(&text)?)?)?))
    }
    pub fn lock_status(&self) -> Result<Value> {
        let enabled = self.get_setting("private.password")?.is_some();
        Ok(json!({"enabled":enabled, "locked":enabled && !self.unlocked.get(), "retry_after": (self.unlock_after.get() - Utc::now().timestamp()).max(0)}))
    }
    pub fn require_unlocked(&self) -> Result<()> {
        if self.get_setting("private.password")?.is_some() && !self.unlocked.get() { return Err(invalid("LOCKED: введите пароль приложения")); }
        Ok(())
    }
    pub fn unlock_app(&self, password: &str) -> Result<Value> {
        if password.len()>512 { return Err(invalid("Пароль слишком длинный")); }
        if Utc::now().timestamp() < self.unlock_after.get() { return Err(invalid("Слишком много попыток. Подождите 30 секунд")); }
        let Some(v) = self.private_get("password")? else { self.unlocked.set(true); return self.lock_status(); };
        let salt = sync::b64_decode(v["salt"].as_str().unwrap_or(""))?;
        let expected = sync::b64_decode(v["hash"].as_str().unwrap_or(""))?;
        let mut hash = [0; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, 210_000, &mut hash);
        if hash.as_slice().ct_eq(&expected).unwrap_u8() != 1 {
            let failures = self.unlock_failures.get().saturating_add(1);
            self.unlock_failures.set(failures);
            if failures >= 5 { self.unlock_after.set(Utc::now().timestamp() + 30); self.unlock_failures.set(0); }
            return Err(invalid("Неверный пароль"));
        }
        self.unlock_failures.set(0); self.unlock_after.set(0); self.unlocked.set(true);
        self.lock_status()
    }
    pub fn set_password(&self, current: &str, password: &str) -> Result<Value> {
        self.unlock_app(current)?;
        if password.is_empty() { self.delete_setting("private.password")?; }
        else {
            if password.chars().count() < 8 || password.len() > 512 { return Err(invalid("Пароль: от 8 символов, максимум 512 байт")); }
            let salt = random::<16>()?;
            let mut hash = [0; 32];
            pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, 210_000, &mut hash);
            self.private_put("password", &json!({"salt":sync::b64_encode(&salt),"hash":sync::b64_encode(&hash)}))?;
        }
        self.unlocked.set(true); self.lock_status()
    }
    pub fn action_snapshot(&self) -> Result<Vec<Record>> {
        let mut records = self.export_records(0)?;
        records.sort_by(|a,b| (&a.kind, &a.uid).cmp(&(&b.kind, &b.uid)));
        Ok(records)
    }
    pub fn remember_action(&self, before: &[Record], label: &str) -> Result<()> {
        let after = self.action_snapshot()?;
        if serde_json::to_value(before)? == serde_json::to_value(&after)? { return Ok(()); }
        self.private_put("undo", &json!({"before":before, "after_hash":sync::hex(&Sha256::digest(serde_json::to_vec(&after)?)), "label":label}))
    }
    pub fn undo_status(&self) -> Result<Value> {
        let Some(slot) = self.private_get("undo")? else { return Ok(json!({"available":false})); };
        let hash = sync::hex(&Sha256::digest(serde_json::to_vec(&self.action_snapshot()?)?));
        Ok(json!({"available":slot["after_hash"] == hash, "label":slot["label"]}))
    }
    pub fn undo_action(&self) -> Result<Value> {
        if self.undo_status()?["available"] != true { return Err(invalid("Отмена недоступна: после действия появились другие изменения")); }
        let slot = self.private_get("undo")?.ok_or_else(|| invalid("Нет действия для отмены"))?;
        let before: Vec<Record> = serde_json::from_value(slot["before"].clone())?;
        let after = self.action_snapshot()?;
        let old: BTreeMap<_,_> = before.iter().map(|r| ((r.kind.clone(),r.uid.clone()),r)).collect();
        let stamp = self.sync_high_water().max(Utc::now().timestamp_millis()) + 1;
        let mut inverse = Vec::new();
        for r in after {
            match old.get(&(r.kind.clone(),r.uid.clone())) {
                Some(previous) if previous.modified == r.modified => {},
                Some(previous) => { let mut row = (*previous).clone(); row.modified = stamp; inverse.push(row); },
                None => inverse.push(Record { modified:stamp, deleted:true, data:Value::Null, ..r }),
            }
        }
        let stats = self.merge_strict(&inverse, &[])?;
        if stats.rejected != 0 { return Err(invalid("Не удалось отменить действие целиком")); }
        self.delete_setting("private.undo")?;
        Ok(json!(stats))
    }
    pub fn backup_text(&self) -> Result<String> {
        let settings: BTreeMap<_,_> = self.settings_with_defaults()?.into_iter().filter(|(k,_)| backup_setting(k)).collect();
        let data = json!({"format":"dayplanner-backup-v1","payload":self.make_payload(0,"data")?,"settings":settings,"icons":self.private_get("icons")?});
        let bytes = serde_json::to_vec(&data)?;
        match self.sync_config()?.code {
            Some(code) => Ok(format!("DPBACKUP1:{}",sync::b64_encode(&sync::seal(&sync::derive_keys(&code).key,&bytes)?))),
            None => Ok(String::from_utf8(bytes).map_err(|_| invalid("Ошибка резервной копии"))?),
        }
    }
    pub fn restore_backup(&self, text: &str) -> Result<Value> {
        if text.len()>40*1024*1024 { return Err(invalid("Резервная копия больше 40 МБ")); }
        let bytes = if let Some(data)=text.trim().strip_prefix("DPBACKUP1:") {
            let code=self.sync_config()?.code.ok_or_else(|| invalid("Сначала введите исходный код группы для расшифровки копии"))?;
            sync::open(&sync::derive_keys(&code).key,&sync::b64_decode(data)?)?
        } else { text.as_bytes().to_vec() };
        let v: Value=serde_json::from_slice(&bytes)?;
        if v["format"] != "dayplanner-backup-v1" { return Err(invalid("Это не резервная копия DayPlanner")); }
        let mut payload: sync::Payload=serde_json::from_value(v["payload"].clone())?;
        if payload.v != 1 && payload.v != 2 { return Err(invalid("Неизвестная версия резервной копии")); }
        let stamp=self.sync_high_water().max(Utc::now().timestamp_millis())+1;
        for r in &mut payload.records { r.modified=stamp; }
        let icons=v.get("icons").filter(|v| !v.is_null());
        if let Some(icons)=icons { for value in icons.as_object().ok_or_else(|| invalid("Повреждён набор иконок"))?.values() { validate_icon(value.as_str().ok_or_else(|| invalid("Повреждённая иконка"))?)?; } }
        let before=self.action_snapshot()?;
        let stats=self.merge_strict(&payload.records,&payload.unlocked)?;
        if let Some(settings)=v["settings"].as_object() { for (key,value) in settings { if backup_setting(key) { if let Some(value)=value.as_str().filter(|s| s.len()<4096) { self.set_setting(key,value)?; } } } }
        if let Some(icons)=icons { self.private_put("icons",icons)?; }
        self.remember_action(&before,"backup.restore")?;
        Ok(json!(stats))
    }
}

fn backup_setting(key: &str) -> bool {
    matches!(key,"theme"|"accent"|"mode"|"bg"|"bg_intensity"|"icon_pack"|"font_family"|"font_scale"|"button_color") || key.starts_with("list.")
}
