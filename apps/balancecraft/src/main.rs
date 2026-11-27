//! BalanceCraft desktop app.
#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use balancecraft_ui_egui::BalanceCraftApp;

/// Fixed install identity (docs/plan.md): also the Wayland app id.
const APP_ID: &str = "it.mauromazzieri.balancecraft";

/// Window icon, 64x64 RGBA, written by tools/icons.py.
const ICON: &[u8] = include_bytes!("../../../assets/app-icon/window-64.rgba");

fn main() -> eframe::Result {
    let open = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("BalanceCraft")
            .with_app_id(APP_ID)
            .with_icon(egui::IconData { rgba: ICON.to_vec(), width: 64, height: 64 })
            .with_inner_size([960.0, 680.0])
            .with_min_inner_size([560.0, 420.0]),
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native("BalanceCraft", options, Box::new(|cc| Ok(Box::new(BalanceCraftApp::new(cc, open)))))
}
