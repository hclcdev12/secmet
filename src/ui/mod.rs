//! Module pour l'interface graphique GTK
//!
//! Découpe du monolithe historique `ui.rs` en sous-modules.

mod app;
mod app_theme;
mod auth;
mod clock_ui;
mod dialogs;
mod disk_list;
mod enrichment;
mod erase;
mod lot_ui;
mod pdf_ui;
mod settings_ui;
mod testdisk_ui;
mod types;
mod update_ui;
mod verify;

pub use app::SecMetApp;
