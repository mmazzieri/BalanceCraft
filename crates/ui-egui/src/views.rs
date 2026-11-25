//! The pages: Today, the Tree and Settings. A view reads the document and
//! pushes [`Action`]s; nothing changes while the frame is drawn.

use crate::app::{Action, Theme, UiState, new_id};
use crate::dialogs::{Dialog, Pulse, Target, TaskMode};
use crate::forms::{AreaForm, TaskForm};
use crate::i18n::{Lang, Strings};
use balancecraft_engine::calendar::{DeadlineStatus, Light, When, deadline_status};
use balancecraft_engine::commands::Command;
use balancecraft_engine::defaults::AREA_COLORS;
use balancecraft_engine::portfolio::{self, Severity};
use balancecraft_engine::settings::{Resolved, effective_settings, global_advance_days};
use balancecraft_engine::stamps::alive;
use balancecraft_engine::tree::{find_visible_task, path_names};
use balancecraft_engine::{Clock, compute_flat_list};
use balancecraft_model::{AppData, Area, HistoryEntry, Num, TaskNode};
use egui::{Color32, RichText};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn area_color(hex: &str) -> Color32 {
    Color32::from_hex(hex).unwrap_or(Color32::GRAY)
}

pub fn swatch(ui: &mut egui::Ui, color: Color32) {
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
    Dialog::Task {
        mode: TaskMode::Edit { id: task.id.clone() },
        form,
        opened,
        error: None,
        inherited,
        prereq_filter: String::new(),
    }
}

pub fn new_task_dialog(data: &AppData, parent_id: &str) -> Dialog {
    let inherited = advance_for(&settings_map(data), data, parent_id);
    Dialog::Task {
        mode: TaskMode::New { parent_id: parent_id.to_owned() },
        form: TaskForm::new(),
        opened: None,
        error: None,
        inherited,
        prereq_filter: String::new(),
    }
}

/// Ids of the tasks with a live completion.
pub fn completed_ids(data: &AppData) -> HashSet<String> {
    done(data).completed
}

/// Visible leaves with their path ("Area › Branch › Task"), in tree order;
/// only those of `area_id` when given.
pub fn leaf_options(data: &AppData, area_id: Option<&str>) -> Vec<(String, String)> {
    fn walk(nodes: &[TaskNode], path: &mut Vec<String>, out: &mut Vec<(String, String)>) {
        for n in live_children(nodes) {
            path.push(n.name.clone());
            if has_live_children(n) {
                walk(&n.children, path, out);
            } else {
                out.push((n.id.clone(), path.join(" › ")));
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    for a in live_areas(data).filter(|a| area_id.is_none_or(|id| a.id == id)) {
        let mut path = vec![a.name.clone()];
        walk(&a.children, &mut path, &mut out);
    }
    out
}

fn to_local(iso: &str) -> Option<chrono::DateTime<chrono::Local>> {
    chrono::DateTime::parse_from_rfc3339(iso).ok().map(|d| d.with_timezone(&chrono::Local))
}

/// An ISO date-time of the document, in local time with `fmt`.
pub fn local_fmt(iso: &str, fmt: &str) -> String {
    to_local(iso).map_or_else(|| iso.get(..10).unwrap_or(iso).to_owned(), |d| d.format(fmt).to_string())
}

/// An ISO date-time as an editable local "YYYY-MM-DD HH:MM".
pub fn local_input(iso: &str) -> String {
    local_fmt(iso, "%Y-%m-%d %H:%M")
}

/// A local "YYYY-MM-DD[ HH:MM]" typed by the user, as an ISO date-time with
/// its offset; a date alone is noon, which no time zone moves to another day.
pub fn local_input_to_iso(raw: &str) -> Option<String> {
    use chrono::TimeZone;
    let stored = crate::forms::parse_date(raw, "12:00").ok().flatten()?;
    let naive = chrono::NaiveDateTime::parse_from_str(&stored, "%Y-%m-%dT%H:%M").ok()?;
    chrono::Local.from_local_datetime(&naive).earliest().map(|d| d.to_rfc3339())
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

/// What is being dragged in the Tree.
#[derive(Debug, Clone, PartialEq)]
enum Drag {
    Task(String),
    Area(String),
}

/// A dragged item over `rect`: what it is and whether it would land before
/// the row (upper half) or after it. Draws the line where it would land.
fn drop_on(ui: &egui::Ui, rect: egui::Rect, accept: impl Fn(&Drag) -> bool) -> Option<(Drag, bool, bool)> {
    let payload = egui::DragAndDrop::payload::<Drag>(ui.ctx())?;
    let pos = ui.ctx().pointer_interact_pos()?;
    if !rect.contains(pos) || !accept(&payload) {
        return None;
    }
    let before = pos.y < rect.center().y;
    let y = if before { rect.top() } else { rect.bottom() };
    ui.painter().hline(rect.x_range(), y, egui::Stroke::new(2.0, ui.visuals().selection.stroke.color));
    let released = ui.input(|i| i.pointer.any_released());
    if released {
        egui::DragAndDrop::clear_payload(ui.ctx());
    }
    Some(((*payload).clone(), before, released))
}

/// The sibling a dropped item goes before: this row, or the next one that is
/// not the dragged item itself (none: the end of the list).
fn before_id(ids: &[&str], i: usize, upper: bool, dragged: &str) -> Option<String> {
    let rest = if upper { ids.get(i..) } else { ids.get(i + 1..) };
    rest.unwrap_or_default().iter().find(|x| **x != dragged).map(|x| (*x).to_owned())
}

fn task_drop(ui: &egui::Ui, state: &mut UiState, node: &TaskNode, parent_id: &str, ids: &[&str], i: usize) {
    let rect = ui.min_rect();
    let accept = |d: &Drag| matches!(d, Drag::Task(id) if *id != node.id);
    if let Some((Drag::Task(id), upper, true)) = drop_on(ui, rect, accept) {
        let before = before_id(ids, i, upper, &id);
        state.actions.push(Action::Run(Command::MoveTask { id, new_parent_id: parent_id.to_owned(), before }));
    }
}

fn node_label(ui: &mut egui::Ui, t: &TreeCtx<'_>, node: &TaskNode) {
    let s = t.s;
    let completed = t.done.completed.contains(&node.id) && !has_live_children(node);
    let mut name = RichText::new(&node.name);
    if completed {
        name = name.strikethrough().weak();
    }
    let id = ui.make_persistent_id(("drag", &node.id));
    ui.dnd_drag_source(id, Drag::Task(node.id.clone()), |ui| ui.label(name));
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
                    task_drop(ui, state, node, parent_id, &ids, i);
                })
                .body(|ui| tree_nodes(ui, t, state, &node.id, &node.children));
        } else {
            ui.horizontal(|ui| {
                ui.add_space(ui.spacing().indent);
                node_label(ui, t, node);
                node_menu(ui, t, state, node, parent_id, &ids, i);
                task_drop(ui, state, node, parent_id, &ids, i);
            });
        }
    }
}

/// On an area name: a task goes to the top of the area, an area changes place.
fn area_drop(ui: &egui::Ui, state: &mut UiState, area: &Area, ids: &[&str], i: usize) {
    let rect = ui.min_rect();
    let accept = |d: &Drag| !matches!(d, Drag::Area(id) if *id == area.id);
    match drop_on(ui, rect, accept) {
        Some((Drag::Task(id), _, true)) => {
            let first = live_children(&area.children).map(|n| n.id.as_str()).find(|x| *x != id).map(str::to_owned);
            state.actions.push(Action::Run(Command::MoveTask { id, new_parent_id: area.id.clone(), before: first }));
        }
        Some((Drag::Area(id), upper, true)) => {
            let before = before_id(ids, i, upper, &id);
            let mut order: Vec<String> = ids.iter().filter(|x| **x != id).map(|x| (*x).to_owned()).collect();
            let at = before.and_then(|b| order.iter().position(|x| *x == b)).unwrap_or(order.len());
            order.insert(at, id);
            state.actions.push(Action::Run(Command::ReorderAreas { order }));
        }
        _ => {}
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
    ui.label(RichText::new(s.drag_hint).small().weak());
    ui.add_space(8.0);
    let areas: Vec<&Area> = live_areas(data).collect();
    let ids: Vec<&str> = areas.iter().map(|a| a.id.as_str()).collect();
    for (i, area) in areas.iter().enumerate() {
        let id = ui.make_persistent_id(("area", &area.id));
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
            .show_header(ui, |ui| {
                swatch(ui, area_color(&area.color));
                let drag = ui.make_persistent_id(("drag-area", &area.id));
                ui.dnd_drag_source(drag, Drag::Area(area.id.clone()), |ui| {
                    ui.label(RichText::new(&area.name).strong())
                });
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
                area_drop(ui, state, area, &ids, i);
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

        ui.label(s.theme);
        let mut theme = state.theme;
        let themes = [(Theme::System, s.theme_system), (Theme::Light, s.theme_light), (Theme::Dark, s.theme_dark)];
        let current = themes.iter().find(|(t, _)| *t == theme).map_or(s.theme_system, |(_, name)| name);
        egui::ComboBox::from_id_salt("theme").selected_text(current).show_ui(ui, |ui| {
            for (t, name) in themes {
                ui.selectable_value(&mut theme, t, name);
            }
        });
        if theme != state.theme {
            state.actions.push(Action::SetTheme(theme));
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

fn silence_text(silence: Option<i64>, s: &Strings) -> String {
    match silence {
        None => s.never.to_owned(),
        Some(0) => s.silence_today.to_owned(),
        Some(1) => format!("1 {}", s.silence_day),
        Some(d) => format!("{d} {}", s.silence_days),
    }
}

fn severity_color(ui: &egui::Ui, sev: Severity) -> Color32 {
    match sev {
        Severity::Danger => ui.visuals().error_fg_color,
        Severity::Warning => ui.visuals().warn_fg_color,
        Severity::Normal => ui.visuals().weak_text_color(),
    }
}

/// Areas: one card per area, the most neglected first.
pub fn areas(ui: &mut egui::Ui, data: &AppData, state: &mut UiState) {
    let s = state.s();
    let log = data.area_actions.as_deref().unwrap_or_default();
    let now = chrono::Utc::now().timestamp_millis();
    let rows = portfolio::rows(&data.areas, log, now);
    if rows.is_empty() {
        ui.label(RichText::new(s.no_areas).weak());
        return;
    }
    for row in rows {
        let frame = egui::Frame::group(ui.style()).inner_margin(10.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                swatch(ui, area_color(&row.area.color));
                ui.label(RichText::new(&row.area.name).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let color = severity_color(ui, portfolio::severity(row.silence));
                    ui.label(RichText::new(silence_text(row.silence, s)).small().color(color));
                });
            });
            match row.area.status.as_deref().filter(|t| !t.is_empty()) {
                Some(t) => ui.label(t),
                None => ui.label(RichText::new(s.no_status).weak().italics()),
            };
            let last = row
                .last
                .map_or_else(|| s.never.to_owned(), |e| format!("{} · {}", e.text, local_fmt(&e.at, s.date_fmt)));
            ui.label(RichText::new(format!("{} {last}", s.last_action)).small().weak());
            match row.area.next_action_text.as_deref().filter(|t| !t.is_empty()) {
                Some(t) => ui.label(RichText::new(format!("{} {t}", s.next_prefix)).small()),
                None => ui.label(RichText::new(s.no_next).small().weak()),
            };
        });
        let r = frame.response.interact(egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if r.clicked() {
            state.actions.push(Action::Open(Dialog::Pulse(Box::new(Pulse::open(row.area)))));
        }
        ui.add_space(6.0);
    }
}

fn area_badge(ui: &mut egui::Ui, data: &AppData, area_id: &str) {
    if let Some(a) = live_areas(data).find(|a| a.id == area_id) {
        // Drawn right to left: the swatch ends up before the name.
        ui.label(RichText::new(&a.name).small());
        swatch(ui, area_color(&a.color));
    }
}

/// The path of a task above it, without the task: "Area › Branch".
fn parent_path(data: &AppData, task_id: &str) -> Option<String> {
    let mut path = path_names(&data.areas, task_id)?;
    path.pop();
    Some(path.join(" › "))
}

/// Tasks on hold, with the reason and the day they were put on hold.
pub fn waiting(ui: &mut egui::Ui, data: &AppData, state: &mut UiState) {
    let s = state.s();
    let items: Vec<&HistoryEntry> =
        data.history.iter().filter(|h| alive(h.meta.as_ref()) && h.waiting.unwrap_or(false)).collect();
    if items.is_empty() {
        ui.label(RichText::new(s.nothing_waiting).weak());
        return;
    }
    for h in items {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                if let Some(p) = parent_path(data, &h.task_id) {
                    ui.label(RichText::new(p).small().weak());
                }
                ui.label(RichText::new(&h.name).strong());
                if let Some(r) = h.waiting_for.as_deref().filter(|r| !r.is_empty()) {
                    ui.label(format!("{} {r}", s.waiting_for));
                }
                if let Some(at) = h.created_at.as_deref() {
                    ui.label(RichText::new(format!("{} {}", s.since, local_fmt(at, s.date_fmt))).small().weak());
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if find_visible_task(&data.areas, &h.task_id).is_some() && ui.button(s.complete).clicked() {
                    state.actions.push(Action::Run(Command::CompleteTask {
                        id: h.task_id.clone(),
                        entry_id: new_id(),
                        action_id: new_id(),
                    }));
                    state.actions.push(Action::Completed { task_id: h.task_id.clone(), name: h.name.clone() });
                }
                if ui.button(s.resume).clicked() {
                    state.actions.push(Action::Run(Command::Resume { id: h.task_id.clone() }));
                }
                area_badge(ui, data, &h.area_id);
            });
        });
        ui.separator();
    }
}

/// Completed tasks, the most recent first.
pub fn history(ui: &mut egui::Ui, data: &AppData, state: &mut UiState) {
    let s = state.s();
    let mut items: Vec<&HistoryEntry> =
        data.history.iter().filter(|h| alive(h.meta.as_ref()) && !h.waiting.unwrap_or(false)).collect();
    items.sort_by(|a, b| b.completed_at.cmp(&a.completed_at));
    ui.label(RichText::new(format!("{} {}", items.len(), s.completed_count)).weak());
    ui.add_space(6.0);
    if items.is_empty() {
        ui.label(RichText::new(s.nothing_done).weak());
        return;
    }
    for h in items {
        ui.horizontal(|ui| {
            let date = h.completed_at.as_deref().map(|d| local_fmt(d, s.date_fmt)).unwrap_or_default();
            ui.label(RichText::new(date).small().weak().monospace());
            ui.vertical(|ui| {
                ui.label(&h.name);
                match parent_path(data, &h.task_id) {
                    Some(p) => ui.label(RichText::new(p).small().weak()),
                    None => ui.label(RichText::new(s.deleted_task).small().weak().italics()),
                };
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(s.uncomplete).clicked() {
                    state.actions.push(Action::Run(Command::Uncomplete { id: h.task_id.clone() }));
                }
                area_badge(ui, data, &h.area_id);
            });
        });
        ui.separator();
    }
}
