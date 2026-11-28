//! The application: open document, sync loop, menus and status bar.

use crate::dialogs::{self, Dialog};
use crate::i18n::{Lang, Strings};
use crate::views;
use balancecraft_engine::commands::{Command, Ctx, NewArea, apply};
use balancecraft_engine::defaults::DEFAULT_AREAS;
use balancecraft_engine::tree::{MAX_DEPTH, find_task, fits_under, height};
use balancecraft_file::{EXTENSION, Now, Store, StoreError};
use balancecraft_model::AppData;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

const PREFS_KEY: &str = "balancecraft";
const RECENT_MAX: usize = 8;
/// After an edit, the shared file is written this long after the last change.
const SAVE_DELAY: f64 = 1.0;
/// How often the shared file is checked for changes made elsewhere.
const POLL_EVERY: f64 = 3.0;
/// How long the undo bar of a completion stays.
const TOAST_SECS: f64 = 8.0;

pub fn generator() -> String {
    format!("BalanceCraft {}", env!("CARGO_PKG_VERSION"))
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Per-computer preferences, saved by eframe.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub lang: Option<Lang>,
    pub recent: Vec<PathBuf>,
    /// The document open at the last exit, reopened at start.
    pub last: Option<PathBuf>,
    /// Light or dark colours, or those of the system (the default).
    pub theme: Theme,
}

/// The colours of the window: those of the system, or always light or dark.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub fn preference(self) -> egui::ThemePreference {
        match self {
            Theme::System => egui::ThemePreference::System,
            Theme::Light => egui::ThemePreference::Light,
            Theme::Dark => egui::ThemePreference::Dark,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Today,
    Tree,
    Areas,
    Waiting,
    History,
    Settings,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Saved,
    Pending,
    Merged,
    Error(String),
}

/// The bar that lets a completion be undone.
#[derive(Debug, Clone)]
pub struct Toast {
    pub task_id: String,
    pub name: String,
    pub until: f64,
}

/// What a view asks for while drawing; applied after the frame is drawn.
#[derive(Debug, Clone)]
pub enum Action {
    Run(Command),
    Open(Dialog),
    Completed { task_id: String, name: String },
    Undo(String),
    SetLang(Lang),
    SetTheme(Theme),
}

/// Interface state that does not belong to the document.
pub struct UiState {
    pub lang: Lang,
    pub theme: Theme,
    pub view: View,
    pub area_filter: Option<String>,
    pub dialog: Option<Dialog>,
    pub toast: Option<Toast>,
    pub actions: Vec<Action>,
    /// Text of the global warning field in Settings.
    pub global_advance: Option<String>,
}

impl UiState {
    pub fn s(&self) -> &'static Strings {
        self.lang.strings()
    }
}

pub struct BalanceCraftApp {
    prefs: Prefs,
    store: Option<Store>,
    ui: UiState,
    status: Status,
    manual_copies: Vec<PathBuf>,
    /// Time of the last edit not yet written to the shared file.
    dirty_since: Option<f64>,
    next_poll: f64,
}

/// The per-computer data folder, outside the synced folder: the replica and the
/// backups. Windows: %LOCALAPPDATA%; macOS: ~/Library/Application Support;
/// elsewhere $XDG_DATA_HOME or ~/.local/share.
fn replica_dir() -> Result<PathBuf, String> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local").join("share")))
    };
    base.map(|d| d.join("BalanceCraft")).ok_or_else(|| "no local data folder".to_owned())
}

fn err_text(e: &StoreError) -> String {
    e.to_string()
}

/// True when `cmd` adds or moves a task past the deepest level.
fn too_deep(data: &AppData, cmd: &Command) -> bool {
    match cmd {
        Command::AddTask { parent_id, .. } => !fits_under(&data.areas, parent_id, 1),
        Command::MoveTask { id, new_parent_id, .. } => {
            find_task(&data.areas, id).is_some_and(|(n, _)| !fits_under(&data.areas, new_parent_id, height(n)))
        }
        _ => false,
    }
}

fn with_extension(p: PathBuf) -> PathBuf {
    if p.extension().is_some_and(|e| e == EXTENSION) { p } else { p.with_extension(EXTENSION) }
}

impl BalanceCraftApp {
    /// `open`: a document given on the command line (or by the file
    /// association); otherwise the one open at the last exit.
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        let prefs: Prefs = cc.storage.and_then(|s| eframe::get_value(s, PREFS_KEY)).unwrap_or_default();
        let lang = prefs.lang.unwrap_or_else(Lang::detect);
        let theme = prefs.theme;
        cc.egui_ctx.set_theme(theme.preference());
        crate::theme::install(&cc.egui_ctx);
        let mut app = BalanceCraftApp {
            prefs,
            store: None,
            ui: UiState {
                lang,
                theme,
                view: View::Today,
                area_filter: None,
                dialog: None,
                toast: None,
                actions: Vec::new(),
                global_advance: None,
            },
            status: Status::Saved,
            manual_copies: Vec::new(),
            dirty_since: None,
            next_poll: 0.0,
        };
        if let Some(path) = open.or_else(|| app.prefs.last.clone().filter(|p| p.exists())) {
            app.open(&path);
        }
        app
    }

    fn remember(&mut self, path: &Path) {
        self.prefs.recent.retain(|p| p != path);
        self.prefs.recent.insert(0, path.to_owned());
        self.prefs.recent.truncate(RECENT_MAX);
        self.prefs.last = Some(path.to_owned());
    }

    fn set_store(&mut self, store: Store, path: &Path) {
        self.close();
        self.store = Some(store);
        self.remember(path);
        self.status = Status::Saved;
        self.ui.view = View::Today;
        self.ui.area_filter = None;
        self.ui.global_advance = None;
    }

    fn open(&mut self, path: &Path) {
        let result = replica_dir()
            .and_then(|dir| Store::open(path, &dir, &generator(), Now(now_ms())).map_err(|e| err_text(&e)));
        match result {
            Ok((store, report)) => {
                // At opening the file is read for the first time: nothing to
                // report as merged.
                self.manual_copies = report.manual_copies;
                self.set_store(store, path);
            }
            Err(e) => self.status = Status::Error(e),
        }
    }

    fn new_document(&mut self, path: PathBuf) {
        let path = with_extension(path);
        let s = self.ui.s();
        let mut data = AppData::default();
        for ((color, weight), name) in DEFAULT_AREAS.iter().zip(s.default_areas) {
            let cmd = Command::AddArea {
                id: new_id(),
                area: NewArea {
                    name: name.to_owned(),
                    color: Some((*color).to_owned()),
                    weight: Some(*weight),
                    advance_days: None,
                },
            };
            if let Some(next) = apply(&data, &cmd, &Ctx::new(&data, now_ms())) {
                data = next;
            }
        }
        let result = replica_dir()
            .and_then(|dir| Store::create(&path, &dir, &generator(), data, Now(now_ms())).map_err(|e| err_text(&e)));
        match result {
            Ok(store) => self.set_store(store, &path),
            Err(e) => self.status = Status::Error(e),
        }
    }

    fn import_cerqua(&mut self, backup: &Path, target: PathBuf) {
        let target = with_extension(target);
        let result = replica_dir().and_then(|dir| {
            Store::import_cerqua_backup(backup, &target, &dir, &generator(), Now(now_ms())).map_err(|e| err_text(&e))
        });
        match result {
            Ok(store) => self.set_store(store, &target),
            Err(e) => self.status = Status::Error(e),
        }
    }

    /// Writes pending edits and lets the document go.
    fn close(&mut self) {
        if self.dirty_since.is_some() {
            self.sync();
        }
        self.store = None;
        self.prefs.last = None;
        self.manual_copies.clear();
        self.ui.dialog = None;
        self.ui.toast = None;
    }

    fn run(&mut self, cmd: &Command, time: f64) {
        let Some(store) = self.store.as_mut() else { return };
        let ctx = Ctx::new(store.data(), now_ms());
        let Some(next) = apply(store.data(), cmd, &ctx) else {
            if too_deep(store.data(), cmd) {
                let s = self.ui.s();
                self.status = Status::Error(s.too_deep.replace("{}", &MAX_DEPTH.to_string()));
            }
            return;
        };
        match store.set_data(next) {
            Ok(()) => {
                self.dirty_since = Some(time);
                self.status = Status::Pending;
            }
            Err(e) => self.status = Status::Error(err_text(&e)),
        }
    }

    fn sync(&mut self) {
        let Some(store) = self.store.as_mut() else { return };
        match store.sync(Now(now_ms())) {
            Ok(report) => {
                self.dirty_since = None;
                self.manual_copies = report.manual_copies;
                self.status = if report.merged_from_disk { Status::Merged } else { Status::Saved };
            }
            Err(e) => self.status = Status::Error(err_text(&e)),
        }
    }

    fn tick(&mut self, time: f64) {
        if self.store.is_none() {
            return;
        }
        let due = self.dirty_since.is_some_and(|t| time - t >= SAVE_DELAY);
        let poll = time >= self.next_poll;
        if poll {
            self.next_poll = time + POLL_EVERY;
        }
        if due || (poll && self.store.as_ref().is_some_and(Store::disk_changed)) {
            self.sync();
        }
        if self.ui.toast.as_ref().is_some_and(|t| time > t.until) {
            self.ui.toast = None;
        }
    }

    fn apply_actions(&mut self, ctx: &egui::Context, time: f64) {
        for action in std::mem::take(&mut self.ui.actions) {
            match action {
                Action::Run(cmd) => self.run(&cmd, time),
                Action::Open(d) => self.ui.dialog = Some(d),
                Action::Completed { task_id, name } => {
                    self.ui.toast = Some(Toast { task_id, name, until: time + TOAST_SECS });
                }
                Action::Undo(id) => {
                    self.run(&Command::Uncomplete { id }, time);
                    self.ui.toast = None;
                }
                Action::SetLang(lang) => {
                    self.ui.lang = lang;
                    self.prefs.lang = Some(lang);
                }
                Action::SetTheme(theme) => {
                    self.ui.theme = theme;
                    self.prefs.theme = theme;
                    ctx.set_theme(theme.preference());
                }
            }
        }
    }

    fn file_filter(dialog: rfd::FileDialog, s: &Strings) -> rfd::FileDialog {
        dialog.add_filter(s.file_filter, &[EXTENSION])
    }

    fn menu(&mut self, ui: &mut egui::Ui) {
        let s = self.ui.s();
        ui.menu_button(s.file, |ui| {
            if ui.button(s.new_doc).clicked() {
                ui.close();
                if let Some(p) = Self::file_filter(rfd::FileDialog::new(), s)
                    .set_file_name(format!("BalanceCraft.{EXTENSION}"))
                    .save_file()
                {
                    self.new_document(p);
                }
            }
            if ui.button(s.open).clicked() {
                ui.close();
                if let Some(p) = Self::file_filter(rfd::FileDialog::new(), s).pick_file() {
                    self.open(&p);
                }
            }
            let recent = self.prefs.recent.clone();
            ui.add_enabled_ui(!recent.is_empty(), |ui| {
                ui.menu_button(s.open_recent, |ui| {
                    for p in recent {
                        if ui.button(p.display().to_string()).clicked() {
                            ui.close();
                            self.open(&p);
                        }
                    }
                });
            });
            ui.separator();
            if ui.button(s.import_cerqua).clicked() {
                ui.close();
                if let Some(backup) = rfd::FileDialog::new().add_filter(s.cerqua_filter, &["json"]).pick_file()
                    && let Some(target) = Self::file_filter(rfd::FileDialog::new(), s)
                        .set_file_name(format!("BalanceCraft.{EXTENSION}"))
                        .save_file()
                {
                    self.import_cerqua(&backup, target);
                }
            }
            ui.separator();
            if ui.add_enabled(self.store.is_some(), egui::Button::new(s.close_doc)).clicked() {
                ui.close();
                self.close();
            }
            if ui.button(s.quit).clicked() {
                ui.close();
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        if self.store.is_some() {
            ui.separator();
            let views = [
                (View::Today, s.today),
                (View::Tree, s.tree),
                (View::Areas, s.areas),
                (View::Waiting, s.waiting),
                (View::History, s.history),
                (View::Settings, s.settings),
            ];
            for (view, label) in views {
                if ui.selectable_label(self.ui.view == view, label).clicked() {
                    self.ui.view = view;
                }
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let s = self.ui.s();
        ui.horizontal(|ui| {
            let (text, color) = match &self.status {
                Status::Saved => (s.status_saved.to_owned(), ui.visuals().weak_text_color()),
                Status::Pending => (s.status_pending.to_owned(), ui.visuals().weak_text_color()),
                Status::Merged => (s.status_merged.to_owned(), ui.visuals().hyperlink_color),
                Status::Error(e) => (format!("{}: {e}", s.status_error), ui.visuals().error_fg_color),
            };
            crate::views::live(&ui.label(egui::RichText::new(text).color(color)));
            if let Some(store) = &self.store {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(store.shared_path().display().to_string()).weak().small());
                });
            }
        });
        if !self.manual_copies.is_empty() {
            ui.label(egui::RichText::new(s.manual_copies).color(ui.visuals().warn_fg_color));
            for p in &self.manual_copies {
                ui.label(egui::RichText::new(p.display().to_string()).small());
            }
        }
    }

    fn toast(&mut self, ui: &mut egui::Ui) {
        let s = self.ui.s();
        let Some(t) = self.ui.toast.clone() else { return };
        ui.horizontal(|ui| {
            crate::views::live(&ui.label(format!("{} {}", s.completed_toast, t.name)));
            if ui.button(s.undo).clicked() {
                self.ui.actions.push(Action::Undo(t.task_id.clone()));
            }
        });
    }

    fn empty(&mut self, ui: &mut egui::Ui) {
        let s = self.ui.s();
        ui.vertical_centered(|ui| {
            ui.add_space(80.0);
            ui.heading(s.no_document);
            ui.add_space(8.0);
            ui.label(s.no_document_hint);
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui.button(s.new_doc).clicked()
                    && let Some(p) = Self::file_filter(rfd::FileDialog::new(), s)
                        .set_file_name(format!("BalanceCraft.{EXTENSION}"))
                        .save_file()
                {
                    self.new_document(p);
                }
                if ui.button(s.open).clicked()
                    && let Some(p) = Self::file_filter(rfd::FileDialog::new(), s).pick_file()
                {
                    self.open(&p);
                }
            });
        });
    }
}

impl eframe::App for BalanceCraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let time = ctx.input(|i| i.time);
        self.tick(time);
        ctx.request_repaint_after(Duration::from_secs(1));

        egui::Panel::top("menu").show(ui, |ui| {
            ui.horizontal(|ui| self.menu(ui));
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            if self.ui.toast.is_some() {
                self.toast(ui);
                ui.separator();
            }
            self.status_bar(ui);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let Some(store) = &self.store else {
                self.empty(ui);
                return;
            };
            let data = store.data();
            let meta = (store.document_id().to_owned(), store.shared_path().to_owned());
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.ui.view {
                View::Today => views::today(ui, data, &mut self.ui),
                View::Tree => views::tree(ui, data, &mut self.ui),
                View::Areas => views::areas(ui, data, &mut self.ui),
                View::Waiting => views::waiting(ui, data, &mut self.ui),
                View::History => views::history(ui, data, &mut self.ui),
                View::Settings => views::settings(ui, data, &mut self.ui, &meta.0, &meta.1),
            });
        });
        if let Some(store) = &self.store {
            dialogs::show(&ctx, store.data(), &mut self.ui);
        }
        self.apply_actions(&ctx, time);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.dirty_since.is_some() {
            self.sync();
        }
        let mut prefs = self.prefs.clone();
        prefs.last = self.store.as_ref().map(|s| s.shared_path().to_owned());
        eframe::set_value(storage, PREFS_KEY, &prefs);
    }
}

#[cfg(test)]
mod theme_tests {
    use super::{Prefs, Theme};

    #[test]
    fn theme_follows_the_system_until_chosen() {
        // Preferences saved before the theme existed open with the system's colours.
        let old: Prefs = serde_json::from_str(r#"{"recent":[]}"#).unwrap();
        assert_eq!(old.theme, Theme::System);
        let chosen = Prefs { theme: Theme::Dark, ..Prefs::default() };
        let back: Prefs = serde_json::from_str(&serde_json::to_string(&chosen).unwrap()).unwrap();
        assert_eq!(back.theme, Theme::Dark);
    }
}
