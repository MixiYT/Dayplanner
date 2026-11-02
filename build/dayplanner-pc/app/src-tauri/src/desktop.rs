//! Напоминания на ПК.
//!
//! Фоновый поток раз в 3 секунды спрашивает у ядра, какие дела уже наступили и ещё не отмечены
//! (`Store::pending`), и открывает для них всплывающее окно `alert.html` в одном из трёх видов:
//! уведомление (маленькое, в углу), будильник (по центру, со звуком), на весь экран.
//! Кнопки окна («Выполнено», «Пропустить», «Через 10 минут») вызывают команду `alert_action`,
//! которая сразу применяет действие к ядру с настоящим временем нажатия.
//!
//! Интерфейсу ничего передавать не нужно: поток работает сам, даже когда окно приложения
//! свёрнуто в трей. Звук играет Rust (системный сигнал Windows), а не веб-страница: так он
//! не зависит от правил автовоспроизведения в WebView2.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{Duration as ChronoDuration, Local, NaiveDateTime};
use planner_core::model::DayItem;
use planner_core::{api as core_api, timefmt::dt, Store};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::AppState;

/// Насколько позже срока ещё имеет смысл показывать напоминание, если приложение работало
/// без перерыва (при выходе из сна показываем всё, что наступило за время сна, независимо от возраста).
const CATCH_UP_MINUTES: i64 = 15;
/// Через сколько минут повторить напоминание после «Через 10 минут».
const SNOOZE_MINUTES: i64 = 10;
/// Сколько окон открывать за один проход: после долгого сна они появятся по очереди, а не все сразу.
const MAX_OPEN_PER_TICK: usize = 3;
/// Будильник звучит не дольше этого времени.
const ALARM_MAX_SECONDS: u64 = 300;

#[cfg(windows)]
mod beep {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBeep(utype: u32) -> i32;
    }

    /// Системный сигнал «внимание» (MB_ICONEXCLAMATION).
    pub fn play() {
        // SAFETY: MessageBeep не принимает указателей и безопасна при любых значениях.
        unsafe {
            MessageBeep(0x0000_0030);
        }
    }
}

#[cfg(not(windows))]
mod beep {
    pub fn play() {}
}

/// Состояние всплывающих окон.
#[derive(Default)]
pub struct Alerts {
    /// Данные окна по его метке (`alert-1`, `alert-2`, …).
    payloads: Mutex<HashMap<String, Value>>,
    /// Отложенные напоминания: ключ срабатывания → когда показать снова.
    snoozed: Mutex<HashMap<String, NaiveDateTime>>,
    /// Окна, для которых сейчас звучит сигнал.
    ringing: Mutex<HashSet<String>>,
    counter: Mutex<u64>,
}

fn item_key(item: &DayItem) -> String {
    format!("{}|{}", item.reminder_id, dt::format(&item.at))
}

/// Акцентный цвет, тема и режим из настроек: окну нужны они же, что и основному интерфейсу.
fn appearance(store: &Arc<Mutex<Store>>) -> (String, String, String) {
    let s = store.lock().unwrap_or_else(|e| e.into_inner());
    (
        s.setting_or("accent", "#6750A4").unwrap_or_else(|_| "#6750A4".into()),
        s.setting_or("theme", "material").unwrap_or_else(|_| "material".into()),
        s.setting_or("mode", "auto").unwrap_or_else(|_| "auto".into()),
    )
}

/// Запускает фоновый планировщик.
pub fn start(app: AppHandle, store: Arc<Mutex<Store>>) {
    std::thread::spawn(move || {
        let mut fired: HashSet<String> = HashSet::new();
        // Момент прошлого прохода: всё, что наступило с тех пор, показываем, даже если прошло много времени
        // (компьютер мог спать).
        let mut last_tick = Local::now().naive_local();
        loop {
            std::thread::sleep(Duration::from_secs(3));
            let now = Local::now().naive_local();

            let pending = {
                let s = store.lock().unwrap_or_else(|e| e.into_inner());
                s.pending(now)
            };
            let mut items = match pending {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("не удалось получить напоминания: {e}");
                    last_tick = now;
                    continue;
                }
            };
            // Показываем от старых к новым.
            items.sort_by_key(|i| i.at);

            let alerts = app.state::<Alerts>();
            // Выполненные или удалённые дела больше не нужно держать в отложенных.
            {
                let keys: HashSet<String> = items.iter().map(item_key).collect();
                alerts
                    .snoozed
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .retain(|k, _| keys.contains(k));
            }
            if fired.len() > 5000 {
                fired.clear();
            }

            let mut opened = 0usize;
            for item in items {
                if opened >= MAX_OPEN_PER_TICK {
                    break;
                }
                let key = item_key(&item);

                // Отложенное напоминание ждёт своего часа.
                let snooze_due = {
                    let mut snoozed = alerts.snoozed.lock().unwrap_or_else(|e| e.into_inner());
                    match snoozed.get(&key).copied() {
                        Some(until) if now >= until => {
                            snoozed.remove(&key);
                            true
                        }
                        Some(_) => continue,
                        None => false,
                    }
                };

                // Статус «не выполнено» тоже годится: при допуске «точно в срок» окно закрывается
                // через секунду, но показать напоминание всё равно нужно.
                let due_since_last_tick = item.at > last_tick;
                let recent = now - item.at <= ChronoDuration::minutes(CATCH_UP_MINUTES);
                let fresh = !fired.contains(&key) && (due_since_last_tick || recent);

                if snooze_due || fresh {
                    let (accent, theme, mode) = appearance(&store);
                    let payload = json!({
                        "key": key,
                        "reminder_id": item.reminder_id,
                        "at": dt::format(&item.at),
                        "title": item.title,
                        "notes": item.notes,
                        "icon": item.icon,
                        "alert": item.alert.as_str(),
                        "accent": accent,
                        "theme": theme,
                        "mode": mode,
                        "test": false,
                    });
                    match show_alert(&app, payload) {
                        // Запоминаем только удачный показ: при ошибке попробуем ещё раз через 3 секунды.
                        Ok(()) => {
                            fired.insert(key);
                            opened += 1;
                        }
                        Err(e) => eprintln!("не удалось показать напоминание: {e}"),
                    }
                }
            }
            last_tick = now;
        }
    });
}

/// Положение маленького уведомления: правый нижний угол основного монитора, окна друг над другом.
fn corner(app: &AppHandle, w: f64, h: f64, index: f64) -> (f64, f64) {
    let monitor = app
        .get_webview_window("main")
        .and_then(|win| win.primary_monitor().ok().flatten());
    if let Some(m) = monitor {
        let scale = m.scale_factor();
        let size = m.size();
        let pos = m.position();
        let mw = size.width as f64 / scale;
        let mh = size.height as f64 / scale;
        let ox = pos.x as f64 / scale;
        let oy = pos.y as f64 / scale;
        // 60 пикселей снизу оставляем под панель задач.
        (ox + mw - w - 16.0, oy + mh - h - 60.0 - index * (h + 10.0))
    } else {
        (100.0 + index * 20.0, 100.0 + index * 20.0)
    }
}

/// Звук: будильник звучит, пока окно не закроют или не нажмут «Выключить звук»;
/// остальные виды дают один сигнал.
fn start_sound(app: &AppHandle, label: &str, repeat: bool) {
    if repeat {
        app.state::<Alerts>()
            .ringing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(label.to_string());
    }
    let app = app.clone();
    let label = label.to_string();
    std::thread::spawn(move || {
        let started = Instant::now();
        loop {
            beep::play();
            if !repeat {
                return;
            }
            std::thread::sleep(Duration::from_millis(1500));
            let on = app
                .state::<Alerts>()
                .ringing
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(&label);
            if !on || started.elapsed() > Duration::from_secs(ALARM_MAX_SECONDS) {
                break;
            }
        }
        app.state::<Alerts>()
            .ringing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&label);
    });
}

fn stop_sound(app: &AppHandle, label: &str) {
    app.state::<Alerts>()
        .ringing
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(label);
}

/// Окно закрыто (кнопкой, Alt+F4 или иначе): забываем его данные и глушим звук.
pub fn forget(app: &AppHandle, label: &str) {
    stop_sound(app, label);
    app.state::<Alerts>()
        .payloads
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(label);
}

/// Открывает всплывающее окно с напоминанием.
pub fn show_alert(app: &AppHandle, payload: Value) -> Result<(), String> {
    let alerts = app.state::<Alerts>();
    let n = {
        let mut c = alerts.counter.lock().unwrap_or_else(|e| e.into_inner());
        *c += 1;
        *c
    };
    let label = format!("alert-{n}");
    alerts
        .payloads
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(label.clone(), payload.clone());

    let kind = payload["alert"].as_str().unwrap_or("notification").to_string();
    let open_now = app
        .webview_windows()
        .keys()
        .filter(|l| l.starts_with("alert-"))
        .count() as f64;

    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("alert.html".into()))
        .title("Напоминание")
        .decorations(false)
        .always_on_top(true)
        .resizable(false);

    let builder = match kind.as_str() {
        "alarm" => builder.inner_size(480.0, 560.0).center().focused(true),
        "full_screen" => builder.fullscreen(true).focused(true),
        _ => {
            let (w, h) = (380.0, 176.0);
            let (x, y) = corner(app, w, h, open_now);
            // Маленькое уведомление не отбирает фокус и не занимает место на панели задач.
            builder.inner_size(w, h).position(x, y).focused(false).skip_taskbar(true)
        }
    };

    if let Err(e) = builder.build() {
        forget(app, &label);
        return Err(e.to_string());
    }
    start_sound(app, &label, kind == "alarm");
    Ok(())
}

/// Данные окна: окно спрашивает их при открытии.
#[tauri::command]
pub async fn alert_info(alerts: State<'_, Alerts>, window: WebviewWindow) -> Result<Option<Value>, String> {
    let map = alerts.payloads.lock().unwrap_or_else(|e| e.into_inner());
    Ok(map.get(window.label()).cloned())
}

/// «Выключить звук» в окне будильника.
#[tauri::command]
pub async fn alert_mute(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    stop_sound(&app, window.label());
    Ok(())
}

/// Кнопка в окне: `done`, `skip`, `snooze` или `close` (просто закрыть).
/// При ошибке окно остаётся открытым, а текст ошибки показывается в нём.
#[tauri::command]
pub async fn alert_action(
    app: AppHandle,
    alerts: State<'_, Alerts>,
    state: State<'_, AppState>,
    window: WebviewWindow,
    action: String,
) -> Result<(), String> {
    let label = window.label().to_string();
    let payload = alerts
        .payloads
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&label)
        .cloned();

    if let Some(p) = payload {
        // Окно «проверить» не трогает данные.
        if !p["test"].as_bool().unwrap_or(false) {
            let now = Local::now().naive_local();
            let args = json!({
                "reminder_id": p["reminder_id"].as_i64().unwrap_or(0),
                "at": p["at"].as_str().unwrap_or(""),
            });
            match action.as_str() {
                "done" | "skip" => {
                    let cmd = if action == "done" { "complete" } else { "skip" };
                    if let Err(e) = core_api::dispatch(&state.store, now, cmd, args) {
                        let text = e.to_string();
                        // «Уже выполнено» (например, отметили в главном окне) ошибкой не считаем.
                        if !text.contains("Уже выполнено") {
                            return Err(text);
                        }
                    }
                }
                "snooze" => {
                    if let Some(key) = p["key"].as_str() {
                        alerts
                            .snoozed
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(key.to_string(), now + ChronoDuration::minutes(SNOOZE_MINUTES));
                    }
                }
                _ => {}
            }
        }
    }

    forget(&app, &label);
    let _ = window.close();
    // Основное окно перерисует список.
    if action == "done" || action == "skip" {
        let _ = app.emit("planner-changed", ());
    }
    Ok(())
}

/// Кнопка «Проверить» в настройках: показывает пример напоминания выбранного вида.
#[tauri::command]
pub async fn desktop_test(app: AppHandle, state: State<'_, AppState>, kind: String) -> Result<(), String> {
    let kind = match kind.as_str() {
        "alarm" | "full_screen" => kind,
        _ => "notification".to_string(),
    };
    let (accent, theme, mode) = appearance(&state.store);
    let now = Local::now().naive_local();
    show_alert(
        &app,
        json!({
            "key": "test",
            "reminder_id": 0,
            "at": dt::format(&now),
            "title": "Пример напоминания",
            "notes": "Так будет выглядеть ваше напоминание",
            "icon": "🔔",
            "alert": kind,
            "accent": accent,
            "theme": theme,
            "mode": mode,
            "test": true,
        }),
    )
}

/// Состояние настроек ПК для карточки «Компьютер».
#[tauri::command]
pub async fn desktop_status(app: AppHandle) -> Result<Value, String> {
    use tauri_plugin_autostart::ManagerExt;
    let autostart = app.autolaunch().is_enabled().unwrap_or(false);
    Ok(json!({ "autostart": autostart, "tray": true }))
}

#[tauri::command]
pub async fn desktop_set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable().map_err(|e| e.to_string())
    } else {
        launcher.disable().map_err(|e| e.to_string())
    }
}
