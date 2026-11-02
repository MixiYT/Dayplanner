// Одна команда `native`. В версии для ПК Android-части нет.
const COMMANDS: &[&str] = &["native"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
