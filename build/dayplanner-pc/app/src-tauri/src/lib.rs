use std::sync::{Arc, Mutex};

use chrono::{Local, NaiveDateTime};
use planner_core::{api as core_api, timefmt::dt, Store};
use serde_json::Value;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

mod desktop;

pub struct AppState {
    pub store: Arc<Mutex<Store>>,
}

/// Главная команда интерфейса: `api(cmd, args, now)`. Интерфейс всегда присылает `now`
/// (локальное время устройства); если не прислал, берём время системы.
#[tauri::command]
async fn api(
    state: State<'_, AppState>,
    cmd: String,
    args: Option<Value>,
    now: Option<String>,
) -> Result<Value, String> {
    let store = state.store.clone();
    let now: NaiveDateTime = match now {
        Some(s) => dt::parse(&s)?,
        None => Local::now().naive_local(),
    };
    // Сетевые команды блокирующие, поэтому выносим всё из асинхронного потока.
    tauri::async_runtime::spawn_blocking(move || {
        core_api::dispatch(&store, now, &cmd, args.unwrap_or(Value::Null)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("внутренняя ошибка: {e}"))?
}

/// Показать главное окно (из трея или при повторном запуске приложения).
fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn run() {
    tauri::Builder::default()
        // Второй запуск не открывает вторую копию, а показывает уже работающую.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .plugin(tauri_plugin_planner_native::init())
        .manage(desktop::Alerts::default())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = Store::open(dir.join("diary.sqlite3")).map_err(|e| e.to_string())?;
            let store = Arc::new(Mutex::new(store));
            app.manage(AppState { store: store.clone() });

            // Значок в трее: приложение остаётся работать, чтобы срабатывали напоминания.
            let show = MenuItem::with_id(app, "show", "Открыть ежедневник", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Выйти", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("Ежедневник")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            // Окно в настройках создаётся скрытым, чтобы при запуске вместе с Windows не мигало:
            // при обычном запуске показываем его сами, при запуске с `--minimized` оставляем в трее.
            if !std::env::args().any(|a| a == "--minimized") {
                show_main(app.handle());
            }

            // Синхронизация с другими устройствами (ntfy и локальная сеть); работает, пока приложение в трее.
            let cloud_handle = app.handle().clone();
            planner_core::dropbox::start(store.clone(), Arc::new(move || { let _ = cloud_handle.emit("planner-changed", ()); }));
            let handle = app.handle().clone();
            planner_core::sync_net::start(
                store.clone(),
                Arc::new(move || {
                    let _ = handle.emit("planner-changed", ());
                }),
            );

            desktop::start(app.handle().clone(), store);
            Ok(())
        })
        // Закрытие главного окна сворачивает приложение в трей.
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                api.prevent_close();
                let _ = window.hide();
            }
            // Окно напоминания закрыто любым способом (в том числе Alt+F4): убираем его данные и звук.
            WindowEvent::Destroyed if window.label().starts_with("alert-") => {
                desktop::forget(window.app_handle(), window.label());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            api,
            desktop::alert_info,
            desktop::alert_action,
            desktop::alert_mute,
            desktop::desktop_test,
            desktop::desktop_status,
            desktop::desktop_set_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("не удалось запустить приложение");
}
