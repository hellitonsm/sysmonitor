#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]

mod full;
mod lite;

fn main() -> eframe::Result<()> {
    if std::env::args().any(|a| a == "--lite") {
        lite::run()
    } else {
        full::run()
    }
}
