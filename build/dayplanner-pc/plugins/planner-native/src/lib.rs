//! Системные функции ПК (уведомления Windows, трей, автозапуск).
//! Сейчас это заглушка: интерфейс получает `null` и скрывает настройки Android.
//! Сюда добавляется всё, что зависит от ОС.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

mod commands;
mod desktop;

use desktop::PlannerNative;

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("planner-native")
        .invoke_handler(tauri::generate_handler![commands::native])
        .setup(|app, api| {
            let native = desktop::init(app, api)?;
            app.manage(native);
            Ok(())
        })
        .build()
}
