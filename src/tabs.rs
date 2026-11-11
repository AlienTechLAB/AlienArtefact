//! Several diagrams in one project, one tab each.
//!
//! The active tab's state lives in `App`'s own fields (`diagram`, `selection`, undo history, view,
//! ...), so the rest of the app works on "the diagram" as before. The other tabs keep theirs in a
//! `TabState`; switching tabs swaps the two.

use std::collections::BTreeSet;

use egui::{vec2, Key, Ui, Vec2};

use crate::app::App;
use crate::canvas::{Drag, LinkError};
use crate::model::{Diagram, Id, NamedDiagram};

/// Everything that belongs to one diagram rather than to the app.
pub struct TabState {
    pub diagram: Diagram,
    pub selection: BTreeSet<Id>,
    pub pan: Vec2,
    pub zoom: f32,
    pub drag: Drag,
    pub editing: Option<Id>,
    pub edit_focus: bool,
    pub undo: Vec<Diagram>,
    pub redo: Vec<Diagram>,
    pub stable: Diagram,
    pub saved: Diagram,
    pub link_error: Option<LinkError>,
    pub fit_pending: bool,
}

impl TabState {
    /// A tab showing `diagram`, which counts as saved.
    pub fn new(diagram: Diagram, fit: bool) -> Self {
        Self {
            stable: diagram.clone(),
            saved: diagram.clone(),
            diagram,
            selection: BTreeSet::new(),
            pan: vec2(40.0, 40.0),
            zoom: 1.0,
            drag: Drag::None,
            editing: None,
            edit_focus: false,
            undo: Vec::new(),
            redo: Vec::new(),
            link_error: None,
            fit_pending: fit,
        }
    }
}

pub struct Tab {
    pub name: String,
    /// `None` for the active tab: its state is in `App`'s fields.
    pub state: Option<TabState>,
}

enum TabAction {
    Switch(usize),
    New,
    Close(usize),
    Duplicate(usize),
    Move(usize, isize),
    StartRename(usize),
}

impl App {
    /// Takes the active tab's state out of the app's fields.
    fn stash(&mut self) -> TabState {
        TabState {
            diagram: std::mem::take(&mut self.diagram),
            selection: std::mem::take(&mut self.selection),
            pan: self.pan,
            zoom: self.zoom,
            drag: std::mem::replace(&mut self.drag, Drag::None),
            editing: self.editing.take(),
            edit_focus: std::mem::take(&mut self.edit_focus),
            undo: std::mem::take(&mut self.undo),
            redo: std::mem::take(&mut self.redo),
            stable: std::mem::take(&mut self.stable),
            saved: std::mem::take(&mut self.saved),
            link_error: self.link_error.take(),
            fit_pending: std::mem::take(&mut self.fit_pending),
        }
    }

    /// Puts a tab's state into the app's fields, making it the one shown and edited.
    fn restore(&mut self, s: TabState) {
        self.diagram = s.diagram;
        self.selection = s.selection;
        self.pan = s.pan;
        self.zoom = s.zoom;
        self.drag = s.drag;
        self.editing = s.editing;
        self.edit_focus = s.edit_focus;
        self.undo = s.undo;
        self.redo = s.redo;
        self.stable = s.stable;
        self.saved = s.saved;
        self.link_error = s.link_error;
        self.fit_pending = s.fit_pending;
    }

    pub fn switch_tab(&mut self, i: usize) {
        if i == self.active || i >= self.tabs.len() || self.link_error.is_some() {
            return;
        }
        // Whatever was being edited becomes an undo step of the tab it was done in.
        self.force_commit();
        self.editing = None;
        let current = self.stash();
        self.tabs[self.active].state = Some(current);
        let next = self.tabs[i].state.take().expect("inactive tabs hold their state");
        self.restore(next);
        self.active = i;
        self.palette_drag = None;
    }

    fn unique_tab_name(&self, base: &str) -> String {
        let taken = |n: &str| self.tabs.iter().any(|t| t.name == n);
        if !taken(base) {
            return base.to_owned();
        }
        (2..).map(|k| format!("{base} {k}")).find(|n| !taken(n)).unwrap()
    }

    pub fn new_tab(&mut self) {
        let name = self.unique_tab_name(&format!("Diagram {}", self.tabs.len() + 1));
        self.tabs.push(Tab { name, state: Some(TabState::new(Diagram::default(), false)) });
        let last = self.tabs.len() - 1;
        self.switch_tab(last);
        self.status = format!("New diagram \"{}\"", self.tabs[last].name);
    }

    fn tab_diagram(&self, i: usize) -> &Diagram {
        if i == self.active { &self.diagram } else { &self.tabs[i].state.as_ref().unwrap().diagram }
    }

    pub fn close_tab(&mut self, i: usize) {
        if self.tabs.len() <= 1 || i >= self.tabs.len() {
            return;
        }
        let d = self.tab_diagram(i);
        if !d.nodes.is_empty() || !d.edges.is_empty() {
            let answer = rfd::MessageDialog::new()
                .set_title("Close diagram")
                .set_description(format!(
                    "Close \"{}\"? Its contents are removed from the project (this cannot be undone).",
                    self.tabs[i].name
                ))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();
            if answer != rfd::MessageDialogResult::Yes {
                return;
            }
        }
        if i == self.active {
            // Show a neighbour first, then drop this one.
            let neighbour = if i + 1 < self.tabs.len() { i + 1 } else { i - 1 };
            self.switch_tab(neighbour);
        }
        let name = self.tabs.remove(i).name;
        if i < self.active {
            self.active -= 1;
        }
        self.status = format!("Closed \"{name}\"");
    }

    fn duplicate_tab(&mut self, i: usize) {
        let diagram = self.tab_diagram(i).clone();
        let name = self.unique_tab_name(&format!("{} copy", self.tabs[i].name));
        let mut state = TabState::new(diagram, true);
        // A copy has never been saved.
        state.saved = Diagram::default();
        self.tabs.insert(i + 1, Tab { name, state: Some(state) });
        if i < self.active {
            self.active += 1;
        }
        self.switch_tab(i + 1);
    }

    fn move_tab(&mut self, i: usize, by: isize) {
        let j = i as isize + by;
        if j < 0 || j as usize >= self.tabs.len() {
            return;
        }
        let j = j as usize;
        self.tabs.swap(i, j);
        if self.active == i {
            self.active = j;
        } else if self.active == j {
            self.active = i;
        }
    }

    /// Replaces the whole project.
    pub fn reset_project(&mut self, diagrams: Vec<NamedDiagram>, active: usize, path: Option<std::path::PathBuf>) {
        let diagrams = if diagrams.is_empty() {
            vec![NamedDiagram { name: "Diagram 1".into(), diagram: Diagram::default() }]
        } else {
            diagrams
        };
        let active = active.min(diagrams.len() - 1);
        self.tabs = diagrams
            .into_iter()
            .map(|d| Tab { name: d.name, state: Some(TabState::new(d.diagram, true)) })
            .collect();
        let first = self.tabs[active].state.take().unwrap();
        self.restore(first);
        self.active = active;
        self.saved_names = self.tabs.iter().map(|t| t.name.clone()).collect();
        self.renaming = None;
        self.path = path;
    }

    /// Every diagram of the project, with the active one's current contents.
    pub fn project_diagrams(&self) -> Vec<NamedDiagram> {
        (0..self.tabs.len())
            .map(|i| NamedDiagram { name: self.tabs[i].name.clone(), diagram: self.tab_diagram(i).clone() })
            .collect()
    }

    /// Unsaved changes in any tab, or tabs added, removed, renamed or reordered.
    pub fn project_dirty(&self) -> bool {
        self.diagram != self.saved
            || self.tabs.iter().filter_map(|t| t.state.as_ref()).any(|s| s.diagram != s.saved)
            || self.tabs.iter().map(|t| &t.name).ne(self.saved_names.iter())
    }

    /// After saving: every tab's current contents count as saved.
    pub fn mark_project_saved(&mut self) {
        self.saved = self.diagram.clone();
        for s in self.tabs.iter_mut().filter_map(|t| t.state.as_mut()) {
            s.saved = s.diagram.clone();
        }
        self.saved_names = self.tabs.iter().map(|t| t.name.clone()).collect();
    }

    pub fn active_tab_name(&self) -> &str {
        &self.tabs[self.active].name
    }

    // ---------------------------------------------------------------- the tab bar

    pub fn tab_bar(&mut self, ui: &mut Ui) {
        let mut action: Option<TabAction> = None;
        let can_close = self.tabs.len() > 1;
        ui.horizontal(|ui| {
            for i in 0..self.tabs.len() {
                let dirty = if i == self.active {
                    self.diagram != self.saved
                } else {
                    self.tabs[i].state.as_ref().is_some_and(|s| s.diagram != s.saved)
                };

                if let Some((at, text)) = &mut self.renaming {
                    if *at == i {
                        let r = ui.add(egui::TextEdit::singleline(text).desired_width(140.0));
                        if self.rename_focus {
                            r.request_focus();
                            self.rename_focus = false;
                        }
                        let done = r.lost_focus() || ui.input(|inp| inp.key_pressed(Key::Enter));
                        let cancel = ui.input(|inp| inp.key_pressed(Key::Escape));
                        if cancel {
                            self.renaming = None;
                        } else if done {
                            let name = text.trim().to_owned();
                            if !name.is_empty() {
                                self.tabs[i].name = name;
                            }
                            self.renaming = None;
                        }
                        ui.separator();
                        continue;
                    }
                }

                let label = format!("{}{}", self.tabs[i].name, if dirty { " *" } else { "" });
                let resp = ui
                    .selectable_label(i == self.active, label)
                    .on_hover_text("Click to show, double-click to rename, right-click for more");
                if resp.double_clicked() {
                    action = Some(TabAction::StartRename(i));
                } else if resp.clicked() {
                    action = Some(TabAction::Switch(i));
                }
                resp.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        action = Some(TabAction::StartRename(i));
                    }
                    if ui.button("Duplicate").clicked() {
                        action = Some(TabAction::Duplicate(i));
                    }
                    if ui.add_enabled(i > 0, egui::Button::new("Move left")).clicked() {
                        action = Some(TabAction::Move(i, -1));
                    }
                    if ui.add_enabled(i + 1 < self.tabs.len(), egui::Button::new("Move right")).clicked() {
                        action = Some(TabAction::Move(i, 1));
                    }
                    ui.separator();
                    if ui.add_enabled(can_close, egui::Button::new("Close")).clicked() {
                        action = Some(TabAction::Close(i));
                    }
                });
                if can_close
                    && ui
                        .add(egui::Button::new("\u{d7}").small().frame(false))
                        .on_hover_text("Close this diagram (Ctrl+W)")
                        .clicked()
                {
                    action = Some(TabAction::Close(i));
                }
                ui.separator();
            }
            if ui.button(" + ").on_hover_text("New diagram in this project (Ctrl+T)").clicked() {
                action = Some(TabAction::New);
            }
        });

        match action {
            Some(TabAction::Switch(i)) => self.switch_tab(i),
            Some(TabAction::New) => self.new_tab(),
            Some(TabAction::Close(i)) => self.close_tab(i),
            Some(TabAction::Duplicate(i)) => self.duplicate_tab(i),
            Some(TabAction::Move(i, by)) => self.move_tab(i, by),
            Some(TabAction::StartRename(i)) => {
                self.renaming = Some((i, self.tabs[i].name.clone()));
                self.rename_focus = true;
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShapeKind;
    use egui::pos2;

    fn app_with_tabs() -> App {
        let mut app = App::default();
        app.diagram.add_node(ShapeKind::Class, pos2(0.0, 0.0));
        app.new_tab();
        app.diagram.add_node(ShapeKind::Trait, pos2(0.0, 0.0));
        app.diagram.add_node(ShapeKind::Trait, pos2(300.0, 0.0));
        app
    }

    #[test]
    fn each_tab_keeps_its_own_diagram_and_history() {
        let mut app = app_with_tabs();
        assert_eq!((app.tabs.len(), app.active), (2, 1));
        assert_eq!(app.diagram.nodes.len(), 2);

        app.switch_tab(0);
        assert_eq!(app.diagram.nodes.len(), 1);
        assert_eq!(app.diagram.nodes[0].kind, ShapeKind::Class);

        app.switch_tab(1);
        assert_eq!(app.diagram.nodes.len(), 2);
        // Undo works within the tab: the second trait was the last change made here.
        app.undo();
        assert!(app.diagram.nodes.len() < 2);
    }

    #[test]
    fn project_round_trip_and_dirty_tracking() {
        let mut app = app_with_tabs();
        assert!(app.project_dirty());
        let diagrams = app.project_diagrams();
        assert_eq!(diagrams.iter().map(|d| d.diagram.nodes.len()).collect::<Vec<_>>(), vec![1, 2]);

        app.mark_project_saved();
        assert!(!app.project_dirty());
        app.tabs[0].name = "Renamed".into();
        assert!(app.project_dirty(), "renaming a tab is a change");

        app.reset_project(diagrams, 0, None);
        assert_eq!((app.tabs.len(), app.active, app.diagram.nodes.len()), (2, 0, 1));
        assert!(!app.project_dirty());
    }

    #[test]
    fn closing_and_moving_tabs_keeps_the_right_one_active() {
        let mut app = app_with_tabs();
        app.new_tab(); // three tabs, third active
        app.switch_tab(1);
        app.move_tab(1, -1);
        assert_eq!(app.active, 0, "the active tab moved left");
        assert_eq!(app.diagram.nodes.len(), 2);

        app.close_tab(2); // empty tab: no question asked
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.diagram.nodes.len(), 2, "still showing the same diagram");

        app.duplicate_tab(0);
        assert_eq!((app.tabs.len(), app.active), (3, 1));
        assert_eq!(app.diagram.nodes.len(), 2);
        assert!(app.tabs[1].name.ends_with("copy"));
    }
}
