// Одна команда `native`: она пересылает вызов в Kotlin-класс PlannerPlugin.
const COMMANDS: &[&str] = &["native"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
