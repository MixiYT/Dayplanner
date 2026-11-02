//! Плагин для системных функций. На Android вызовы уходят в Kotlin (папка `android`),
//! на ПК пока ничего не делают: уведомления для Windows добавим следующим этапом.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

mod commands;
#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(target_os = "android")]
mod mobile;

#[cfg(not(target_os = "android"))]
use desktop::PlannerNative;
#[cfg(target_os = "android")]
use mobile::PlannerNative;

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("planner-native")
        .invoke_handler(tauri::generate_handler![commands::native])
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let native = mobile::init(app, api)?;
            #[cfg(not(target_os = "android"))]
            let native = desktop::init(app, api)?;
            app.manage(native);
            Ok(())
        })
        .build()
}
