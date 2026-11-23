//! Modal dialogs: task and area forms, wait reason, delete confirmation and
//! "Move to…". A dialog whose target was deleted on another computer while it
//! was open closes with a notice, as Cerqua's forms do (#137, #155).

use crate::app::{Action, UiState, new_id};
use crate::forms::{AreaForm, AreaValues, TaskForm, TaskValues};
use crate::i18n::Strings;
use crate::views::area_color;
use balancecraft_engine::commands::Command;
use balancecraft_engine::defaults::AREA_COLORS;
use balancecraft_engine::stamps::alive;
use balancecraft_engine::tree::{find_visible_area, find_visible_task, is_visible_parent, search};
use balancecraft_model::{AppData, TaskNode};
use egui::RichText;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskMode {
    New { parent_id: String },
    Edit { id: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Task(String),
    Area(String),
}

#[derive(Debug, Clone)]
pub enum Dialog {
    Task {
        mode: TaskMode,
        form: TaskForm,
        /// The values shown at opening, for the patch of an edit.
        opened: Option<TaskValues>,
        error: Option<&'static str>,
        /// The warning the task inherits when its own field is empty.
        inherited: i64,
    },
    Area {
        id: Option<String>,
        form: AreaForm,
        opened: Option<AreaValues>,
        error: Option<&'static str>,
        inherited: i64,
    },
    Wait {
        id: String,
        name: String,
        reason: String,
    },
    Delete {
        target: Target,
        name: String,
    },
    Move {
        id: String,
        name: String,
        filter: String,
    },
    Notice(&'static str),
}

/// The dialog's target is still there.
fn target_alive(d: &Dialog, data: &AppData) -> bool {
    match d {
        Dialog::Task { mode: TaskMode::New { parent_id }, .. } => is_visible_parent(&data.areas, parent_id),
        Dialog::Task { mode: TaskMode::Edit { id }, .. }
        | Dialog::Wait { id, .. }
        | Dialog::Move { id, .. }
        | Dialog::Delete { target: Target::Task(id), .. } => find_visible_task(&data.areas, id).is_some(),
        Dialog::Area { id: Some(id), .. } | Dialog::Delete { target: Target::Area(id), .. } => {
            find_visible_area(&data.areas, id).is_some()
        }
        Dialog::Area { id: None, .. } | Dialog::Notice(_) => true,
    }
}

fn buttons(ui: &mut egui::Ui, s: &Strings, ok: &str) -> (bool, bool) {
    let mut out = (false, false);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        out.0 = ui.button(ok).clicked();
        out.1 = ui.button(s.cancel).clicked();
    });
    out
}

fn field(ui: &mut egui::Ui, label: &str, text: &mut String, hint: &str) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(280.0));
    ui.end_row();
}

fn error_line(ui: &mut egui::Ui, error: Option<&'static str>) {
    if let Some(e) = error {
        ui.label(RichText::new(e).color(ui.visuals().error_fg_color));
    }
}

/// Draws the open dialog; returns whether it stays open.
fn body(ui: &mut egui::Ui, d: &mut Dialog, data: &AppData, s: &'static Strings, actions: &mut Vec<Action>) -> bool {
    match d {
        Dialog::Task { mode, form, opened, error, inherited } => {
            ui.heading(if matches!(mode, TaskMode::New { .. }) { s.new_task } else { s.edit_task });
            ui.add_space(8.0);
            egui::Grid::new("task-form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                field(ui, s.name, &mut form.name, "");
                field(ui, s.weight, &mut form.weight, "5");
                field(ui, s.due_date, &mut form.due, s.date_hint);
                field(ui, s.start_date, &mut form.start, s.date_hint);
                field(ui, s.advance_days, &mut form.advance, &format!("{} ({inherited})", s.inherited));
                ui.label(s.notes);
                ui.add(egui::TextEdit::multiline(&mut form.notes).desired_rows(3).desired_width(280.0));
                ui.end_row();
            });
            error_line(ui, *error);
            let (ok, cancel) = buttons(ui, s, s.save);
            if cancel {
                return false;
            }
            if ok {
                match form.validate(s) {
                    Err(e) => *error = Some(e),
                    Ok(values) => {
                        let cmd = match (mode, opened) {
                            (TaskMode::New { parent_id }, _) => Some(Command::AddTask {
                                parent_id: parent_id.clone(),
                                id: new_id(),
                                task: values.new_task(),
                            }),
                            (TaskMode::Edit { id }, Some(opened)) => {
                                let changes = values.changes(opened);
                                (!changes.is_empty()).then(|| Command::EditTask { id: id.clone(), changes })
                            }
                            (TaskMode::Edit { .. }, None) => None,
                        };
                        actions.extend(cmd.map(Action::Run));
                        return false;
                    }
                }
            }
            true
        }
        Dialog::Area { id, form, opened, error, inherited } => {
            ui.heading(if id.is_none() { s.new_area } else { s.edit_area });
            ui.add_space(8.0);
            egui::Grid::new("area-form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                field(ui, s.name, &mut form.name, "");
                field(ui, s.weight, &mut form.weight, "10");
                ui.label(s.color);
                ui.horizontal(|ui| {
                    for c in AREA_COLORS {
                        let selected = form.color.eq_ignore_ascii_case(c);
                        let (rect, r) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                        r.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, c));
                        let painter = ui.painter();
                        painter.circle_filled(rect.center(), 8.0, area_color(c));
                        if selected || r.hovered() {
                            painter.circle_stroke(
                                rect.center(),
                                10.5,
                                egui::Stroke::new(1.5, ui.visuals().strong_text_color()),
                            );
                        }
                        if r.on_hover_text(c).clicked() {
                            form.color = c.to_owned();
                        }
                    }
                });
                ui.end_row();
                field(ui, s.advance_days, &mut form.advance, &format!("{} ({inherited})", s.inherited));
            });
            error_line(ui, *error);
            let (ok, cancel) = buttons(ui, s, s.save);
            if cancel {
                return false;
            }
            if ok {
                match form.validate(s) {
                    Err(e) => *error = Some(e),
                    Ok(values) => {
                        let cmd = match (id, opened) {
                            (None, _) => Some(Command::AddArea { id: new_id(), area: values.new_area() }),
                            (Some(id), Some(opened)) => {
                                let changes = values.changes(opened);
                                (!changes.is_empty()).then(|| Command::EditArea { id: id.clone(), changes })
                            }
                            (Some(_), None) => None,
                        };
                        actions.extend(cmd.map(Action::Run));
                        return false;
                    }
                }
            }
            true
        }
        Dialog::Wait { id, name, reason } => {
            ui.heading(s.put_on_hold);
            ui.label(RichText::new(name.as_str()).strong());
            ui.add_space(8.0);
            ui.label(s.reason);
            let r = ui.add(egui::TextEdit::singleline(reason).desired_width(280.0));
            r.request_focus();
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let (ok, cancel) = buttons(ui, s, s.put_on_hold);
            if cancel {
                return false;
            }
            if ok || enter {
                actions.push(Action::Run(Command::PutOnHold {
                    id: id.clone(),
                    entry_id: new_id(),
                    reason: reason.trim().to_owned(),
                }));
                return false;
            }
            true
        }
        Dialog::Delete { target, name } => {
            ui.label(match target {
                Target::Task(_) => s.confirm_delete_task,
                Target::Area(_) => s.confirm_delete_area,
            });
            ui.label(RichText::new(name.as_str()).strong());
            let (ok, cancel) = buttons(ui, s, s.delete);
            if cancel {
                return false;
            }
            if ok {
                actions.push(Action::Run(match target {
                    Target::Task(id) => Command::DeleteTask { id: id.clone() },
                    Target::Area(id) => Command::DeleteArea { id: id.clone() },
                }));
                return false;
            }
            true
        }
        Dialog::Move { id, name, filter } => {
            ui.heading(s.move_to);
            ui.label(RichText::new(name.as_str()).strong());
            ui.add(egui::TextEdit::singleline(filter).hint_text("…").desired_width(320.0));
            let moving = find_visible_task(&data.areas, id).map(|(n, _)| n);
            let mut chosen = None;
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                let f = filter.trim().to_lowercase();
                for a in data.areas.iter().filter(|a| alive(a.meta.as_ref())) {
                    if f.is_empty() || a.name.to_lowercase().contains(&f) {
                        let text = RichText::new(&a.name).strong().color(area_color(&a.color));
                        if ui.selectable_label(false, text).clicked() {
                            chosen = Some(a.id.clone());
                        }
                    }
                    targets(ui, &a.children, 1, moving, &f, &mut chosen);
                }
            });
            let (_, cancel) = {
                ui.add_space(8.0);
                (false, ui.button(s.cancel).clicked())
            };
            if cancel {
                return false;
            }
            if let Some(new_parent_id) = chosen {
                actions.push(Action::Run(Command::MoveTask { id: id.clone(), new_parent_id }));
                return false;
            }
            true
        }
        Dialog::Notice(text) => {
            ui.label(*text);
            ui.add_space(8.0);
            !ui.button("OK").clicked()
        }
    }
}

/// The places a task can move to: visible tasks outside its own subtree.
fn targets(
    ui: &mut egui::Ui,
    nodes: &[TaskNode],
    depth: usize,
    moving: Option<&TaskNode>,
    filter: &str,
    chosen: &mut Option<String>,
) {
    for n in nodes.iter().filter(|n| alive(n.meta.as_ref())) {
        if moving.is_some_and(|m| search(m, &n.id).is_some()) {
            continue;
        }
        if filter.is_empty() || n.name.to_lowercase().contains(filter) {
            #[allow(clippy::cast_precision_loss)]
            ui.horizontal(|ui| {
                ui.add_space(16.0 * depth as f32);
                if ui.selectable_label(false, &n.name).clicked() {
                    *chosen = Some(n.id.clone());
                }
            });
        }
        targets(ui, &n.children, depth + 1, moving, filter, chosen);
    }
}

pub fn show(ctx: &egui::Context, data: &AppData, state: &mut UiState) {
    let Some(mut d) = state.dialog.take() else { return };
    let s = state.s();
    if !target_alive(&d, data) {
        d = Dialog::Notice(s.target_gone);
    }
    let mut actions = Vec::new();
    let response = egui::Modal::new(egui::Id::unique("dialog")).show(ctx, |ui| {
        ui.set_max_width(440.0);
        body(ui, &mut d, data, s, &mut actions)
    });
    state.actions.extend(actions);
    if response.inner && !response.should_close() {
        state.dialog = Some(d);
    }
}
