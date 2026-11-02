use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use serde_json::Value;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Result<PlannerNative<R>, String> {
    Ok(PlannerNative(PhantomData))
}

pub struct PlannerNative<R: Runtime>(PhantomData<fn() -> R>);

impl<R: Runtime> PlannerNative<R> {
    /// На ПК системных функций пока нет: интерфейс получает `null` и скрывает Android-настройки.
    pub fn call(&self, _cmd: &str, _args: Value) -> Result<Value, String> {
        Ok(Value::Null)
    }
}
