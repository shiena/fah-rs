#![windows_subsystem = "windows"]
mod cli;
use fah_gui as gui;
use fah_windows::{inference, platform};
fn main() {
    let result = cli::run();
    // cli::run has dropped all clients and sessions before process-wide teardown.
    unsafe {
        platform::shutdown();
    }
    if let Err(error) = result {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
