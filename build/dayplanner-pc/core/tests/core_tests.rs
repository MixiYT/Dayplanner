use std::sync::Mutex;

use chrono::{NaiveDate, NaiveDateTime};
use planner_core::achievements::level_for;
use planner_core::api::dispatch;
use planner_core::model::*;
use planner_core::recurrence::occurrences;
use planner_core::Store;
use serde_json::json;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn t(y: i32, m: u32, day: u32, h: u32, mi: u32) -> NaiveDateTime {
    d(y, m, day).and_hms_opt(h, mi, 0).unwrap()
}

fn rem(title: &str, start: NaiveDateTime, repeat: Repeat) -> Reminder {
    Reminder { title: title.into(), start, repeat, ..Reminder::default() }
}

fn store() -> Store {
    Store::open_in_memory().unwrap()
}

// ---------- повторения ----------

#[test]
fn weekdays_repeat_picks_only_chosen_days() {
    // 2026-10-26 — понедельник
    let r = rem("Спорт", t(2026, 10, 26, 9, 0), Repeat::Weekdays { days: vec![1, 3] });
    let got = occurrences(&r, d(2026, 10, 26), d(2026, 11, 1));
    assert_eq!(got, vec![t(2026, 10, 26, 9, 0), t(2026, 10, 28, 9, 0)]);
}

#[test]
fn monthly_repeat_clamps_to_last_day() {
    let r = rem("Платёж", t(2026, 1, 31, 10, 0), Repeat::Monthly);
    assert_eq!(occurrences(&r, d(2026, 2, 1), d(2026, 2, 28)), vec![t(2026, 2, 28, 10, 0)]);
    assert_eq!(occurrences(&r, d(2026, 3, 1), d(2026, 3, 31)), vec![t(2026, 3, 31, 10, 0)]);
}

#[test]
fn repeat_until_is_respected() {
    let mut r = rem("Курс", t(2026, 10, 26, 9, 0), Repeat::Daily);
    r.repeat_until = Some(d(2026, 10, 28));
    assert_eq!(occurrences(&r, d(2026, 10, 26), d(2026, 10, 31)).len(), 3);
}

#[test]
fn one_time_reminder_fires_once() {
    let r = rem("Звонок", t(2026, 10, 26, 9, 0), Repeat::None);
    assert_eq!(occurrences(&r, d(2026, 10, 1), d(2026, 12, 31)), vec![t(2026, 10, 26, 9, 0)]);
}

// ---------- проверка данных ----------

#[test]
fn validation_rejects_bad_input() {
    let s = store();
    assert!(s.save_reminder(&rem("  ", t(2026, 10, 26, 9, 0), Repeat::None)).is_err());
    assert!(s
        .save_reminder(&rem("x", t(2026, 10, 26, 9, 0), Repeat::Weekdays { days: vec![] }))
        .is_err());
    assert!(s
        .save_reminder(&rem("x", t(2026, 10, 26, 9, 0), Repeat::Weekdays { days: vec![8] }))
        .is_err());

    let bad = ScheduleEntry {
        title: "Лекция".into(),
        weekday: Some(1),
        start: chrono::NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        end: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        ..ScheduleEntry::default()
    };
    assert!(s.save_schedule(&bad).is_err());
    let both = ScheduleEntry { title: "x".into(), weekday: Some(1), date: Some(d(2026, 10, 26)), ..ScheduleEntry::default() };
    assert!(s.save_schedule(&both).is_err());
}

// ---------- статусы и выполнение ----------

#[test]
fn status_changes_over_time() {
    let s = store();
    let mut r = rem("Лекарство", t(2026, 10, 26, 10, 0), Repeat::None);
    r.grace_minutes = 30;
    let r = s.save_reminder(&r).unwrap();
    let status = |now: NaiveDateTime| s.day_view(d(2026, 10, 26), now).unwrap().items[0].status;

    assert_eq!(status(t(2026, 10, 26, 9, 0)), Status::Upcoming);
    assert_eq!(status(t(2026, 10, 26, 10, 10)), Status::Due);
    assert_eq!(status(t(2026, 10, 26, 10, 31)), Status::Missed);

    s.complete(r.id, t(2026, 10, 26, 10, 0), t(2026, 10, 26, 10, 10)).unwrap();
    assert_eq!(status(t(2026, 10, 26, 12, 0)), Status::DoneOnTime);
}

#[test]
fn complete_on_time_gives_points_and_first_achievement() {
    let s = store();
    let mut r = rem("Зарядка", t(2026, 10, 26, 10, 0), Repeat::None);
    r.grace_minutes = 30;
    let r = s.save_reminder(&r).unwrap();

    let out = s.complete(r.id, t(2026, 10, 26, 10, 0), t(2026, 10, 26, 10, 20)).unwrap();
    assert_eq!(out.status, Status::DoneOnTime);
    assert_eq!(out.points, 10);
    assert!(out.unlocked.iter().any(|a| a.id == "first_step"));
    // 10 за выполнение + 5 награда за «Первый шаг»
    assert_eq!(out.total_points, 15);
}

#[test]
fn complete_after_grace_is_late() {
    let s = store();
    let mut r = rem("Отчёт", t(2026, 10, 26, 10, 0), Repeat::None);
    r.grace_minutes = 30;
    let r = s.save_reminder(&r).unwrap();
    let out = s.complete(r.id, t(2026, 10, 26, 10, 0), t(2026, 10, 26, 11, 0)).unwrap();
    assert_eq!(out.status, Status::DoneLate);
    assert_eq!(out.points, 4);
}

#[test]
fn cannot_complete_twice_or_wrong_time_or_far_future() {
    let s = store();
    let r = s.save_reminder(&rem("Дело", t(2026, 10, 26, 10, 0), Repeat::None)).unwrap();
    let now = t(2026, 10, 26, 10, 5);
    s.complete(r.id, t(2026, 10, 26, 10, 0), now).unwrap();
    assert!(s.complete(r.id, t(2026, 10, 26, 10, 0), now).is_err(), "повторная отметка");
    assert!(s.complete(r.id, t(2026, 10, 26, 10, 5), now).is_err(), "нет такого срабатывания");

    let daily = s.save_reminder(&rem("Ежедневное", t(2026, 10, 26, 10, 0), Repeat::Daily)).unwrap();
    // до срабатывания 26 часов — слишком рано
    assert!(s.complete(daily.id, t(2026, 10, 27, 10, 0), t(2026, 10, 26, 8, 0)).is_err());
}

#[test]
fn skipped_is_not_missed_and_undo_removes_points() {
    let s = store();
    let r = s.save_reminder(&rem("Дело", t(2026, 10, 26, 10, 0), Repeat::None)).unwrap();
    let later = t(2026, 10, 27, 12, 0);
    assert_eq!(s.stats(later).unwrap().missed, 1);
    s.skip(r.id, t(2026, 10, 26, 10, 0), later).unwrap();
    let st = s.stats(later).unwrap();
    assert_eq!(st.missed, 0);
    assert_eq!(st.skipped, 1);

    s.complete(r.id, t(2026, 10, 26, 10, 0), later).unwrap();
    assert!(s.stats(later).unwrap().points > 0);
    s.undo(r.id, t(2026, 10, 26, 10, 0)).unwrap();
    assert_eq!(s.stats(later).unwrap().total_done, 0);
}

#[test]
fn deleting_reminder_keeps_history() {
    let s = store();
    let r = s.save_reminder(&rem("Дело", t(2026, 10, 26, 10, 0), Repeat::None)).unwrap();
    s.complete(r.id, t(2026, 10, 26, 10, 0), t(2026, 10, 26, 10, 1)).unwrap();
    s.delete_reminder(r.id).unwrap();
    assert_eq!(s.stats(t(2026, 10, 26, 12, 0)).unwrap().total_done, 1);
}

// ---------- серии и достижения ----------

fn daily_with_three_days_done(s: &Store) -> i64 {
    let r = s.save_reminder(&rem("Привычка", t(2026, 10, 20, 9, 0), Repeat::Daily)).unwrap();
    for day in 20..=22 {
        s.complete(r.id, t(2026, 10, day, 9, 0), t(2026, 10, day, 9, 5)).unwrap();
    }
    r.id
}

#[test]
fn streak_counts_clean_days_and_breaks_on_miss() {
    let s = store();
    daily_with_three_days_done(&s);

    let st = s.stats(t(2026, 10, 22, 12, 0)).unwrap();
    assert_eq!(st.current_streak, 3);
    assert_eq!(st.best_streak, 3);
    assert_eq!(st.missed, 0);

    // 23-го и 24-го отметок нет — серия прервана, рекорд остаётся
    let st = s.stats(t(2026, 10, 24, 12, 0)).unwrap();
    assert_eq!(st.current_streak, 0);
    assert_eq!(st.best_streak, 3);
    assert_eq!(st.missed, 2);
}

#[test]
fn today_pending_item_does_not_break_streak() {
    let s = store();
    daily_with_three_days_done(&s);
    // 23-го в 08:00 сегодняшнее дело ещё впереди
    assert_eq!(s.stats(t(2026, 10, 23, 8, 0)).unwrap().current_streak, 3);
}

#[test]
fn pending_lists_recent_unmarked_items() {
    let s = store();
    daily_with_three_days_done(&s);
    let p = s.pending(t(2026, 10, 24, 12, 0)).unwrap();
    assert_eq!(p.len(), 2);
    assert!(p.iter().all(|i| i.status == Status::Missed));
}

#[test]
fn streak_achievement_is_unlocked_with_reward() {
    let s = store();
    daily_with_three_days_done(&s);
    let list = s.achievements(t(2026, 10, 22, 12, 0)).unwrap();
    let streak3 = list.iter().find(|a| a.id == "streak_3").unwrap();
    assert!(streak3.unlocked);
    assert_eq!(streak3.progress, 3);
    let streak7 = list.iter().find(|a| a.id == "streak_7").unwrap();
    assert!(!streak7.unlocked);
    assert_eq!(streak7.progress, 3);
}

#[test]
fn organizer_achievement_needs_three_categories() {
    let s = store();
    let now = t(2026, 10, 26, 8, 0);
    for cat in [1, 2] {
        let mut r = rem("x", t(2026, 10, 26, 9, 0), Repeat::None);
        r.category_id = Some(cat);
        s.save_reminder(&r).unwrap();
    }
    assert!(s.evaluate_achievements(now).unwrap().iter().all(|a| a.id != "organizer"));
    let mut r = rem("x", t(2026, 10, 26, 9, 0), Repeat::None);
    r.category_id = Some(3);
    s.save_reminder(&r).unwrap();
    assert!(s.evaluate_achievements(now).unwrap().iter().any(|a| a.id == "organizer"));
    // повторно не выдаётся
    assert!(s.evaluate_achievements(now).unwrap().is_empty());
}

#[test]
fn levels_grow_with_points() {
    assert_eq!(level_for(0), (1, 0.0));
    assert_eq!(level_for(49).0, 1);
    assert_eq!(level_for(50).0, 2);
    assert_eq!(level_for(200).0, 3);
}

// ---------- расписание ----------

#[test]
fn week_view_contains_weekly_and_one_off_entries() {
    let s = store();
    let weekly = ScheduleEntry {
        title: "Матан".into(),
        weekday: Some(1),
        start: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        end: chrono::NaiveTime::from_hms_opt(10, 30, 0).unwrap(),
        ..ScheduleEntry::default()
    };
    s.save_schedule(&weekly).unwrap();
    let one_off = ScheduleEntry {
        title: "Консультация".into(),
        date: Some(d(2026, 10, 27)),
        ..ScheduleEntry::default()
    };
    s.save_schedule(&one_off).unwrap();

    let week = s.week_view(d(2026, 10, 28), t(2026, 10, 26, 8, 0)).unwrap();
    assert_eq!(week.start, d(2026, 10, 26));
    assert_eq!(week.days.len(), 7);
    assert_eq!(week.days[0].entries.len(), 1);
    assert_eq!(week.days[1].entries.len(), 1);
    assert_eq!(week.days[1].entries[0].title, "Консультация");
    assert_eq!(week.days[2].entries.len(), 0);

    // следующая неделя: только еженедельное занятие
    let next = s.week_view(d(2026, 11, 2), t(2026, 10, 26, 8, 0)).unwrap();
    assert_eq!(next.days[0].entries.len(), 1);
    assert_eq!(next.days[1].entries.len(), 0);
}

// ---------- системная интеграция ----------

#[test]
fn upcoming_alarms_skip_done_and_past_and_respect_limit() {
    let s = store();
    let r = s.save_reminder(&rem("Таблетки", t(2026, 10, 26, 9, 0), Repeat::Daily)).unwrap();
    let now = t(2026, 10, 26, 8, 0);
    assert_eq!(s.upcoming_alarms(now, 2, 100).unwrap().len(), 3);

    // досрочно отметили сегодняшнее срабатывание
    s.complete(r.id, t(2026, 10, 26, 9, 0), now).unwrap();
    let alarms = s.upcoming_alarms(now, 2, 100).unwrap();
    assert_eq!(alarms.len(), 2);
    assert_eq!(alarms[0].at, t(2026, 10, 27, 9, 0));
    assert_eq!(alarms[0].key, format!("{}|2026-10-27T09:00:00", r.id));

    assert_eq!(s.upcoming_alarms(now, 2, 1).unwrap().len(), 1);
    // позже срока ничего из прошлого не планируется
    assert!(s.upcoming_alarms(t(2026, 10, 30, 12, 0), 0, 100).unwrap().is_empty());
}

#[test]
fn widget_snapshot_has_seven_days_and_theme() {
    let s = store();
    s.save_reminder(&rem("Дело", t(2026, 10, 26, 9, 0), Repeat::Daily)).unwrap();
    s.set_setting("accent", "#FF5722").unwrap();
    let snap = s.widget_snapshot(t(2026, 10, 26, 8, 0)).unwrap();
    assert_eq!(snap.days.len(), 7);
    assert_eq!(snap.days[0].date, "2026-10-26");
    assert_eq!(snap.days[0].items.len(), 1);
    assert_eq!(snap.accent, "#FF5722");
}

// ---------- API для интерфейса ----------

#[test]
fn api_roundtrip_like_the_ui_uses_it() {
    let store = Mutex::new(Store::open_in_memory().unwrap());
    let now = t(2026, 10, 26, 8, 0);

    // интерфейс присылает время без секунд
    let saved = dispatch(
        &store,
        now,
        "reminders.save",
        json!({"reminder": {
            "title": "Таблетки", "icon": "💊", "category_id": 3,
            "start": "2026-10-26T09:00", "alert": "alarm",
            "repeat": {"type": "daily"}
        }}),
    )
    .unwrap();
    assert_eq!(saved["alert"], "alarm");
    assert_eq!(saved["start"], "2026-10-26T09:00:00");
    let id = saved["id"].as_i64().unwrap();
    assert!(id > 0);

    let day = dispatch(&store, now, "day", json!({"date": "2026-10-26"})).unwrap();
    assert_eq!(day["items"].as_array().unwrap().len(), 1);
    assert_eq!(day["items"][0]["status"], "upcoming");

    let done = dispatch(
        &store,
        t(2026, 10, 26, 9, 0),
        "complete",
        json!({"reminder_id": id, "at": "2026-10-26T09:00"}),
    )
    .unwrap();
    assert_eq!(done["status"], "done_on_time");

    let stats = dispatch(&store, t(2026, 10, 26, 9, 1), "stats", json!({})).unwrap();
    assert_eq!(stats["total_done"], 1);

    assert!(dispatch(&store, now, "no.such.command", json!({})).is_err());
}

#[test]
fn settings_have_defaults_and_token_is_protected() {
    let store = Mutex::new(Store::open_in_memory().unwrap());
    let now = t(2026, 10, 26, 8, 0);
    let s = dispatch(&store, now, "settings.get", json!({})).unwrap();
    assert_eq!(s["theme"], "material");
    assert_eq!(s["online.registered"], "false");

    dispatch(&store, now, "settings.set", json!({"key": "accent", "value": "#00BCD4"})).unwrap();
    let s = dispatch(&store, now, "settings.get", json!({})).unwrap();
    assert_eq!(s["accent"], "#00BCD4");

    assert!(dispatch(&store, now, "settings.set", json!({"key": "online.token", "value": "x"})).is_err());
}


// ---------- заметки ----------

#[test]
fn notes_crud_sorting_and_search() {
    let s = store();
    let n1 = s
        .save_note(&Note { title: "Покупки".into(), body: "молоко, хлеб".into(), ..Note::default() }, t(2026, 10, 29, 9, 0))
        .unwrap();
    let n2 = s
        .save_note(
            &Note { title: "Идеи".into(), body: "Приложение".into(), pinned: true, category_id: Some(5), ..Note::default() },
            t(2026, 10, 29, 8, 0),
        )
        .unwrap();
    assert!(n1.id > 0 && n2.id > n1.id);
    assert_eq!(n1.created, t(2026, 10, 29, 9, 0));

    // закреплённая выше, хотя изменена раньше
    let all = s.list_notes(None, None).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].title, "Идеи");
    assert_eq!(all[1].title, "Покупки");

    // поиск без учёта регистра, в том числе по-русски, и по тексту
    assert_eq!(s.list_notes(Some("МОЛОКО"), None).unwrap().len(), 1);
    assert_eq!(s.list_notes(Some("  идеи "), None).unwrap().len(), 1);
    assert_eq!(s.list_notes(Some("нет такого"), None).unwrap().len(), 0);
    assert_eq!(s.list_notes(None, Some(5)).unwrap().len(), 1);

    // редактирование: время создания остаётся, время изменения обновляется
    let mut e = n1.clone();
    e.body = "хлеб".into();
    let saved = s.save_note(&e, t(2026, 10, 29, 10, 0)).unwrap();
    assert_eq!(saved.created, t(2026, 10, 29, 9, 0));
    assert_eq!(saved.updated, t(2026, 10, 29, 10, 0));
    assert_eq!(s.list_notes(Some("молоко"), None).unwrap().len(), 0);

    s.delete_note(n2.id).unwrap();
    assert_eq!(s.list_notes(None, None).unwrap().len(), 1);
}

#[test]
fn note_validation_and_updating_missing_note() {
    let s = store();
    let now = t(2026, 10, 29, 9, 0);
    assert!(s.save_note(&Note::default(), now).is_err(), "пустая заметка");
    assert!(s.save_note(&Note { body: "   \n ".into(), ..Note::default() }, now).is_err(), "только пробелы");
    assert!(s.save_note(&Note { body: "x".into(), color: Some("red".into()), ..Note::default() }, now).is_err());
    assert!(s.save_note(&Note { body: "x".into(), color: Some("#12345".into()), ..Note::default() }, now).is_err());
    assert!(s.save_note(&Note { body: "x".repeat(20_001), ..Note::default() }, now).is_err());
    // заголовок или текст по отдельности допустимы, пустой цвет считается «без цвета»
    let only_body = s.save_note(&Note { body: "текст".into(), color: Some(String::new()), ..Note::default() }, now).unwrap();
    assert_eq!(only_body.color, None);
    assert!(s.save_note(&Note { title: "Заголовок".into(), color: Some("#A1b2C3".into()), ..Note::default() }, now).is_ok());
    // обновить несуществующую нельзя
    assert!(s.save_note(&Note { id: 999, title: "x".into(), ..Note::default() }, now).is_err());
}

#[test]
fn deleting_category_clears_it_in_notes() {
    let s = store();
    let n = s
        .save_note(&Note { title: "x".into(), category_id: Some(2), ..Note::default() }, t(2026, 10, 29, 9, 0))
        .unwrap();
    s.delete_category(2).unwrap();
    assert_eq!(s.get_note(n.id).unwrap().category_id, None);
}

#[test]
fn notes_api_roundtrip_like_the_ui_uses_it() {
    let store = Mutex::new(Store::open_in_memory().unwrap());
    let now = t(2026, 10, 29, 9, 30);
    // интерфейс присылает заметку без created/updated
    let saved = dispatch(
        &store,
        now,
        "notes.save",
        json!({"note": {"title": "Список", "body": "1. хлеб\n2. молоко", "color": "#43A047", "pinned": true, "category_id": 4}}),
    )
    .unwrap();
    assert_eq!(saved["pinned"], true);
    assert_eq!(saved["updated"], "2026-10-29T09:30:00");
    let id = saved["id"].as_i64().unwrap();

    let list = dispatch(&store, now, "notes.list", json!({"query": null, "category_id": null})).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["body"], "1. хлеб\n2. молоко");
    let found = dispatch(&store, now, "notes.list", json!({"query": "МОЛОКО"})).unwrap();
    assert_eq!(found.as_array().unwrap().len(), 1);
    let none = dispatch(&store, now, "notes.list", json!({"category_id": 1})).unwrap();
    assert_eq!(none.as_array().unwrap().len(), 0);

    // сохранённую заметку интерфейс присылает обратно целиком (с created и updated)
    let mut again = list[0].clone();
    again["title"] = json!("Список покупок");
    let edited = dispatch(&store, t(2026, 10, 29, 11, 0), "notes.save", json!({ "note": again })).unwrap();
    assert_eq!(edited["created"], "2026-10-29T09:30:00");
    assert_eq!(edited["updated"], "2026-10-29T11:00:00");

    dispatch(&store, now, "notes.delete", json!({"id": id})).unwrap();
    assert_eq!(dispatch(&store, now, "notes.list", json!({})).unwrap().as_array().unwrap().len(), 0);
}

// ---------- синхронизация ----------

use planner_core::sync::{b64_decode_for_tests, b64_encode_for_tests};

fn dev(clock: i64) -> Store {
    let s = Store::open_in_memory().unwrap();
    s.set_test_clock(Some(clock));
    s
}

/// Переносит все данные с одного устройства на другое (как это делает файл или локальная сеть).
fn sync_into(from: &Store, to: &Store) -> planner_core::sync::MergeStats {
    let p = from.make_payload(0, "data").unwrap();
    to.apply_payload(&p).unwrap()
}

fn both_ways(a: &Store, b: &Store) {
    sync_into(a, b);
    sync_into(b, a);
}

fn note(title: &str) -> Note {
    Note { title: title.into(), body: "текст".into(), ..Note::default() }
}

#[test]
fn base64_roundtrip_for_all_small_lengths() {
    for n in 0..40usize {
        let data: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
        let enc = b64_encode_for_tests(&data);
        assert_eq!(b64_decode_for_tests(&enc).unwrap(), data, "длина {n}");
    }
    assert!(b64_decode_for_tests("abc").is_err());
    assert!(b64_decode_for_tests("ab$d").is_err());
}

#[test]
fn group_code_format_and_encryption() {
    use planner_core::sync::*;
    let code = generate_code();
    assert_eq!(code.len(), 19);
    assert_eq!(normalize_code(&code).unwrap(), code);
    assert_eq!(normalize_code(&code.to_lowercase().replace('-', " ")).unwrap(), code);
    assert!(normalize_code("ABCD").is_err());
    assert!(normalize_code("0000-0000-0000-0000").is_err(), "нули и буквы O/I в алфавит не входят");
    let k1 = derive_keys(&code);
    let k2 = derive_keys(&generate_code());
    assert_ne!(k1.topic, k2.topic);
    assert_eq!(k1.topic, derive_keys(&code).topic);
    let sealed = seal(&k1.key, "секрет".as_bytes()).unwrap();
    assert_eq!(open(&k1.key, &sealed).unwrap(), "секрет".as_bytes());
    assert!(open(&k2.key, &sealed).is_err(), "чужой код не расшифровывает");
    let mut broken = sealed.clone();
    let last = broken.len() - 1;
    broken[last] ^= 1;
    assert!(open(&k1.key, &broken).is_err(), "повреждение обнаруживается");
    // два шифрования одного текста различаются (случайный nonce)
    assert_ne!(seal(&k1.key, b"x").unwrap(), seal(&k1.key, b"x").unwrap());
}

#[test]
fn default_categories_merge_instead_of_doubling() {
    let a = dev(1000);
    let b = dev(2000);
    both_ways(&a, &b);
    assert_eq!(a.list_categories().unwrap().len(), 5);
    assert_eq!(b.list_categories().unwrap().len(), 5);
}

#[test]
fn records_transfer_with_different_local_ids_and_category_links() {
    let a = dev(10_000);
    let b = dev(20_000);
    // у B уже есть свои записи, поэтому числовые id на устройствах расходятся
    b.save_note(&note("своя заметка B"), t(2026, 10, 1, 9, 0)).unwrap();
    b.save_reminder(&rem("своё дело B", t(2026, 10, 1, 9, 0), Repeat::None)).unwrap();

    let cat = a.save_category(&Category { id: 0, name: "Спорт".into(), color: "#112233".into(), icon: "🏃".into() }).unwrap();
    let n = a.save_note(&Note { category_id: Some(cat.id), pinned: true, ..note("заметка A") }, t(2026, 10, 2, 9, 0)).unwrap();
    let r = a.save_reminder(&Reminder { category_id: Some(cat.id), ..rem("дело A", t(2026, 10, 3, 8, 0), Repeat::Daily) }).unwrap();
    let e = a
        .save_schedule(&ScheduleEntry { title: "Лекция".into(), weekday: Some(2), start: tmv(9, 0), end: tmv(10, 30), category_id: Some(cat.id), ..ScheduleEntry::default() })
        .unwrap();
    assert!(n.id > 0 && r.id > 0 && e.id > 0);

    let stats = sync_into(&a, &b);
    assert!(stats.applied >= 4, "{stats:?}");
    assert_eq!(stats.rejected, 0);

    let bcat = b.list_categories().unwrap().into_iter().find(|c| c.name == "Спорт").expect("категория");
    let bnotes = b.list_notes(None, None).unwrap();
    assert_eq!(bnotes.len(), 2);
    let got = bnotes.iter().find(|x| x.title == "заметка A").unwrap();
    assert!(got.pinned);
    assert_eq!(got.category_id, Some(bcat.id), "ссылка на категорию переведена в локальный id");
    let brem = b.list_reminders(true).unwrap();
    assert_eq!(brem.len(), 2);
    assert_eq!(brem.iter().find(|x| x.title == "дело A").unwrap().category_id, Some(bcat.id));
    assert_eq!(b.list_schedule().unwrap().len(), 1);

    // повторная передача ничего не меняет
    let again = sync_into(&a, &b);
    assert_eq!(again.applied, 0, "{again:?}");
    assert_eq!(b.list_notes(None, None).unwrap().len(), 2);
}

fn tmv(h: u32, m: u32) -> chrono::NaiveTime {
    chrono::NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

#[test]
fn last_write_wins_and_both_sides_converge() {
    let a = dev(10_000);
    let b = dev(10_000);
    let n = a.save_note(&note("исходная"), t(2026, 10, 1, 9, 0)).unwrap();
    sync_into(&a, &b);
    let bn = b.list_notes(None, None).unwrap().remove(0);

    // A правит раньше, B позже: побеждает B
    a.set_test_clock(Some(30_000));
    a.save_note(&Note { title: "правка A".into(), ..n.clone() }, t(2026, 10, 1, 10, 0)).unwrap();
    b.set_test_clock(Some(40_000));
    b.save_note(&Note { title: "правка B".into(), ..bn.clone() }, t(2026, 10, 1, 10, 5)).unwrap();
    both_ways(&a, &b);
    assert_eq!(a.list_notes(None, None).unwrap()[0].title, "правка B");
    assert_eq!(b.list_notes(None, None).unwrap()[0].title, "правка B");
    assert_eq!(a.list_notes(None, None).unwrap().len(), 1);
}

#[test]
fn deletions_propagate_and_newer_edit_beats_older_deletion() {
    let a = dev(10_000);
    let b = dev(10_000);
    let n = a.save_note(&note("временная"), t(2026, 10, 1, 9, 0)).unwrap();
    let r = a.save_reminder(&rem("дело", t(2026, 10, 1, 9, 0), Repeat::None)).unwrap();
    both_ways(&a, &b);
    assert_eq!(b.list_notes(None, None).unwrap().len(), 1);

    a.set_test_clock(Some(50_000));
    a.delete_note(n.id).unwrap();
    a.delete_reminder(r.id).unwrap();
    both_ways(&a, &b);
    assert!(b.list_notes(None, None).unwrap().is_empty());
    assert!(b.list_reminders(true).unwrap().is_empty());

    // заметку удалили на A (время 60 000), но на B её позже изменили (70 000): правка сильнее
    let c = a.save_note(&note("вторая"), t(2026, 10, 2, 9, 0)).unwrap();
    both_ways(&a, &b);
    a.set_test_clock(Some(60_000));
    a.delete_note(c.id).unwrap();
    b.set_test_clock(Some(70_000));
    let bc = b.list_notes(None, None).unwrap().remove(0);
    b.save_note(&Note { body: "дописано".into(), ..bc }, t(2026, 10, 2, 9, 30)).unwrap();
    both_ways(&a, &b);
    assert_eq!(a.list_notes(None, None).unwrap().len(), 1);
    assert_eq!(a.list_notes(None, None).unwrap()[0].body, "дописано");
    assert_eq!(b.list_notes(None, None).unwrap().len(), 1);
}

#[test]
fn local_ids_of_deleted_records_are_not_reused() {
    let a = dev(10_000);
    let n1 = a.save_note(&note("1"), t(2026, 10, 1, 9, 0)).unwrap();
    a.delete_note(n1.id).unwrap();
    let n2 = a.save_note(&note("2"), t(2026, 10, 1, 9, 0)).unwrap();
    assert!(n2.id > n1.id, "id удалённой заметки не выдаётся снова");
}

#[test]
fn completions_sync_and_undo_propagates() {
    let a = dev(10_000);
    let b = dev(10_000);
    b.save_reminder(&rem("лишнее на B", t(2026, 10, 1, 7, 0), Repeat::None)).unwrap();
    let r = a.save_reminder(&rem("зарядка", t(2026, 10, 27, 8, 0), Repeat::Daily)).unwrap();
    both_ways(&a, &b);
    let br = b.list_reminders(true).unwrap().into_iter().find(|x| x.title == "зарядка").unwrap();
    assert_ne!(r.id, br.id, "числовые id разные");

    let at = t(2026, 10, 27, 8, 0);
    a.set_test_clock(Some(20_000));
    a.complete(r.id, at, t(2026, 10, 27, 8, 5)).unwrap();
    both_ways(&a, &b);
    let view = b.day_view(d(2026, 10, 27), t(2026, 10, 27, 12, 0)).unwrap();
    let item = view.items.iter().find(|i| i.title == "зарядка").unwrap();
    assert_eq!(item.status, Status::DoneOnTime);
    assert_eq!(b.stats(t(2026, 10, 27, 12, 0)).unwrap().total_done, 1);

    // отмена на B доходит до A
    b.set_test_clock(Some(30_000));
    b.undo(br.id, at).unwrap();
    both_ways(&a, &b);
    let view = a.day_view(d(2026, 10, 27), t(2026, 10, 27, 12, 0)).unwrap();
    assert_ne!(view.items.iter().find(|i| i.title == "зарядка").unwrap().status, Status::DoneOnTime);

    // повторная отметка после отмены снова побеждает
    a.set_test_clock(Some(40_000));
    a.complete(r.id, at, t(2026, 10, 27, 9, 0)).unwrap();
    both_ways(&a, &b);
    assert_eq!(b.stats(t(2026, 10, 27, 12, 0)).unwrap().total_done, 1);
}

#[test]
fn completion_of_a_reminder_deleted_elsewhere_keeps_history() {
    let a = dev(10_000);
    let b = dev(10_000);
    let r = a.save_reminder(&rem("разовое", t(2026, 10, 20, 8, 0), Repeat::None)).unwrap();
    a.complete(r.id, t(2026, 10, 20, 8, 0), t(2026, 10, 20, 8, 1)).unwrap();
    a.set_test_clock(Some(20_000));
    a.delete_reminder(r.id).unwrap();
    // B вообще не видел это напоминание: история всё равно доезжает
    both_ways(&a, &b);
    assert!(b.list_reminders(true).unwrap().is_empty());
    assert_eq!(b.stats(t(2026, 10, 21, 12, 0)).unwrap().total_done, 1);
}

#[test]
fn deleting_category_on_one_device_clears_it_on_the_other() {
    let a = dev(10_000);
    let b = dev(10_000);
    let c = a.save_category(&Category { id: 0, name: "Временная".into(), color: "#445566".into(), icon: "🧪".into() }).unwrap();
    a.save_note(&Note { category_id: Some(c.id), ..note("с категорией") }, t(2026, 10, 1, 9, 0)).unwrap();
    both_ways(&a, &b);
    assert_eq!(b.list_notes(None, None).unwrap()[0].category_id.is_some(), true);
    a.set_test_clock(Some(50_000));
    a.delete_category(c.id).unwrap();
    both_ways(&a, &b);
    assert!(b.list_categories().unwrap().iter().all(|x| x.name != "Временная"));
    assert_eq!(b.list_notes(None, None).unwrap()[0].category_id, None);
}

#[test]
fn unlocked_achievements_merge_with_earliest_time() {
    let a = dev(10_000);
    let b = dev(10_000);
    a.unlock("first_step", &t(2026, 10, 2, 9, 0), 20).unwrap();
    b.unlock("first_step", &t(2026, 10, 1, 9, 0), 20).unwrap();
    b.unlock("done_10", &t(2026, 10, 5, 9, 0), 30).unwrap();
    both_ways(&a, &b);
    let ma = a.unlocked_map().unwrap();
    assert_eq!(ma.len(), 2);
    assert_eq!(ma["first_step"], "2026-10-01T09:00:00");
    assert_eq!(b.unlocked_map().unwrap()["first_step"], "2026-10-01T09:00:00");
}

#[test]
fn file_export_import_plain_and_encrypted() {
    let a = dev(10_000);
    let b = dev(20_000);
    a.save_note(&note("в файле"), t(2026, 10, 1, 9, 0)).unwrap();

    // без группы файл обычный
    let plain = a.sync_export_text().unwrap();
    assert!(plain.starts_with('{'));
    assert_eq!(b.sync_import_text(&plain).unwrap().rejected, 0);
    assert_eq!(b.list_notes(None, None).unwrap().len(), 1);

    // с группой файл зашифрован, прочитать его можно только с тем же кодом
    let code = a.sync_create_group().unwrap();
    let enc = a.sync_export_text().unwrap();
    assert!(enc.starts_with("DPSYNC1:"));
    assert!(!enc.contains("в файле"));
    let c = dev(30_000);
    assert!(c.sync_import_text(&enc).is_err(), "без кода нельзя");
    c.sync_join_group(&generate_code_for_tests()).unwrap();
    assert!(c.sync_import_text(&enc).is_err(), "чужой код");
    c.sync_join_group(&code).unwrap();
    c.sync_import_text(&enc).unwrap();
    assert_eq!(c.list_notes(None, None).unwrap().len(), 1);

    assert!(b.sync_import_text("это не json").is_err());
    assert!(b.sync_import_text("{\"v\":99,\"device\":\"x\",\"msg\":\"data\"}").is_err());
}

fn generate_code_for_tests() -> String {
    planner_core::sync::generate_code()
}

#[test]
fn corrupted_records_are_rejected_without_breaking_the_rest() {
    use planner_core::sync::Record;
    let b = dev(10_000);
    let good = Record { kind: "category".into(), uid: "u-good".into(), modified: 5, deleted: false, data: json!({"name": "Хорошая", "color": "#000000", "icon": "⭐"}) };
    let bad_kind = Record { kind: "wat".into(), uid: "u-1".into(), modified: 5, deleted: false, data: json!({}) };
    let bad_data = Record { kind: "reminder".into(), uid: "u-2".into(), modified: 5, deleted: false, data: json!("строка") };
    let empty_name = Record { kind: "category".into(), uid: "u-3".into(), modified: 5, deleted: false, data: json!({"name": " ", "color": "#000000", "icon": "x"}) };
    let stats = b.merge(&[bad_kind, bad_data, good, empty_name], &[]).unwrap();
    assert_eq!(stats.applied, 1);
    assert_eq!(stats.rejected, 3);
    assert!(b.list_categories().unwrap().iter().any(|c| c.name == "Хорошая"));
}

#[test]
fn sync_commands_through_api() {
    let store = Mutex::new(Store::open_in_memory().unwrap());
    let now = t(2026, 10, 29, 9, 0);
    let st = dispatch(&store, now, "sync.status", json!({})).unwrap();
    assert_eq!(st["enabled"], false);
    let created = dispatch(&store, now, "sync.create", json!({})).unwrap();
    let code = created["code"].as_str().unwrap().to_string();
    let st = dispatch(&store, now, "sync.status", json!({})).unwrap();
    assert_eq!(st["enabled"], true);
    assert_eq!(st["code"], code.as_str());
    assert_eq!(st["ntfy"], true);
    dispatch(&store, now, "sync.config", json!({"ntfy": false, "lan": true, "server": "https://ntfy.example.org/"})).unwrap();
    let st = dispatch(&store, now, "sync.status", json!({})).unwrap();
    assert_eq!(st["ntfy"], false);
    assert_eq!(st["server"], "https://ntfy.example.org");
    assert!(dispatch(&store, now, "sync.config", json!({"ntfy": true, "lan": true, "server": "ftp://x"})).is_err());
    assert!(dispatch(&store, now, "sync.join", json!({"code": "плохой"})).is_err());
    dispatch(&store, now, "notes.save", json!({"note": {"title": "x", "body": "y"}})).unwrap();
    let exp = dispatch(&store, now, "sync.export", json!({})).unwrap();
    let text = exp["text"].as_str().unwrap().to_string();
    let other = Mutex::new(Store::open_in_memory().unwrap());
    dispatch(&other, now, "sync.join", json!({"code": code})).unwrap();
    let stats = dispatch(&other, now, "sync.import", json!({"text": text})).unwrap();
    assert_eq!(stats["rejected"], 0);
    assert_eq!(dispatch(&other, now, "notes.list", json!({})).unwrap().as_array().unwrap().len(), 1);
    dispatch(&store, now, "sync.leave", json!({})).unwrap();
    assert_eq!(dispatch(&store, now, "sync.status", json!({})).unwrap()["enabled"], false);
}
