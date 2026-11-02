use serde::de::DeserializeOwned;
use serde_json::Value;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

const PLUGIN_IDENTIFIER: &str = "app.dayplanner.nativebridge";

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> Result<PlannerNative<R>, String> {
    let handle = api
        .register_android_plugin(PLUGIN_IDENTIFIER, "PlannerPlugin")
        .map_err(|e| e.to_string())?;
    Ok(PlannerNative(handle))
}

pub struct PlannerNative<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> PlannerNative<R> {
    pub fn call(&self, cmd: &str, args: Value) -> Result<Value, String> {
        self.0.run_mobile_plugin::<Value>(cmd, args).map_err(|e| e.to_string())
    }
}
