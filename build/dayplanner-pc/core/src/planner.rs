//! Вся «умная» логика: представления дня/недели, статусы, выполнение,
//! очки, серии, достижения, список будильников и данные для виджетов.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use serde::Serialize;

use crate::achievements::{level_for, DEFS};
use crate::error::{Error, Result};
use crate::model::*;
use crate::recurrence::occurrences;
use crate::store::{comp_key, Store};
use crate::timefmt::{dt, tm};

/// Очки за выполнение вовремя / с опозданием.
pub const POINTS_ON_TIME: i64 = 10;
pub const POINTS_LATE: i64 = 4;

/// Определяет состояние срабатывания напоминания по времени и записи о выполнении.
///
/// * есть отметка «выполнено» — вовремя, если отметили не позже чем через
///   `grace` минут после срока (раньше срока — тоже вовремя), иначе с опозданием;
/// * отметки нет — «ожидается», «пора» (в пределах допуска) или «пропущено».
pub fn compute_status(
    grace_minutes: i64,
    at: NaiveDateTime,
    now: NaiveDateTime,
    completion: Option<&Completion>,
) -> Status {
    let deadline = at + Duration::minutes(grace_minutes);
    match completion {
        Some(c) if c.kind == CompletionKind::Skipped => Status::Skipped,
        Some(c) => {
            if c.completed_at <= deadline {
                Status::DoneOnTime
            } else {
                Status::DoneLate
            }
        }
        None => {
            if now < at {
                Status::Upcoming
            } else if now <= deadline {
                Status::Due
            } else {
                Status::Missed
            }
        }
    }
}

fn midnight(d: NaiveDate) -> NaiveDateTime {
    d.and_hms_opt(0, 0, 0).unwrap()
}

impl Store {
    // ---------- представления ----------

    fn build_days(&self, from: NaiveDate, to: NaiveDate, now: NaiveDateTime) -> Result<Vec<DayView>> {
        let comps = self.completions_between(midnight(from), midnight(to + Duration::days(1)))?;
        let reminders = self.list_reminders(false)?;
        let schedule = self.list_schedule()?;
        let mut days = Vec::new();
        let mut d = from;
        while d <= to {
            let mut entries: Vec<ScheduleEntry> =
                schedule.iter().filter(|e| e.applies_on(d)).cloned().collect();
            entries.sort_by(|a, b| (a.start, a.end).cmp(&(b.start, b.end)));

            let mut items = Vec::new();
            for r in &reminders {
                for at in occurrences(r, d, d) {
                    let c = comps.get(&comp_key(r.id, &at));
                    items.push(DayItem {
                        reminder_id: r.id,
                        title: r.title.clone(),
                        notes: r.notes.clone(),
                        icon: r.icon.clone(),
                        category_id: r.category_id,
                        at,
                        alert: r.alert,
                        status: compute_status(r.grace_minutes, at, now, c),
                    });
                }
            }
            items.sort_by_key(|i| i.at);
            days.push(DayView { date: d, entries, items });
            d = match d.succ_opt() {
                Some(n) => n,
                None => break,
            };
        }
        Ok(days)
    }

    pub fn day_view(&self, date: NaiveDate, now: NaiveDateTime) -> Result<DayView> {
        let mut v = self.build_days(date, date, now)?;
        v.pop().ok_or_else(|| Error::Invalid("неверная дата".into()))
    }

    /// Неделя, в которую входит `date` (с понедельника по воскресенье).
    pub fn week_view(&self, date: NaiveDate, now: NaiveDateTime) -> Result<WeekView> {
        let monday = date - Duration::days(date.weekday().num_days_from_monday() as i64);
        let days = self.build_days(monday, monday + Duration::days(6), now)?;
        Ok(WeekView { start: monday, days })
    }

    /// Напоминания за последние дни, по которым ещё нет отметки (пора или пропущено).
    pub fn pending(&self, now: NaiveDateTime) -> Result<Vec<DayItem>> {
        let today = now.date();
        let days = self.build_days(today - Duration::days(3), today, now)?;
        let mut out: Vec<DayItem> = days
            .into_iter()
            .flat_map(|d| d.items)
            .filter(|i| i.status == Status::Due || i.status == Status::Missed)
            .collect();
        out.sort_by(|a, b| b.at.cmp(&a.at));
        Ok(out)
    }

    // ---------- выполнение ----------

    fn check_occurrence(&self, r: &Reminder, at: NaiveDateTime) -> Result<()> {
        if occurrences(r, at.date(), at.date()).contains(&at) {
            Ok(())
        } else {
            Err(Error::Invalid("У напоминания нет такого срабатывания".into()))
        }
    }

    /// Отметить срабатывание выполненным. Начисляет очки и проверяет достижения.
    pub fn complete(&self, reminder_id: i64, at: NaiveDateTime, now: NaiveDateTime) -> Result<CompleteOutcome> {
        let r = self.get_reminder(reminder_id)?;
        self.check_occurrence(&r, at)?;
        if at - now > Duration::hours(24) {
            return Err(Error::Invalid("Нельзя отметить дело, до которого больше суток".into()));
        }
        if let Some(c) = self.get_completion(reminder_id, &at)? {
            if c.kind == CompletionKind::Done {
                return Err(Error::Invalid("Уже выполнено".into()));
            }
        }
        let on_time = now <= at + Duration::minutes(r.grace_minutes);
        let points = if on_time { POINTS_ON_TIME } else { POINTS_LATE };
        self.put_completion(&Completion {
            reminder_id,
            at,
            kind: CompletionKind::Done,
            completed_at: now,
            points,
        })?;
        let unlocked = self.evaluate_achievements(now)?;
        let total_points = self.stats(now)?.points;
        Ok(CompleteOutcome {
            status: if on_time { Status::DoneOnTime } else { Status::DoneLate },
            points,
            total_points,
            unlocked,
        })
    }

    /// Пропустить срабатывание: не считается ни выполненным, ни пропущенным.
    pub fn skip(&self, reminder_id: i64, at: NaiveDateTime, now: NaiveDateTime) -> Result<()> {
        let r = self.get_reminder(reminder_id)?;
        self.check_occurrence(&r, at)?;
        self.put_completion(&Completion {
            reminder_id,
            at,
            kind: CompletionKind::Skipped,
            completed_at: now,
            points: 0,
        })
    }

    /// Снять отметку (очки за неё пропадают).
    pub fn undo(&self, reminder_id: i64, at: NaiveDateTime) -> Result<()> {
        self.delete_completion(reminder_id, &at)
    }

    // ---------- статистика ----------

    pub fn stats(&self, now: NaiveDateTime) -> Result<Stats> {
        let comps = self.all_completions()?;
        let reminders = self.list_reminders(true)?;
        let grace_by_id: HashMap<i64, i64> = reminders.iter().map(|r| (r.id, r.grace_minutes)).collect();

        let mut s = Stats::default();
        let mut done_by_day: BTreeMap<NaiveDate, u32> = BTreeMap::new();
        let early_limit = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let mut points: i64 = 0;

        for c in &comps {
            points += c.points;
            match c.kind {
                CompletionKind::Skipped => s.skipped += 1,
                CompletionKind::Done => {
                    s.total_done += 1;
                    let grace = grace_by_id.get(&c.reminder_id).copied().unwrap_or(60);
                    if c.completed_at <= c.at + Duration::minutes(grace) {
                        s.on_time += 1;
                    } else {
                        s.late += 1;
                    }
                    if c.at.time() < early_limit {
                        s.early_bird += 1;
                    }
                    if c.at.time().hour() >= 22 {
                        s.night_owl += 1;
                    }
                    *done_by_day.entry(c.at.date()).or_insert(0) += 1;
                }
            }
        }

        // Пропущенные: срок + допуск прошли, а отметки нет.
        let marked: HashSet<(i64, String)> = comps.iter().map(|c| comp_key(c.reminder_id, &c.at)).collect();
        let mut missed_by_day: BTreeMap<NaiveDate, u32> = BTreeMap::new();
        let today = now.date();
        let floor = today - Duration::days(3650);
        for r in reminders.iter().filter(|r| !r.archived) {
            let from = r.start.date().max(floor);
            for at in occurrences(r, from, today) {
                if at + Duration::minutes(r.grace_minutes) < now && !marked.contains(&comp_key(r.id, &at)) {
                    *missed_by_day.entry(at.date()).or_insert(0) += 1;
                    s.missed += 1;
                }
            }
        }

        // Серии: день «хороший», если в нём есть выполненное и нет пропущенного.
        // Дни без дел серию не прерывают.
        let mut days: BTreeMap<NaiveDate, (u32, u32)> = BTreeMap::new();
        for (d, n) in &done_by_day {
            days.entry(*d).or_insert((0, 0)).0 += *n;
        }
        for (d, n) in &missed_by_day {
            days.entry(*d).or_insert((0, 0)).1 += *n;
        }
        let mut run = 0u32;
        for (_, (done, missed)) in days.iter() {
            if *missed > 0 {
                run = 0;
            } else if *done > 0 {
                run += 1;
                s.perfect_days += 1;
                s.best_streak = s.best_streak.max(run);
            }
        }
        for (_, (done, missed)) in days.iter().rev() {
            if *missed > 0 {
                break;
            }
            if *done > 0 {
                s.current_streak += 1;
            }
        }

        s.reminders_created = reminders.len() as u32;
        s.categories_used = reminders.iter().filter_map(|r| r.category_id).collect::<HashSet<i64>>().len() as u32;

        s.points = points + self.achievement_points()?;
        let (level, progress) = level_for(s.points);
        s.level = level;
        s.level_progress = progress;

        let today_view = self.build_days(today, today, now)?;
        if let Some(d) = today_view.first() {
            s.today_total = d.items.len() as u32;
            s.today_done = d.items.iter().filter(|i| i.status.is_done()).count() as u32;
        }
        Ok(s)
    }

    // ---------- достижения ----------

    fn achievement_view(def: &crate::achievements::Def, stats: &Stats, at: Option<String>) -> AchievementView {
        let value = def.metric.value(stats);
        AchievementView {
            id: def.id.to_string(),
            icon: def.icon.to_string(),
            title: def.title.to_string(),
            description: def.desc.to_string(),
            reward: def.reward,
            target: def.target,
            progress: value.min(def.target),
            unlocked: at.is_some(),
            unlocked_at: at,
        }
    }

    pub fn achievements(&self, now: NaiveDateTime) -> Result<Vec<AchievementView>> {
        let stats = self.stats(now)?;
        let got = self.unlocked_map()?;
        Ok(DEFS
            .iter()
            .map(|d| Store::achievement_view(d, &stats, got.get(d.id).cloned()))
            .collect())
    }

    /// Выдаёт все достижения, условия которых выполнены. Возвращает новые.
    pub fn evaluate_achievements(&self, now: NaiveDateTime) -> Result<Vec<AchievementView>> {
        let mut fresh = Vec::new();
        // Награда добавляет очки, а очки поднимают уровень — повторяем, пока есть новые.
        for _ in 0..5 {
            let stats = self.stats(now)?;
            let got = self.unlocked_map()?;
            let mut any = false;
            for def in DEFS {
                if !got.contains_key(def.id) && def.metric.value(&stats) >= def.target {
                    self.unlock(def.id, &now, def.reward)?;
                    fresh.push(Store::achievement_view(def, &stats, Some(dt::format(&now))));
                    any = true;
                }
            }
            if !any {
                break;
            }
        }
        Ok(fresh)
    }

    // ---------- интеграция с системой ----------

    /// Ближайшие срабатывания без отметки — их нужно запланировать в системе.
    pub fn upcoming_alarms(&self, now: NaiveDateTime, days: i64, limit: usize) -> Result<Vec<AlarmItem>> {
        let from = now.date();
        let to = from + Duration::days(days);
        let comps = self.completions_between(midnight(from), midnight(to + Duration::days(1)))?;
        let mut out = Vec::new();
        for r in self.list_reminders(false)? {
            for at in occurrences(&r, from, to) {
                if at < now || comps.contains_key(&comp_key(r.id, &at)) {
                    continue;
                }
                out.push(AlarmItem {
                    key: format!("{}|{}", r.id, dt::format(&at)),
                    reminder_id: r.id,
                    at,
                    title: r.title.clone(),
                    notes: r.notes.clone(),
                    icon: r.icon.clone(),
                    alert: r.alert,
                });
            }
        }
        out.sort_by_key(|a| a.at);
        out.truncate(limit);
        Ok(out)
    }

    /// Данные для виджетов рабочего стола: 7 дней вперёд плюс тема оформления.
    pub fn widget_snapshot(&self, now: NaiveDateTime) -> Result<WidgetSnapshot> {
        let today = now.date();
        let views = self.build_days(today, today + Duration::days(6), now)?;
        let days = views
            .into_iter()
            .map(|v| WidgetDay {
                date: v.date.format("%Y-%m-%d").to_string(),
                items: v
                    .items
                    .iter()
                    .map(|i| WidgetItem {
                        time: i.at.format("%H:%M").to_string(),
                        title: i.title.clone(),
                        icon: i.icon.clone(),
                        done: i.status.is_done() || i.status == Status::Skipped,
                        overdue: i.status == Status::Missed || i.status == Status::Due,
                    })
                    .collect(),
                schedule: v
                    .entries
                    .iter()
                    .map(|e| WidgetSlot {
                        start: tm::format(&e.start),
                        end: tm::format(&e.end),
                        title: e.title.clone(),
                        icon: e.icon.clone(),
                        location: e.location.clone(),
                    })
                    .collect(),
            })
            .collect();
        let stats = self.stats(now)?;
        Ok(WidgetSnapshot {
            generated: dt::format(&now),
            theme: self.setting_or("theme", "material")?,
            accent: self.setting_or("accent", "#6750A4")?,
            mode: self.setting_or("mode", "auto")?,
            days,
            streak: stats.current_streak,
            points: stats.points,
            level: stats.level,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WidgetItem {
    pub time: String,
    pub title: String,
    pub icon: String,
    pub done: bool,
    pub overdue: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WidgetSlot {
    pub start: String,
    pub end: String,
    pub title: String,
    pub icon: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WidgetDay {
    pub date: String,
    pub items: Vec<WidgetItem>,
    pub schedule: Vec<WidgetSlot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WidgetSnapshot {
    pub generated: String,
    pub theme: String,
    pub accent: String,
    pub mode: String,
    pub days: Vec<WidgetDay>,
    pub streak: u32,
    pub points: i64,
    pub level: u32,
}
