use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::timefmt::{dt, tm};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub data: String,
}

// ---------- напоминания ----------

/// Способ напоминания.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    /// Обычное уведомление.
    #[default]
    Notification,
    /// Будильник: громкий звук и вибрация, пока не отключите.
    Alarm,
    /// Уведомление на весь экран (показывается поверх экрана блокировки).
    FullScreen,
}

impl AlertKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertKind::Notification => "notification",
            AlertKind::Alarm => "alarm",
            AlertKind::FullScreen => "full_screen",
        }
    }

    pub fn parse(s: &str) -> AlertKind {
        match s {
            "alarm" => AlertKind::Alarm,
            "full_screen" => AlertKind::FullScreen,
            _ => AlertKind::Notification,
        }
    }
}

/// Повторение. Дни недели: 1 = понедельник … 7 = воскресенье.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Repeat {
    #[default]
    None,
    Daily,
    Weekdays { days: Vec<u8> },
    Monthly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Reminder {
    /// 0 — новое напоминание.
    pub id: i64,
    pub title: String,
    pub notes: String,
    /// Эмодзи или имя иконки.
    pub icon: String,
    pub category_id: Option<i64>,
    /// Дата и точное время первого срабатывания.
    #[serde(with = "dt")]
    pub start: NaiveDateTime,
    pub alert: AlertKind,
    pub repeat: Repeat,
    pub repeat_until: Option<NaiveDate>,
    /// Сколько минут после срока выполнение ещё считается «вовремя».
    pub grace_minutes: i64,
    pub archived: bool,
    pub attachments: Vec<Attachment>,
}

impl Default for Reminder {
    fn default() -> Self {
        Reminder {
            id: 0,
            title: String::new(),
            notes: String::new(),
            icon: "🔔".to_string(),
            category_id: None,
            start: NaiveDate::from_ymd_opt(2000, 1, 1)
                .and_then(|d| d.and_hms_opt(9, 0, 0))
                .unwrap(),
            alert: AlertKind::Notification,
            repeat: Repeat::None,
            repeat_until: None,
            grace_minutes: 60,
            archived: false,
            attachments: Vec::new(),
        }
    }
}

impl Reminder {
    pub fn validate(&self) -> Result<()> {
        crate::features::validate_icon(&self.icon)?;
        crate::features::validate_attachments(&self.attachments)?;
        if self.title.trim().is_empty() {
            return Err(Error::Invalid("Название не может быть пустым".into()));
        }
        if self.title.chars().count() > 200 {
            return Err(Error::Invalid("Название слишком длинное (максимум 200 символов)".into()));
        }
        if !(0..=1440).contains(&self.grace_minutes) {
            return Err(Error::Invalid("Допуск по времени: от 0 до 1440 минут".into()));
        }
        if let Repeat::Weekdays { days } = &self.repeat {
            if days.is_empty() || days.iter().any(|d| !(1..=7).contains(d)) {
                return Err(Error::Invalid("Выберите хотя бы один день недели".into()));
            }
        }
        if let Some(until) = self.repeat_until {
            if until < self.start.date() {
                return Err(Error::Invalid("Дата окончания повторов раньше начала".into()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub icon: String,
}

impl Default for Category {
    fn default() -> Self {
        Category { id: 0, name: String::new(), color: "#6750A4".into(), icon: "📁".into() }
    }
}

// ---------- таблицы-расписания ----------

/// Строка таблицы расписания: занятие/событие с началом и концом.
/// Либо повторяется каждую неделю (`weekday`), либо привязано к дате (`date`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduleEntry {
    pub id: i64,
    pub title: String,
    /// 1 = понедельник … 7 = воскресенье.
    pub weekday: Option<u8>,
    pub date: Option<NaiveDate>,
    #[serde(with = "tm")]
    pub start: NaiveTime,
    #[serde(with = "tm")]
    pub end: NaiveTime,
    pub color: Option<String>,
    pub icon: String,
    pub location: String,
    pub category_id: Option<i64>,
}

impl Default for ScheduleEntry {
    fn default() -> Self {
        ScheduleEntry {
            id: 0,
            title: String::new(),
            weekday: None,
            date: None,
            start: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
            color: None,
            icon: "📚".into(),
            location: String::new(),
            category_id: None,
        }
    }
}

impl ScheduleEntry {
    pub fn validate(&self) -> Result<()> {
        crate::features::validate_icon(&self.icon)?;
        if self.title.trim().is_empty() {
            return Err(Error::Invalid("Название не может быть пустым".into()));
        }
        match (self.weekday, self.date) {
            (Some(w), None) if (1..=7).contains(&w) => {}
            (None, Some(_)) => {}
            _ => return Err(Error::Invalid("Укажите либо день недели, либо дату".into())),
        }
        if self.end <= self.start {
            return Err(Error::Invalid("Конец должен быть позже начала".into()));
        }
        Ok(())
    }

    pub fn applies_on(&self, d: NaiveDate) -> bool {
        match (self.date, self.weekday) {
            (Some(date), _) => date == d,
            (None, Some(w)) => d.weekday().number_from_monday() as u8 == w,
            _ => false,
        }
    }
}

// ---------- выполнение ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionKind {
    Done,
    Skipped,
}

/// Запись о том, что конкретное срабатывание напоминания выполнено или пропущено.
#[derive(Debug, Clone)]
pub struct Completion {
    pub reminder_id: i64,
    pub at: NaiveDateTime,
    pub kind: CompletionKind,
    pub completed_at: NaiveDateTime,
    pub points: i64,
}

/// Состояние срабатывания напоминания. Вычисляется автоматически.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Срок ещё не наступил.
    Upcoming,
    /// Срок наступил, окно «вовремя» ещё открыто.
    Due,
    DoneOnTime,
    DoneLate,
    /// Окно закрылось, а отметки нет.
    Missed,
    Skipped,
}

impl Status {
    pub fn is_done(&self) -> bool {
        matches!(self, Status::DoneOnTime | Status::DoneLate)
    }
}

// ---------- представления ----------

#[derive(Debug, Clone, Serialize)]
pub struct DayItem {
    pub reminder_id: i64,
    pub title: String,
    pub notes: String,
    pub icon: String,
    pub category_id: Option<i64>,
    #[serde(with = "dt")]
    pub at: NaiveDateTime,
    pub alert: AlertKind,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayView {
    pub date: NaiveDate,
    pub entries: Vec<ScheduleEntry>,
    pub items: Vec<DayItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeekView {
    pub start: NaiveDate,
    pub days: Vec<DayView>,
}

/// Что нужно запланировать в системе (AlarmManager на Android).
#[derive(Debug, Clone, Serialize)]
pub struct AlarmItem {
    pub key: String,
    pub reminder_id: i64,
    #[serde(with = "dt")]
    pub at: NaiveDateTime,
    pub title: String,
    pub notes: String,
    pub icon: String,
    pub alert: AlertKind,
}

// ---------- статистика ----------

#[derive(Debug, Clone, Default, Serialize)]
pub struct Stats {
    pub total_done: u32,
    pub on_time: u32,
    pub late: u32,
    pub missed: u32,
    pub skipped: u32,
    pub current_streak: u32,
    pub best_streak: u32,
    pub perfect_days: u32,
    pub early_bird: u32,
    pub night_owl: u32,
    pub reminders_created: u32,
    pub categories_used: u32,
    pub points: i64,
    pub level: u32,
    /// Прогресс до следующего уровня, 0.0–1.0.
    pub level_progress: f64,
    pub today_total: u32,
    pub today_done: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AchievementView {
    pub id: String,
    pub icon: String,
    pub title: String,
    pub description: String,
    pub reward: i64,
    pub target: u32,
    pub progress: u32,
    pub unlocked: bool,
    pub unlocked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompleteOutcome {
    pub status: Status,
    pub points: i64,
    pub total_points: i64,
    pub unlocked: Vec<AchievementView>,
}

// ---------- заметки ----------

/// Заметка: заголовок и/или текст, цвет, закрепление, категория.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Note {
    /// 0 — новая заметка.
    pub id: i64,
    pub title: String,
    pub body: String,
    pub icon: String,
    /// Цвет карточки `#RRGGBB`; пусто — без цвета.
    pub color: Option<String>,
    pub pinned: bool,
    pub category_id: Option<i64>,
    pub attachments: Vec<Attachment>,
    /// Ставится ядром при сохранении.
    #[serde(with = "dt")]
    pub created: NaiveDateTime,
    /// Ставится ядром при каждом сохранении.
    #[serde(with = "dt")]
    pub updated: NaiveDateTime,
}

impl Default for Note {
    fn default() -> Self {
        let epoch = NaiveDate::from_ymd_opt(2000, 1, 1).and_then(|d| d.and_hms_opt(0, 0, 0)).unwrap();
        Note {
            id: 0,
            title: String::new(),
            body: String::new(),
            icon: "builtin:note".into(),
            color: None,
            pinned: false,
            category_id: None,
            attachments: Vec::new(),
            created: epoch,
            updated: epoch,
        }
    }
}

impl Note {
    pub fn validate(&self) -> Result<()> {
        crate::features::validate_icon(&self.icon)?;
        crate::features::validate_attachments(&self.attachments)?;
        if self.title.trim().is_empty() && self.body.trim().is_empty() {
            return Err(Error::Invalid("Заметка пуста: добавьте заголовок или текст".into()));
        }
        if self.title.chars().count() > 200 {
            return Err(Error::Invalid("Заголовок слишком длинный (максимум 200 символов)".into()));
        }
        if self.body.chars().count() > 20_000 {
            return Err(Error::Invalid("Текст слишком длинный (максимум 20 000 символов)".into()));
        }
        if let Some(c) = &self.color {
            let ok = c.is_empty()
                || (c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit()));
            if !ok {
                return Err(Error::Invalid("Цвет должен быть вида #RRGGBB".into()));
            }
        }
        Ok(())
    }
}
