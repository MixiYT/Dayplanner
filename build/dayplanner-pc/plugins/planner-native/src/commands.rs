use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Runtime};

use crate::PlannerNative;

/// `native(cmd, args)` → Kotlin-команда `cmd` с аргументами `args`.
#[tauri::command]
pub(crate) async fn native<R: Runtime>(
    app: AppHandle<R>,
    cmd: String,
    args: Option<Value>,
) -> Result<Value, String> {
    app.state::<PlannerNative<R>>().call(&cmd, args.unwrap_or_else(|| json!({})))
}
