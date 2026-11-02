// Не показывать консоль в Windows-версии.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dayplanner_lib::run()
}
