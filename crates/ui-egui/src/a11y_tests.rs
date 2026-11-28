//! Every view and every dialog, in both languages and both themes, under
//! the accessibility check of `egui_kittest`: each control has a name, so
//! a screen reader can say what it is. It runs with the tests on every CI
//! platform.

use crate::app::{Theme, UiState, View};
use crate::dialogs::{self, Dialog, EntryEdit, NextEdit, Pulse, Target};
use crate::focus::Focus;
use crate::forms::AreaForm;
use crate::i18n::Lang;
use crate::views;
use balancecraft_model::{AppData, TaskNode};
use egui_kittest::Harness;
use std::path::PathBuf;

fn data() -> AppData {
    let json = r##"{"areas":[
        {"id":"a1","name":"Home","color":"#3366AA","status":"Fine","nextActionText":"Call the plumber","children":[
            {"id":"t1","name":"Paint the door","dueDate":"2026-01-10","children":[
                {"id":"t3","name":"Buy paint","children":[]}
            ]},
            {"id":"t4","name":"Fix the tap","children":[]}
        ]},
        {"id":"a2","name":"Health","color":"#AA3366","children":[{"id":"t2","name":"Book a visit","children":[]}]}
    ],
    "history":[
        {"id":"h1","taskId":"t4","areaId":"a1","name":"Fix the tap","waiting":true,"waitingFor":"the plumber","createdAt":"2026-01-02T10:00:00Z"},
        {"id":"h2","taskId":"t9","areaId":"a2","name":"Run","completedAt":"2026-01-03T10:00:00Z"}
    ],
    "areaActions":[{"id":"x1","areaId":"a1","text":"Called","at":"2026-01-04T10:00:00Z"}]
    }"##;
    serde_json::from_str(json).unwrap()
}

fn state(lang: Lang, theme: Theme, view: View) -> UiState {
    UiState {
        lang,
        theme,
        view,
        area_filter: None,
        dialog: None,
        toast: None,
        actions: Vec::new(),
        global_advance: None,
        focus: Focus::default(),
    }
}

fn task<'a>(data: &'a AppData, id: &str) -> &'a TaskNode {
    fn find<'a>(nodes: &'a [TaskNode], id: &str) -> Option<&'a TaskNode> {
        nodes.iter().find_map(|n| if n.id == id { Some(n) } else { find(&n.children, id) })
    }
    data.areas.iter().find_map(|a| find(&a.children, id)).unwrap()
}

fn dialogs_of(data: &AppData, s: &'static crate::i18n::Strings) -> Vec<Dialog> {
    let home = &data.areas[0];
    let form = AreaForm::from_area(home);
    vec![
        views::new_task_dialog(data, "a1"),
        views::edit_task_dialog(data, task(data, "t1"), s),
        Dialog::Area { id: None, form: AreaForm::new("#3366AA"), opened: None, error: None, inherited: 5 },
        Dialog::Area { id: Some("a1".into()), opened: form.validate(s).ok(), form, error: None, inherited: 5 },
        Dialog::Wait { id: "t2".into(), name: "Book a visit".into(), reason: String::new() },
        Dialog::Delete { target: Target::Task("t2".into()), name: "Book a visit".into() },
        Dialog::Move { id: "t2".into(), name: "Book a visit".into(), filter: String::new() },
        Dialog::Pulse(Box::new(Pulse::open(home))),
        Dialog::Pulse(Box::new(Pulse {
            next: Some(NextEdit { text: "Call".into(), task: None, opened_text: "Call".into(), opened_task: None }),
            entry: Some(EntryEdit {
                id: "x1".into(),
                text: "Called".into(),
                date: "04/01/2026".into(),
                opened_text: "Called".into(),
                opened_date: "04/01/2026".into(),
            }),
            ..Pulse::open(home)
        })),
        Dialog::Notice(s.target_gone),
    ]
}

fn draw(ui: &mut egui::Ui, (data, st): &mut (AppData, UiState)) {
    egui::CentralPanel::default().show(ui, |ui| match st.view {
        View::Today => views::today(ui, data, st),
        View::Tree => views::tree(ui, data, st),
        View::Areas => views::areas(ui, data, st),
        View::Waiting => views::waiting(ui, data, st),
        View::History => views::history(ui, data, st),
        View::Settings => views::settings(ui, data, st, "doc", &PathBuf::from("doc.balancecraft")),
    });
    let ctx = ui.ctx().clone();
    dialogs::show(&ctx, data, st);
}

/// Runs one screen; `run` panics when a control has no name.
fn check(case: String, st: UiState, data: AppData, failures: &mut Vec<String>) {
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut h = Harness::new_ui_state(draw, (data, st));
        h.run();
    }));
    if run.is_err() {
        failures.push(case);
    }
}

#[test]
fn every_view_and_dialog_names_its_controls() {
    let views = [View::Today, View::Tree, View::Areas, View::Waiting, View::History, View::Settings];
    let mut failures = Vec::new();
    for lang in [Lang::En, Lang::It] {
        for theme in [Theme::Light, Theme::Dark] {
            for view in views {
                check(format!("{lang:?} {theme:?} {view:?}"), state(lang, theme, view), data(), &mut failures);
            }
            let data = data();
            let s = state(lang, theme, View::Today).s();
            for dialog in dialogs_of(&data, s) {
                let case = format!(
                    "{lang:?} {theme:?} dialog {}",
                    format!("{dialog:?}").split([' ', '(']).next().unwrap_or("")
                );
                let mut st = state(lang, theme, View::Today);
                st.dialog = Some(dialog);
                check(case, st, data.clone(), &mut failures);
            }
        }
    }
    assert!(failures.is_empty(), "controls without a name in: {failures:#?}");
}
