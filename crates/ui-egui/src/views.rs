//! The pages: Today, the Tree and Settings. A view reads the document and
//! pushes [`Action`]s; nothing changes while the frame is drawn.

use crate::app::{Action, UiState, new_id};
use crate::dialogs::{Dialog, Target, TaskMode};
use crate::forms::{AreaForm, TaskForm};
use crate::i18n::{Lang, Strings};
use balancecraft_engine::calendar::{DeadlineStatus, Light, When, deadline_status};
use balancecraft_engine::commands::Command;
use balancecraft_engine::defaults::AREA_COLORS;
use balancecraft_engine::settings::{Resolved, effective_settings, global_advance_days};
use balancecraft_engine::stamps::alive;
use balancecraft_engine::tree::path_names;
use balancecraft_engine::{Clock, compute_flat_list};
use balancecraft_model::{AppData, Area, Num, TaskNode};
use egui::{Color32, RichText};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn area_color(hex: &str) -> Color32 {
    Color32::from_hex(hex).unwrap_or(Color32::GRAY)
}

fn swatch(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 16.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, color);
}

fn light_color(ui: &egui::Ui, light: Light) -> Color32 {
    match light {
        Light::Red => ui.visuals().error_fg_color,
        Light::Yellow => ui.visuals().warn_fg_color,
        Light::Green => Color32::from_rgb(0x4A, 0x9C, 0x59),
    }
}

fn deadline_text(st: DeadlineStatus, s: &Strings) -> String {
    match st.when {
        When::Overdue => format!("⚠ {}", s.overdue),
        When::Today => format!("📅 {}", s.due_today),
        When::Future => format!("📅 {} {}", st.working_days_left, s.working_days),
    }
}

fn settings_map(data: &AppData) -> HashMap<String, Resolved> {
    effective_settings(&data.areas, data.settings.as_ref().map(|g| &g.values))
}

fn advance_for(map: &HashMap<String, Resolved>, data: &AppData, id: &str) -> i64 {
    map.get(id).map_or_else(|| global_advance_days(data.settings.as_ref().map(|g| &g.values)), |r| r.effective)
}

fn today_str(clock: &Clock<chrono::Local>) -> String {
    clock.now_dt().get(..10).unwrap_or_default().to_owned()
}

fn live_areas(data: &AppData) -> impl Iterator<Item = &Area> {
    data.areas.iter().filter(|a| alive(a.meta.as_ref()))
}

fn live_children(children: &[TaskNode]) -> impl Iterator<Item = &TaskNode> {
    children.iter().filter(|c| alive(c.meta.as_ref()))
}

fn has_live_children(n: &TaskNode) -> bool {
    live_children(&n.children).next().is_some()
}

pub fn edit_task_dialog(data: &AppData, task: &TaskNode, s: &Strings) -> Dialog {
    let form = TaskForm::from_task(task);
    let opened = form.validate(s).ok();
    let inherited = settings_map(data).get(&task.id).map_or(5, |r| r.inherited);
    Dialog::Task { mode: TaskMode::Edit { id: task.id.clone() }, form, opened, error: None, inherited }
}

pub fn new_task_dialog(data: &AppData, parent_id: &str) -> Dialog {
    let inherited = advance_for(&settings_map(data), data, parent_id);
    Dialog::Task {
        mode: TaskMode::New { parent_id: parent_id.to_owned() },
        form: TaskForm::new(),
        opened: None,
        error: None,
        inherited,
    }
}

fn complete(state: &mut UiState, task: &TaskNode) {
    state.actions.push(Action::Run(Command::CompleteTask {
        id: task.id.clone(),
        entry_id: new_id(),
        action_id: new_id(),
    }));
    state.actions.push(Action::Completed { task_id: task.id.clone(), name: task.name.clone() });
}

pub fn today(ui: &mut egui::Ui, data: &AppData, state: &mut UiState) {
    let s = state.s();
    let Some(clock) = Clock::local() else {
        ui.label("clock error");
        return;
    };
    let today = today_str(&clock);
    let map = settings_map(data);
    let list = compute_flat_list(&data.areas, &data.history, &clock);

    let selected = state
        .area_filter
        .as_ref()
        .and_then(|id| live_areas(data).find(|a| &a.id == id))
        .map_or(s.all_areas.to_owned(), |a| a.name.clone());
    egui::ComboBox::from_id_salt("area-filter").selected_text(selected).show_ui(ui, |ui| {
        ui.selectable_value(&mut state.area_filter, None, s.all_areas);
        for a in live_areas(data) {
            ui.selectable_value(&mut state.area_filter, Some(a.id.clone()), &a.name);
        }
    });
    ui.add_space(8.0);

    let mut shown = 0;
    let filter = state.area_filter.clone();
    for item in list.iter().filter(|i| filter.as_ref().is_none_or(|f| *f == i.area.id)) {
        shown += 1;
        let task = item.task;
        ui.horizontal(|ui| {
            swatch(ui, area_color(&item.area.color));
            ui.vertical(|ui| {
                ui.label(RichText::new(&task.name).strong());
                ui.horizontal(|ui| {
                    let mut path = path_names(&data.areas, &task.id).unwrap_or_default();
                    path.pop();
                    ui.label(RichText::new(path.join(" › ")).small().weak());
                    if let Some(due) = task.due_date.as_deref().filter(|d| !d.is_empty()) {
                        let st = deadline_status(due, &today, advance_for(&map, data, &task.id));
                        ui.label(RichText::new(deadline_text(st, s)).small().color(light_color(ui, st.light)));
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(s.complete).clicked() {
                    complete(state, task);
                }
                if ui.button(s.wait).clicked() {
                    state.actions.push(Action::Open(Dialog::Wait {
                        id: task.id.clone(),
                        name: task.name.clone(),
                        reason: String::new(),
                    }));
                }
                if ui.button(s.edit).clicked() {
                    state.actions.push(Action::Open(edit_task_dialog(data, task, s)));
                }
                if ui.button(s.add_subtask).clicked() {
                    state.actions.push(Action::Open(new_task_dialog(data, &task.id)));
                }
            });
        });
        ui.separator();
    }
    if shown == 0 {
        ui.add_space(24.0);
        ui.label(RichText::new(s.nothing_today).weak());
    }
}

/// History of the visible tasks: completed ids and the reason of each wait.
struct Done {
    completed: HashSet<String>,
    waiting: HashMap<String, String>,
}

fn done(data: &AppData) -> Done {
    let mut d = Done { completed: HashSet::new(), waiting: HashMap::new() };
    for h in data.history.iter().filter(|h| alive(h.meta.as_ref())) {
        if h.waiting.unwrap_or(false) {
            d.waiting.insert(h.task_id.clone(), h.waiting_for.clone().unwrap_or_default());
        } else {
            d.completed.insert(h.task_id.clone());
        }
    }
    d
}

fn reorder(ids: &[&str], i: usize, up: bool) -> Option<Vec<String>> {
    let j = if up { i.checked_sub(1)? } else { i + 1 };
    if j >= ids.len() {
        return None;
    }
    let mut v: Vec<String> = ids.iter().map(|s| (*s).to_owned()).collect();
    v.swap(i, j);
    Some(v)
}

fn fmt_weight(w: Option<Num>) -> String {
    w.map(|n| format!("{}", n.get())).unwrap_or_default()
}

struct TreeCtx<'a> {
    data: &'a AppData,
    s: &'static Strings,
    done: Done,
    map: HashMap<String, Resolved>,
    today: String,
}

fn node_menu(
    ui: &mut egui::Ui,
    t: &TreeCtx<'_>,
    state: &mut UiState,
    node: &TaskNode,
    parent_id: &str,
    siblings: &[&str],
    i: usize,
) {
    let s = t.s;
    let leaf = !has_live_children(node);
    ui.menu_button("…", |ui| {
        if ui.button(s.add_subtask).clicked() {
            state.actions.push(Action::Open(new_task_dialog(t.data, &node.id)));
            ui.close();
        }
        if ui.button(s.edit).clicked() {
            state.actions.push(Action::Open(edit_task_dialog(t.data, node, s)));
            ui.close();
        }
        if ui.button(s.move_to).clicked() {
            state.actions.push(Action::Open(Dialog::Move {
                id: node.id.clone(),
                name: node.name.clone(),
                filter: String::new(),
            }));
            ui.close();
        }
        for (up, label) in [(true, s.move_up), (false, s.move_down)] {
            if let Some(order) = reorder(siblings, i, up)
                && ui.button(label).clicked()
            {
                state.actions.push(Action::Run(Command::ReorderChildren { parent_id: parent_id.to_owned(), order }));
                ui.close();
            }
        }
        if leaf {
            ui.separator();
            if t.done.completed.contains(&node.id) {
                if ui.button(s.uncomplete).clicked() {
                    state.actions.push(Action::Run(Command::Uncomplete { id: node.id.clone() }));
                    ui.close();
                }
            } else {
                if ui.button(s.complete).clicked() {
                    complete(state, node);
                    ui.close();
                }
                if t.done.waiting.contains_key(&node.id) {
                    if ui.button(s.resume).clicked() {
                        state.actions.push(Action::Run(Command::Resume { id: node.id.clone() }));
                        ui.close();
                    }
                } else if ui.button(s.put_on_hold).clicked() {
                    state.actions.push(Action::Open(Dialog::Wait {
                        id: node.id.clone(),
                        name: node.name.clone(),
                        reason: String::new(),
                    }));
                    ui.close();
                }
            }
        }
        ui.separator();
        if ui.button(s.delete).clicked() {
            state
                .actions
                .push(Action::Open(Dialog::Delete { target: Target::Task(node.id.clone()), name: node.name.clone() }));
            ui.close();
        }
    });
}

fn node_label(ui: &mut egui::Ui, t: &TreeCtx<'_>, node: &TaskNode) {
    let s = t.s;
    let completed = t.done.completed.contains(&node.id) && !has_live_children(node);
    let mut name = RichText::new(&node.name);
    if completed {
        name = name.strikethrough().weak();
    }
    ui.label(name);
    if node.weight.is_some() {
        ui.label(RichText::new(fmt_weight(node.weight)).small().weak());
    }
    if let Some(reason) = t.done.waiting.get(&node.id) {
        ui.label(RichText::new(format!("⏸ {} {reason}", s.waiting_for)).small().color(ui.visuals().warn_fg_color));
    }
    if let Some(due) = node.due_date.as_deref().filter(|d| !d.is_empty() && !completed) {
        let st = deadline_status(due, &t.today, advance_for(&t.map, t.data, &node.id));
        let date = due.get(..10).unwrap_or(due);
        ui.label(RichText::new(format!("{date} · {}", deadline_text(st, s))).small().color(light_color(ui, st.light)));
    }
}

fn tree_nodes(ui: &mut egui::Ui, t: &TreeCtx<'_>, state: &mut UiState, parent_id: &str, children: &[TaskNode]) {
    let live: Vec<&TaskNode> = live_children(children).collect();
    let ids: Vec<&str> = live.iter().map(|n| n.id.as_str()).collect();
    for (i, node) in live.iter().enumerate() {
        if has_live_children(node) {
            let id = ui.make_persistent_id(("node", &node.id));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
                .show_header(ui, |ui| {
                    node_label(ui, t, node);
                    node_menu(ui, t, state, node, parent_id, &ids, i);
                })
                .body(|ui| tree_nodes(ui, t, state, &node.id, &node.children));
        } else {
            ui.horizontal(|ui| {
                ui.add_space(ui.spacing().indent);
                node_label(ui, t, node);
                node_menu(ui, t, state, node, parent_id, &ids, i);
            });
        }
    }
}

pub fn tree(ui: &mut egui::Ui, data: &AppData, state: &mut UiState) {
    let s = state.s();
    let today = Clock::local().map(|c| today_str(&c)).unwrap_or_default();
    let t = TreeCtx { data, s, done: done(data), map: settings_map(data), today };
    if ui.button(s.add_area).clicked() {
        let used: HashSet<&str> = live_areas(data).map(|a| a.color.as_str()).collect();
        let color = AREA_COLORS.iter().find(|c| !used.contains(**c)).unwrap_or(&AREA_COLORS[0]);
        let inherited = global_advance_days(data.settings.as_ref().map(|g| &g.values));
        state.actions.push(Action::Open(Dialog::Area {
            id: None,
            form: AreaForm::new(color),
            opened: None,
            error: None,
            inherited,
        }));
    }
    ui.add_space(8.0);
    let areas: Vec<&Area> = live_areas(data).collect();
    let ids: Vec<&str> = areas.iter().map(|a| a.id.as_str()).collect();
    for (i, area) in areas.iter().enumerate() {
        let id = ui.make_persistent_id(("area", &area.id));
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
            .show_header(ui, |ui| {
                swatch(ui, area_color(&area.color));
                ui.label(RichText::new(&area.name).strong());
                ui.label(RichText::new(fmt_weight(area.weight)).small().weak());
                ui.menu_button("…", |ui| {
                    if ui.button(s.add_task).clicked() {
                        state.actions.push(Action::Open(new_task_dialog(data, &area.id)));
                        ui.close();
                    }
                    if ui.button(s.edit).clicked() {
                        let form = AreaForm::from_area(area);
                        let opened = form.validate(s).ok();
                        let inherited = t.map.get(&area.id).map_or(5, |r| r.inherited);
                        state.actions.push(Action::Open(Dialog::Area {
                            id: Some(area.id.clone()),
                            form,
                            opened,
                            error: None,
                            inherited,
                        }));
                        ui.close();
                    }
                    for (up, label) in [(true, s.move_up), (false, s.move_down)] {
                        if let Some(order) = reorder(&ids, i, up)
                            && ui.button(label).clicked()
                        {
                            state.actions.push(Action::Run(Command::ReorderAreas { order }));
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button(s.delete).clicked() {
                        state.actions.push(Action::Open(Dialog::Delete {
                            target: Target::Area(area.id.clone()),
                            name: area.name.clone(),
                        }));
                        ui.close();
                    }
                });
            })
            .body(|ui| tree_nodes(ui, &t, state, &area.id, &area.children));
    }
}

pub fn settings(ui: &mut egui::Ui, data: &AppData, state: &mut UiState, doc_id: &str, path: &Path) {
    let s = state.s();
    egui::Grid::new("settings").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
        ui.label(s.language);
        let mut lang = state.lang;
        egui::ComboBox::from_id_salt("lang")
            .selected_text(match lang {
                Lang::It => "Italiano",
                Lang::En => "English",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut lang, Lang::It, "Italiano");
                ui.selectable_value(&mut lang, Lang::En, "English");
            });
        if lang != state.lang {
            state.actions.push(Action::SetLang(lang));
        }
        ui.end_row();

        ui.label(s.global_advance);
        let stored = data
            .settings
            .as_ref()
            .and_then(|g| balancecraft_engine::settings::valid_advance_days(g.values.advance_days));
        let text = state.global_advance.get_or_insert_with(|| stored.map(|v| v.to_string()).unwrap_or_default());
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(text).desired_width(48.0).hint_text("5"));
            let parsed = balancecraft_engine::settings::parse_advance_days(text);
            use balancecraft_engine::settings::Parsed;
            #[allow(clippy::cast_precision_loss)]
            let value = match parsed {
                Parsed::Inherit => Some(None),
                Parsed::Value(v) => Some(Some(v as f64)),
                Parsed::Invalid => None,
            };
            match value {
                Some(v) => {
                    #[allow(clippy::cast_precision_loss)]
                    let changed = v != stored.map(|x| x as f64);
                    if ui.add_enabled(changed, egui::Button::new(s.save)).clicked() {
                        state.actions.push(Action::Run(Command::SetAdvanceDays { value: v }));
                    }
                }
                None => {
                    ui.label(RichText::new(s.err_advance).color(ui.visuals().error_fg_color));
                }
            }
        });
        ui.end_row();

        ui.label(s.document);
        ui.label(RichText::new(doc_id).monospace().small());
        ui.end_row();
        ui.label(s.location);
        ui.label(path.display().to_string());
        ui.end_row();
    });
}
