use std::io::Write;
use base64::Engine;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tauri::{plugin::PluginApi, AppHandle, Manager, Runtime};

pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Result<PlannerNative<R>, String> {
    Ok(PlannerNative(app.clone()))
}

pub struct PlannerNative<R: Runtime>(AppHandle<R>);

impl<R: Runtime> PlannerNative<R> {
    /// На ПК системных функций пока нет: интерфейс получает `null` и скрывает Android-настройки.
    pub fn call(&self, cmd: &str, args: Value) -> Result<Value, String> {
        match cmd {
            "open_url" => {
                let url=args["url"].as_str().unwrap_or("");
                if !url.starts_with("https://www.dropbox.com/oauth2/authorize?") || url.chars().any(char::is_whitespace) { return Err("Недопустимый адрес".into()); }
                #[cfg(windows)] let result=std::process::Command::new("rundll32.exe").arg("url.dll,FileProtocolHandler").arg(url).spawn();
                #[cfg(target_os="macos")] let result=std::process::Command::new("open").arg(url).spawn();
                #[cfg(all(not(windows),not(target_os="macos")))] let result=std::process::Command::new("xdg-open").arg(url).spawn();
                result.map_err(|_| "Не удалось открыть браузер".to_string())?;
                Ok(serde_json::json!({"opened":true}))
            },
            "save_file" => {
                let name=args["name"].as_str().unwrap_or("");
                if name.is_empty() || name.len()>240 || name.chars().any(|c| c.is_control() || "/\\:*?\"<>|".contains(c)) { return Err("Недопустимое имя файла".into()); }
                let data=args["data"].as_str().unwrap_or("");
                if data.len()>56*1024*1024 { return Err("Файл больше 40 МБ".into()); }
                let bytes=base64::engine::general_purpose::STANDARD.decode(data).map_err(|_| "Повреждённый файл".to_string())?;
                let dir=self.0.path().download_dir().map_err(|_| "Не найдена папка загрузок".to_string())?;
                std::fs::create_dir_all(&dir).map_err(|_| "Не удалось открыть загрузки".to_string())?;
                for index in 0..1000 {
                    let path=dir.join(if index==0 { name.into() } else { let p=std::path::Path::new(name); format!("{} ({index}).{}",p.file_stem().and_then(|s|s.to_str()).unwrap_or("file"),p.extension().and_then(|s|s.to_str()).unwrap_or("")) });
                    match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                        Ok(mut file) => { file.write_all(&bytes).map_err(|_| "Не удалось сохранить файл".to_string())?; return Ok(serde_json::json!({"saved":true,"path":path.to_string_lossy()})); },
                        Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists => continue,
                        Err(_) => return Err("Нет доступа к папке загрузок".into()),
                    }
                }
                Err("Слишком много файлов с таким именем".into())
            },
            _ => Ok(Value::Null),
        }
    }
}
