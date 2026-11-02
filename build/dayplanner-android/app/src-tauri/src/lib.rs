use std::sync::{Arc, Mutex};

use chrono::{Local, NaiveDateTime};
use planner_core::{api as core_api, timefmt::dt, Store};
use serde_json::Value;
use tauri::{Emitter, Manager, State};

struct AppState {
    store: Arc<Mutex<Store>>,
}

/// Единственная команда интерфейса: `api(cmd, args, now)`.
/// `now` нужен, когда отметка сделана раньше (например, нажата в уведомлении
/// при закрытом приложении), и должна быть записана настоящим временем нажатия.
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_planner_native::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = Store::open(dir.join("diary.sqlite3")).map_err(|e| e.to_string())?;
            let store = Arc::new(Mutex::new(store));
            app.manage(AppState { store: store.clone() });

            // Синхронизация с другими устройствами работает, пока приложение открыто.
            let cloud_handle = app.handle().clone();
            planner_core::dropbox::start(store.clone(), Arc::new(move || { let _ = cloud_handle.emit("planner-changed", ()); }));
            let handle = app.handle().clone();
            planner_core::sync_net::start(
                store,
                Arc::new(move || {
                    let _ = handle.emit("planner-changed", ());
                }),
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![api])
        .run(tauri::generate_context!())
        .expect("не удалось запустить приложение");
}
