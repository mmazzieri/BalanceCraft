//! Modal dialogs: task and area forms, wait reason, delete confirmation and
//! "Move to…". A dialog whose target was deleted on another computer while it
//! was open closes with a notice, as Cerqua's forms do (#137, #155).

use crate::app::{Action, UiState, new_id};
use crate::forms::{AreaForm, AreaValues, TaskForm, TaskValues};
use crate::i18n::Strings;
use crate::views::{area_color, completed_ids, leaf_options, local_fmt, local_input, local_input_to_iso, swatch};
use balancecraft_engine::commands::Command;
use balancecraft_engine::defaults::AREA_COLORS;
use balancecraft_engine::portfolio::actions_for;
use balancecraft_engine::stamps::alive;
use balancecraft_engine::tree::{find_visible_area, find_visible_task, is_visible_parent, search};
use balancecraft_model::{AppData, Area, TaskNode};
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
        prereq_filter: String,
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
    Pulse(Box<Pulse>),
    Notice(&'static str),
}

/// The page of an area in the Areas view: status, next action and log.
/// As in Cerqua since 1.44.4 (#156), each value is written only when it
/// differs from the one shown when it was opened for editing.
#[derive(Debug, Clone)]
pub struct Pulse {
    pub area_id: String,
    pub status: String,
    pub status_opened: String,
    pub next: Option<NextEdit>,
    pub log_text: String,
    pub entry: Option<EntryEdit>,
}

#[derive(Debug, Clone)]
pub struct NextEdit {
    pub text: String,
    pub task: Option<String>,
    pub opened_text: String,
    pub opened_task: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EntryEdit {
    pub id: String,
    pub text: String,
    pub date: String,
    pub opened_text: String,
    pub opened_date: String,
}

impl Pulse {
    pub fn open(area: &Area) -> Self {
        let status = area.status.clone().unwrap_or_default();
        Pulse {
            area_id: area.id.clone(),
            status_opened: status.trim().to_owned(),
            status,
            next: None,
            log_text: String::new(),
            entry: None,
        }
    }

    /// The status, when it changed since it was shown or last saved.
    fn flush_status(&mut self, actions: &mut Vec<Action>) {
        let value = self.status.trim();
        if value != self.status_opened {
            actions
                .push(Action::Run(Command::SetAreaStatus { area_id: self.area_id.clone(), status: value.to_owned() }));
            self.status_opened = value.to_owned();
        }
    }
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
        Dialog::Pulse(p) => find_visible_area(&data.areas, &p.area_id).is_some(),
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
    let name = ui.label(label);
    ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(280.0)).labelled_by(name.id);
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
        Dialog::Task { mode, form, opened, error, inherited, prereq_filter } => {
            ui.heading(if matches!(mode, TaskMode::New { .. }) { s.new_task } else { s.edit_task });
            ui.add_space(8.0);
            egui::Grid::new("task-form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                field(ui, s.name, &mut form.name, "");
                field(ui, s.weight, &mut form.weight, "5");
                field(ui, s.due_date, &mut form.due, s.date_hint);
                field(ui, s.start_date, &mut form.start, s.date_hint);
                field(ui, s.advance_days, &mut form.advance, &format!("{} ({inherited})", s.inherited));
                let name = ui.label(s.notes);
                ui.add(egui::TextEdit::multiline(&mut form.notes).desired_rows(3).desired_width(280.0))
                    .labelled_by(name.id);
                ui.end_row();
            });
            let own = match mode {
                TaskMode::Edit { id } => Some(id.as_str()),
                TaskMode::New { .. } => None,
            };
            prereq_picker(ui, data, s, form, own, prereq_filter);
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
                                let current = find_visible_task(&data.areas, id)
                                    .and_then(|(n, _)| n.prereqs.clone())
                                    .unwrap_or_default();
                                let changes = values.changes(opened, &current);
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
            let name = ui.label(s.reason);
            let r = ui.add(egui::TextEdit::singleline(reason).desired_width(280.0)).labelled_by(name.id);
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
            ui.add(egui::TextEdit::singleline(filter).hint_text(s.filter).desired_width(320.0));
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
                actions.push(Action::Run(Command::MoveTask { id: id.clone(), new_parent_id, before: None }));
                return false;
            }
            true
        }
        Dialog::Pulse(p) => pulse(ui, p, data, s, actions),
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

/// Leaves a task can depend on: every visible leaf but the task itself.
fn prereq_picker(
    ui: &mut egui::Ui,
    data: &AppData,
    s: &'static Strings,
    form: &mut TaskForm,
    own: Option<&str>,
    filter: &mut String,
) {
    let leaves: Vec<(String, String)> =
        leaf_options(data, None).into_iter().filter(|(id, _)| Some(id.as_str()) != own).collect();
    if leaves.is_empty() {
        return;
    }
    ui.add_space(6.0);
    egui::CollapsingHeader::new(format!("{} ({})", s.prereqs, form.prereqs.len()))
        .id_salt("prereqs")
        .default_open(!form.prereqs.is_empty())
        .show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(filter).hint_text(s.filter).desired_width(320.0));
            let f = filter.trim().to_lowercase();
            egui::ScrollArea::vertical().max_height(150.0).id_salt("prereq-list").show(ui, |ui| {
                for (id, label) in &leaves {
                    let mut on = form.prereqs.contains(id);
                    if !on && !f.is_empty() && !label.to_lowercase().contains(&f) {
                        continue;
                    }
                    if ui.checkbox(&mut on, label).changed() {
                        form.toggle_prereq(id);
                    }
                }
            });
        });
}

fn pulse(ui: &mut egui::Ui, p: &mut Pulse, data: &AppData, s: &'static Strings, actions: &mut Vec<Action>) -> bool {
    let Some(area) = find_visible_area(&data.areas, &p.area_id) else { return false };
    ui.horizontal(|ui| {
        swatch(ui, area_color(&area.color));
        ui.heading(&area.name);
    });
    ui.add_space(8.0);

    let name = ui.label(RichText::new(s.status).strong());
    let r = ui
        .add(egui::TextEdit::singleline(&mut p.status).hint_text(s.no_status).desired_width(400.0))
        .labelled_by(name.id);
    if r.lost_focus() {
        p.flush_status(actions);
    }
    ui.add_space(8.0);

    let next_name = ui.label(RichText::new(s.next_action).strong());
    let candidates = leaf_options(data, Some(&p.area_id));
    let done = completed_ids(data);
    let candidates: Vec<(String, String)> = candidates.into_iter().filter(|(id, _)| !done.contains(id)).collect();
    let mut set_next = None;
    let mut close_next = false;
    if let Some(edit) = &mut p.next {
        ui.add(egui::TextEdit::singleline(&mut edit.text).desired_width(400.0)).labelled_by(next_name.id);
        ui.horizontal(|ui| {
            let name = ui.label(s.linked_task);
            let label = |id: &Option<String>| {
                id.as_ref()
                    .and_then(|id| candidates.iter().find(|(c, _)| c == id))
                    .map_or_else(|| s.no_task.to_owned(), |(_, l)| l.clone())
            };
            egui::ComboBox::from_id_salt("next-task")
                .selected_text(label(&edit.task))
                .width(300.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut edit.task, None, s.no_task);
                    for (id, l) in &candidates {
                        if ui.selectable_label(edit.task.as_deref() == Some(id), l).clicked() {
                            edit.task = Some(id.clone());
                            if edit.text.trim().is_empty() {
                                edit.text = l.rsplit(" › ").next().unwrap_or(l).to_owned();
                            }
                        }
                    }
                })
                .response
                .labelled_by(name.id);
        });
        ui.horizontal(|ui| {
            if ui.button(s.set).clicked() {
                let text = edit.text.trim();
                if text != edit.opened_text || edit.task != edit.opened_task {
                    set_next = Some(Command::SetAreaNextAction {
                        area_id: p.area_id.clone(),
                        text: text.to_owned(),
                        task_id: edit.task.clone(),
                    });
                }
                close_next = true;
            }
            if ui.button(s.cancel).clicked() {
                close_next = true;
            }
        });
    } else {
        let text = area.next_action_text.as_deref().unwrap_or_default();
        if text.is_empty() {
            ui.label(RichText::new(s.no_next).weak());
        } else {
            ui.label(text);
        }
        ui.horizontal(|ui| {
            if ui.button(s.edit).clicked() {
                let task = area.next_action_task_id.clone();
                p.next = Some(NextEdit {
                    text: text.to_owned(),
                    opened_text: text.to_owned(),
                    opened_task: task.clone(),
                    task,
                });
            }
            if !text.is_empty() && ui.button(s.complete).clicked() {
                actions.push(Action::Run(Command::CompleteNextAction {
                    area_id: p.area_id.clone(),
                    id: new_id(),
                    text: None,
                }));
            }
        });
    }
    actions.extend(set_next.map(Action::Run));
    if close_next {
        p.next = None;
    }
    ui.add_space(8.0);

    let log_name = ui.label(RichText::new(s.log).strong());
    ui.horizontal(|ui| {
        let r = ui
            .add(egui::TextEdit::singleline(&mut p.log_text).hint_text(s.log_hint).desired_width(320.0))
            .labelled_by(log_name.id);
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button(s.add).clicked() || enter) && !p.log_text.trim().is_empty() {
            actions.push(Action::Run(Command::LogAreaAction {
                area_id: p.area_id.clone(),
                id: new_id(),
                text: p.log_text.trim().to_owned(),
            }));
            p.log_text.clear();
        }
    });
    let log = data.area_actions.as_deref().unwrap_or_default();
    let mut close_entry = false;
    egui::ScrollArea::vertical().max_height(260.0).id_salt("log").show(ui, |ui| {
        for e in actions_for(&p.area_id, log) {
            match &mut p.entry {
                Some(edit) if edit.id == e.id => {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut edit.date).desired_width(120.0).hint_text(s.date_hint));
                        ui.add(egui::TextEdit::singleline(&mut edit.text).desired_width(220.0).hint_text(s.log_hint));
                    });
                    ui.horizontal(|ui| {
                        if ui.button(s.save).clicked() {
                            let text = edit.text.trim();
                            let text = (text != edit.opened_text.trim()).then(|| text.to_owned());
                            let at = (edit.date.trim() != edit.opened_date)
                                .then(|| local_input_to_iso(&edit.date))
                                .flatten();
                            if text.is_some() || at.is_some() {
                                actions.push(Action::Run(Command::EditAreaAction { id: e.id.clone(), text, at }));
                            }
                            close_entry = true;
                        }
                        if ui.button(s.cancel).clicked() {
                            close_entry = true;
                        }
                    });
                }
                _ => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(local_fmt(&e.at, s.date_fmt)).small().weak());
                        ui.label(&e.text);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(s.delete).clicked() {
                                actions.push(Action::Run(Command::DeleteAreaAction { id: e.id.clone() }));
                            }
                            if ui.small_button(s.edit).clicked() {
                                let date = local_input(&e.at);
                                p.entry = Some(EntryEdit {
                                    id: e.id.clone(),
                                    text: e.text.clone(),
                                    opened_text: e.text.clone(),
                                    opened_date: date.clone(),
                                    date,
                                });
                            }
                        });
                    });
                }
            }
        }
    });
    if close_entry {
        p.entry = None;
    }
    ui.add_space(8.0);
    !ui.button(s.close).clicked()
}

pub fn show(ctx: &egui::Context, data: &AppData, state: &mut UiState) {
    let Some(mut d) = state.dialog.take() else {
        state.focus.no_dialog(ctx);
        return;
    };
    let s = state.s();
    if !target_alive(&d, data) {
        d = Dialog::Notice(s.target_gone);
    }
    let mut actions = Vec::new();
    state.focus.before_dialog(ctx);
    let response = egui::Modal::new(egui::Id::unique("dialog")).show(ctx, |ui| {
        ui.set_max_width(440.0);
        body(ui, &mut d, data, s, &mut actions)
    });
    if response.inner && !response.should_close() {
        state.dialog = Some(d);
    } else {
        if let Dialog::Pulse(p) = &mut d {
            // Closing writes the status being typed, as Cerqua does.
            p.flush_status(&mut actions);
        }
        state.focus.dialog_closed(ctx);
    }
    state.actions.extend(actions);
}
