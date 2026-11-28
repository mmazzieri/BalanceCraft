//! BalanceCraft interface (egui): Today, the Tree, settings and the file menu.
#![forbid(unsafe_code)]

#[cfg(test)]
mod a11y_tests;
mod app;
mod dialogs;
mod focus;
mod forms;
mod i18n;
mod theme;
mod views;

pub use app::BalanceCraftApp;
