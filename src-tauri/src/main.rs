// Prevents an additional console window on Windows in release, improving startup time.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    wbbridge_lib::run()
}
