//! Keyboard focus across the window: F6 and Shift+F6 move between the menu
//! bar, the page and the status bar; a dialog takes the focus when it opens
//! and gives it back to where it was when it closes.

use egui::{Context, FocusDirection, Id, Key, Modifiers, Rect, Ui};

/// A part of the window that F6 moves to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Region {
    Menu,
    Page,
    Status,
}

#[derive(Default, PartialEq, Eq, Debug)]
enum DialogStep {
    #[default]
    Closed,
    /// Opened in this pass: remember the focus and let it go.
    Opened,
    /// Shown once: the dialog's widgets now take the focus.
    Entering,
    Open,
    /// Closed in the last pass. The window behind still refused input then,
    /// so the focus goes back in the first pass without the dialog.
    Closing,
}

#[derive(Default)]
pub struct Focus {
    /// Where each region was drawn in the last pass.
    rects: Vec<(Region, Rect)>,
    seen: Vec<(Region, Rect)>,
    /// The region that takes the focus when it is drawn in this pass.
    pending: Option<Region>,
    /// The region F6 last moved to: where F6 goes on from when that region
    /// had nothing to focus.
    last: Option<Region>,
    /// Where the focus was when the dialog opened.
    back: Option<Id>,
    dialog: DialogStep,
}

impl Focus {
    /// At the start of a pass: reads F6 and Shift+F6. `regions` are the ones
    /// with something to focus, in the order they appear.
    pub fn begin_pass(&mut self, ctx: &Context, regions: &[Region]) {
        self.rects = std::mem::take(&mut self.seen);
        if self.dialog != DialogStep::Closed || regions.is_empty() {
            return;
        }
        // Shift first: a key without Shift also matches the key with it.
        let prev = ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::F6));
        let next = !prev && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F6));
        if !next && !prev {
            return;
        }
        let focused = ctx.memory(|m| m.focused());
        let here = match focused {
            Some(id) => ctx.read_response(id).and_then(|r| {
                self.rects.iter().find(|(_, rect)| rect.contains(r.rect.center())).map(|(region, _)| *region)
            }),
            // A region with nothing to focus (an empty list) still counts as
            // where the user is, so the next F6 does not start over.
            None => self.last,
        };
        let n = regions.len();
        let at = here.and_then(|h| regions.iter().position(|r| *r == h));
        let to = match (at, next) {
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
            (None, true) => 0,
            (None, false) => n - 1,
        };
        self.pending = regions.get(to).copied();
        self.last = self.pending;
        ctx.request_repaint();
    }

    /// Call first thing inside a region's contents.
    pub fn enter(&mut self, ui: &Ui, region: Region) {
        if self.pending == Some(region) {
            self.pending = None;
            give_to_next(ui.ctx());
        }
    }

    /// Call with the region's rectangle once it is drawn.
    pub fn drawn(&mut self, region: Region, rect: Rect) {
        self.seen.push((region, rect));
    }

    /// A dialog was put on screen.
    pub fn dialog_opened(&mut self) {
        self.dialog = DialogStep::Opened;
    }

    /// Call before drawing the open dialog's contents.
    pub fn before_dialog(&mut self, ctx: &Context) {
        match self.dialog {
            DialogStep::Closed | DialogStep::Opened | DialogStep::Closing => {
                // A dialog that opens from another keeps the first one's place.
                if self.back.is_none() {
                    self.back = ctx.memory(|m| m.focused());
                }
                if let Some(id) = ctx.memory(|m| m.focused()) {
                    ctx.memory_mut(|m| m.surrender_focus(id));
                }
                self.dialog = DialogStep::Entering;
                ctx.request_repaint();
            }
            DialogStep::Entering => {
                // A widget that asked for the focus itself keeps it.
                if ctx.memory(|m| m.focused()).is_none() {
                    give_to_next(ctx);
                }
                self.dialog = DialogStep::Open;
            }
            DialogStep::Open => {}
        }
    }

    /// Call in a pass with no dialog on screen, after the page is drawn.
    pub fn no_dialog(&mut self, ctx: &Context) {
        match self.dialog {
            DialogStep::Closed => {}
            DialogStep::Closing => {
                match self.back.take() {
                    // A widget still on screen, such as a row's button, gets it back.
                    Some(id) if ctx.read_response(id).is_some() => ctx.memory_mut(|m| m.request_focus(id)),
                    // A menu item that closed with its menu: the page takes it.
                    Some(_) => {
                        self.pending = Some(Region::Page);
                        ctx.request_repaint();
                    }
                    None => {}
                }
                self.dialog = DialogStep::Closed;
            }
            // Taken away without closing (the document was closed).
            _ => {
                self.back = None;
                self.dialog = DialogStep::Closed;
            }
        }
    }

    /// The dialog closed in this pass.
    pub fn dialog_closed(&mut self, ctx: &Context) {
        self.dialog = DialogStep::Closing;
        ctx.request_repaint();
    }
}

/// Drops the current focus, so the next widget drawn in this pass that can
/// take it gets it.
fn give_to_next(ctx: &Context) {
    ctx.memory_mut(|m| {
        if let Some(id) = m.focused() {
            m.surrender_focus(id);
        }
        m.move_focus(FocusDirection::Next);
    });
}

#[cfg(test)]
mod tests {
    use super::{Focus, Region};
    use egui::Key;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[derive(Default)]
    struct Window {
        focus: Focus,
        dialog: bool,
        /// The page shows only text, as an empty list does.
        empty: bool,
    }

    /// Three regions laid out as in the app, and a dialog opened by a button
    /// on the page.
    fn draw(ui: &mut egui::Ui, w: &mut Window) {
        let ctx = ui.ctx().clone();
        w.focus.begin_pass(&ctx, &[Region::Menu, Region::Page, Region::Status]);
        let menu = egui::Panel::top("menu").show(ui, |ui| {
            w.focus.enter(ui, Region::Menu);
            let _ = ui.button("File");
        });
        w.focus.drawn(Region::Menu, menu.response.rect);
        let status = egui::Panel::bottom("status").show(ui, |ui| {
            w.focus.enter(ui, Region::Status);
            let _ = ui.button("Undo");
        });
        w.focus.drawn(Region::Status, status.response.rect);
        let page = egui::CentralPanel::default().show(ui, |ui| {
            w.focus.enter(ui, Region::Page);
            if w.empty {
                ui.label("Nothing here");
                return;
            }
            if ui.button("Edit").clicked() {
                w.dialog = true;
                w.focus.dialog_opened();
            }
            let _ = ui.button("Delete");
        });
        w.focus.drawn(Region::Page, page.response.rect);
        if w.dialog {
            w.focus.before_dialog(&ctx);
            let r = egui::Modal::new(egui::Id::unique("dialog")).show(&ctx, |ui| {
                let _ = ui.button("Save");
            });
            if r.should_close() {
                w.dialog = false;
                w.focus.dialog_closed(&ctx);
            }
        } else {
            w.focus.no_dialog(&ctx);
        }
    }

    fn focused(h: &Harness<'_, Window>, label: &str) -> bool {
        h.query_by(|n| n.is_focused() && n.label().is_some_and(|l| l == label)).is_some()
    }

    fn press(h: &mut Harness<'_, Window>, key: Key, shift: bool) {
        let m = if shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE };
        h.key_press_modifiers(m, key);
        h.run();
    }

    #[test]
    fn f6_moves_between_regions_both_ways() {
        let mut h = Harness::new_ui_state(draw, Window::default());
        h.run();
        press(&mut h, Key::F6, false);
        assert!(focused(&h, "File"));
        press(&mut h, Key::F6, false);
        assert!(focused(&h, "Edit"));
        press(&mut h, Key::F6, false);
        assert!(focused(&h, "Undo"));
        press(&mut h, Key::F6, false);
        assert!(focused(&h, "File"));
        press(&mut h, Key::F6, true);
        assert!(focused(&h, "Undo"));
    }

    #[test]
    fn f6_goes_past_a_page_with_nothing_to_focus() {
        let mut h = Harness::new_ui_state(draw, Window { empty: true, ..Window::default() });
        h.run();
        press(&mut h, Key::F6, false);
        assert!(focused(&h, "File"));
        press(&mut h, Key::F6, false);
        press(&mut h, Key::F6, false);
        assert!(focused(&h, "Undo"));
    }

    #[test]
    fn a_dialog_takes_the_focus_and_gives_it_back() {
        let mut h = Harness::new_ui_state(draw, Window::default());
        h.run();
        press(&mut h, Key::F6, false);
        press(&mut h, Key::F6, false);
        assert!(focused(&h, "Edit"));
        press(&mut h, Key::Enter, false);
        assert!(h.state().dialog);
        assert!(focused(&h, "Save"));
        press(&mut h, Key::Escape, false);
        assert!(!h.state().dialog);
        assert!(focused(&h, "Edit"));
    }
}
