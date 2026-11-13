//! The workspace: a project's tabs (diagrams, text notes, images) arranged by a docking layout.
//!
//! Tabs can be dragged into other tab groups, to any edge of a group (splitting it, as in VS Code),
//! or out into floating windows. The layout is saved with the project.
//!
//! Diagram tabs share the app's editing machinery. The **active** diagram (the one the format panel,
//! toolbar and shortcuts work on) keeps its state in `App`'s own fields (`diagram`, `selection`,
//! undo history, view, ...); every other diagram keeps its state in a `TabState`. Drawing another
//! visible diagram swaps its state in for the duration of the drawing; clicking into it keeps it
//! swapped in, making it the active one.

use std::collections::BTreeSet;

use egui::{vec2, Pos2, Rect, Ui, Vec2, WidgetText};
use egui_stack::tab_viewer::OnCloseResponse;
use egui_stack::{DockArea, NodePath, Style, TabPath, TabViewer};

use crate::app::App;
use crate::canvas::{Drag, LinkError};
use crate::model::{Diagram, Id, ItemContent, Layout, Node, Project, ProjectItem, ShapeKind};

pub type ItemId = u64;

/// Everything that belongs to one diagram rather than to the app.
pub struct TabState {
    pub diagram: Diagram,
    pub selection: BTreeSet<Id>,
    pub pan: Vec2,
    pub zoom: f32,
    pub canvas_rect: Rect,
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
            canvas_rect: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
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

/// What a tab holds, with what is needed to edit it.
pub enum Content {
    /// `None` while it is the active diagram: its state is in `App`'s fields.
    Diagram(Option<TabState>),
    Text { text: String, saved: String },
    Image { path: String, saved: String },
    /// The contents of a text file object (`node`) of diagram tab `diagram`. The text lives in the
    /// diagram, so its changes are the diagram's (undo, unsaved marker).
    FileText { diagram: ItemId, node: Id },
}

/// The kinds of tab that can be created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewKind {
    Diagram,
    Text,
    Image,
}

impl NewKind {
    fn base_name(self) -> &'static str {
        match self {
            NewKind::Diagram => "Diagram",
            NewKind::Text => "Notes",
            NewKind::Image => "Image",
        }
    }
}

pub struct Tab {
    pub id: ItemId,
    pub name: String,
    pub content: Content,
}

impl Tab {
    pub fn is_diagram(&self) -> bool {
        matches!(self.content, Content::Diagram(_))
    }
}

/// Changes to the tab list asked for while the dock is being drawn, applied after it.
pub enum Pending {
    Add(NewKind, Option<NodePath>),
    Closed(ItemId),
    Activate(ItemId),
    Duplicate(ItemId),
    Rename(ItemId),
}

impl App {
    // ---------------------------------------------------------------- the active diagram

    /// Takes the active diagram's state out of the app's fields.
    fn stash(&mut self) -> TabState {
        TabState {
            diagram: std::mem::take(&mut self.diagram),
            selection: std::mem::take(&mut self.selection),
            pan: self.pan,
            zoom: self.zoom,
            canvas_rect: self.canvas_rect,
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

    /// Puts a diagram's state into the app's fields.
    fn restore(&mut self, s: TabState) {
        self.diagram = s.diagram;
        self.selection = s.selection;
        self.pan = s.pan;
        self.zoom = s.zoom;
        self.canvas_rect = s.canvas_rect;
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

    pub fn index_of(&self, id: ItemId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// Makes diagram tab `i` the one in the app's fields, without any other effect.
    fn swap_active(&mut self, i: usize) {
        if self.active == Some(i) || !self.tabs[i].is_diagram() {
            return;
        }
        let current = self.stash();
        if let Some(a) = self.active {
            self.tabs[a].content = Content::Diagram(Some(current));
        }
        let Content::Diagram(next) = &mut self.tabs[i].content else { unreachable!() };
        let next = next.take().expect("inactive diagrams hold their state");
        self.restore(next);
        self.active = Some(i);
    }

    /// Puts the active diagram back into its tab, leaving no diagram active (the app's fields
    /// then hold an empty diagram that is not shown anywhere).
    fn detach_active(&mut self) {
        let Some(a) = self.active.take() else { return };
        let current = self.stash();
        self.tabs[a].content = Content::Diagram(Some(current));
        self.restore(TabState::new(Diagram::default(), true));
    }

    /// The id of the active diagram's tab, if there is a diagram.
    pub fn active_id(&self) -> Option<ItemId> {
        self.active.map(|a| self.tabs[a].id)
    }

    /// Makes diagram tab `i` the active one (the format panel, toolbar and shortcuts work on it).
    pub fn switch_tab(&mut self, i: usize) {
        if self.active == Some(i) || i >= self.tabs.len() || !self.tabs[i].is_diagram() || self.link_error.is_some() {
            return;
        }
        // Whatever was being edited becomes an undo step of the diagram it was done in.
        self.force_commit();
        self.editing = None;
        self.swap_active(i);
        self.palette_drag = None;
    }

    /// Draws diagram tab `i`. Another diagram than the active one is swapped in for the drawing,
    /// and stays active if the user pressed a mouse button on it.
    fn show_diagram(&mut self, i: usize, ui: &mut Ui) {
        let id = self.tabs[i].id;
        if self.active == Some(i) {
            self.canvas(ui);
        } else {
            let previous = self.active;
            self.swap_active(i);
            let pressed = self.canvas(ui);
            if pressed {
                // The previous diagram's last edits become its own undo step.
                if let Some(Content::Diagram(Some(prev))) = previous.map(|p| &mut self.tabs[p].content) {
                    if prev.diagram != prev.stable {
                        let before = std::mem::replace(&mut prev.stable, prev.diagram.clone());
                        prev.undo.push(before);
                        prev.redo.clear();
                    }
                    prev.editing = None;
                }
            } else {
                match previous {
                    Some(p) => self.swap_active(p),
                    None => self.detach_active(),
                }
            }
        }
        self.visible_diagrams.push((id, ui.min_rect()));
    }

    pub fn active_tab_name(&self) -> Option<&str> {
        self.active.map(|a| self.tabs[a].name.as_str())
    }

    /// The visible diagram tab under screen point `p` (as drawn last frame), the active one first.
    pub fn diagram_tab_at(&self, p: Pos2) -> Option<usize> {
        let active_id = self.active_id();
        let mut hits = self.last_visible_diagrams.iter().filter(|(_, r)| r.contains(p));
        let first = hits.clone().find(|(id, _)| Some(*id) == active_id).or_else(|| hits.next())?;
        self.index_of(first.0)
    }

    // ---------------------------------------------------------------- creating and removing tabs

    fn unique_name(&self, base: &str) -> String {
        let taken = |n: &str| self.tabs.iter().any(|t| t.name == n);
        (1..).map(|k| format!("{base} {k}")).find(|n| !taken(n)).unwrap()
    }

    fn alloc_item_id(&mut self) -> ItemId {
        let id = self.tabs.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        id.max(self.next_item_id)
    }

    /// Adds a new tab, in `group` (or the focused group), and shows it.
    pub fn add_tab(&mut self, kind: NewKind, group: Option<NodePath>) -> ItemId {
        let id = self.alloc_item_id();
        self.next_item_id = id + 1;
        let name = self.unique_name(kind.base_name());
        let content = match kind {
            NewKind::Diagram => Content::Diagram(Some(TabState::new(Diagram::default(), false))),
            NewKind::Text => Content::Text { text: String::new(), saved: String::new() },
            NewKind::Image => Content::Image { path: String::new(), saved: String::new() },
        };
        self.tabs.push(Tab { id, name: name.clone(), content });
        if let Some(path) = group {
            self.dock.set_focused_node_and_surface(path);
        }
        self.dock.push_to_focused_leaf(id);
        if kind == NewKind::Diagram {
            self.switch_tab(self.tabs.len() - 1);
        }
        self.status = format!("New {} \"{name}\"", kind.base_name().to_lowercase());
        id
    }

    /// Makes sure there is an active diagram and that its tab is the one shown in its group:
    /// when the project has no diagram, a new one is created.
    pub fn ensure_diagram(&mut self) {
        match self.active_id() {
            None => {
                self.add_tab(NewKind::Diagram, None);
            }
            Some(id) => {
                if let Some(path) = self.dock.find_tab(&id) {
                    let _ = self.dock.set_active_tab(path);
                }
            }
        }
    }

    /// A shape clicked in the palette: added to the middle of the active diagram's view (to a new
    /// diagram if there is none), a little apart from the previous ones.
    pub fn add_shape_from_palette(&mut self, kind: ShapeKind) {
        self.ensure_diagram();
        let n = self.diagram.nodes.len() as f32;
        let middle = if self.canvas_rect.is_positive() {
            self.to_world(self.canvas_rect.center())
        } else {
            // A diagram not drawn yet.
            Pos2::new(300.0, 200.0)
        };
        self.add_node_at(kind, middle + vec2(1.0, 1.0) * (n % 8.0) * 10.0);
    }

    pub fn new_tab(&mut self) {
        self.add_tab(NewKind::Diagram, None);
    }

    fn has_content(&self, i: usize) -> bool {
        match &self.tabs[i].content {
            Content::Diagram(Some(s)) => !s.diagram.nodes.is_empty() || !s.diagram.edges.is_empty(),
            Content::Diagram(None) => !self.diagram.nodes.is_empty() || !self.diagram.edges.is_empty(),
            Content::Text { text, .. } => !text.trim().is_empty(),
            Content::Image { path, .. } => !path.trim().is_empty(),
            // Its text belongs to the diagram; closing the tab loses nothing.
            Content::FileText { .. } => false,
        }
    }

    /// Asks before closing a tab with contents. True when it may go.
    fn confirm_close(&self, i: usize) -> bool {
        if !self.has_content(i) {
            return true;
        }
        rfd::MessageDialog::new()
            .set_title("Close tab")
            .set_description(format!(
                "Close \"{}\"? Its contents are removed from the project (this cannot be undone).",
                self.tabs[i].name
            ))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes
    }

    /// Removes tab `id` from the project (the dock no longer shows it).
    fn forget_tab(&mut self, id: ItemId) {
        let Some(i) = self.index_of(id) else { return };
        if self.active == Some(i) {
            // Hand the app's fields to another diagram first, if there is one.
            self.force_commit();
            self.editing = None;
            match (0..self.tabs.len()).find(|&j| j != i && self.tabs[j].is_diagram()) {
                Some(next) => self.swap_active(next),
                None => self.detach_active(),
            }
        }
        let name = self.tabs.remove(i).name;
        if let Some(a) = self.active.filter(|&a| i < a) {
            self.active = Some(a - 1);
        }
        self.status = format!("Closed \"{name}\"");
    }

    /// Closes tab `id`, asking first if it has contents (Ctrl+W, menu).
    pub fn close_item(&mut self, id: ItemId) {
        let Some(i) = self.index_of(id) else { return };
        if !self.confirm_close(i) {
            return;
        }
        if let Some(path) = self.dock.find_tab(&id) {
            self.dock.remove_tab(path);
        }
        self.forget_tab(id);
    }

    /// The tab focused in the dock (the one Ctrl+W closes).
    pub fn focused_item(&mut self) -> Option<ItemId> {
        self.dock.find_active_focused().map(|(_, id)| *id)
    }

    /// The kept text selection, when the focused tab is the editor of that file.
    pub fn focused_file_selection(&mut self) -> Option<crate::code::CodeSelection> {
        let i = self.focused_item().and_then(|id| self.index_of(id))?;
        let Content::FileText { diagram, node } = self.tabs[i].content else { return None };
        self.code_selection.clone().filter(|s| s.diagram == diagram && s.node == node)
    }

    /// Whether the focused tab is a diagram (or no tab has focus yet).
    pub fn diagram_focused(&mut self) -> bool {
        match self.focused_item().and_then(|id| self.index_of(id)) {
            Some(i) => matches!(self.tabs[i].content, Content::Diagram(_)),
            None => true,
        }
    }

    fn duplicate_tab(&mut self, id: ItemId) {
        let Some(i) = self.index_of(id) else { return };
        let content = match &self.tabs[i].content {
            Content::Diagram(Some(s)) => ItemContent::Diagram(s.diagram.clone()),
            Content::Diagram(None) => ItemContent::Diagram(self.diagram.clone()),
            Content::Text { text, .. } => ItemContent::Text(text.clone()),
            Content::Image { path, .. } => ItemContent::Image(path.clone()),
            // A second view of the same file adds nothing.
            Content::FileText { .. } => return,
        };
        let new_id = self.alloc_item_id();
        self.next_item_id = new_id + 1;
        let name = format!("{} copy", self.tabs[i].name);
        // A copy has never been saved.
        let content = match content {
            ItemContent::Diagram(d) => {
                let mut s = TabState::new(d, true);
                s.saved = Diagram::default();
                Content::Diagram(Some(s))
            }
            ItemContent::Text(t) => Content::Text { text: t, saved: String::new() },
            ItemContent::Image(p) => Content::Image { path: p, saved: String::new() },
            ItemContent::FileText { .. } => return,
        };
        self.tabs.push(Tab { id: new_id, name, content });
        if let Some(path) = self.dock.find_tab(&id) {
            self.dock.set_focused_node_and_surface(path.node_path());
        }
        self.dock.push_to_focused_leaf(new_id);
        if let Some(j) = self.index_of(new_id).filter(|&j| self.tabs[j].is_diagram()) {
            self.switch_tab(j);
        }
    }

    // ---------------------------------------------------------------- project load and save

    /// Replaces the whole project.
    pub fn reset_project(&mut self, project: Project, path: Option<std::path::PathBuf>) {
        let items = project.items;
        self.dock = project.layout.unwrap_or_else(|| Layout::new(items.iter().map(|i| i.id).collect()));
        self.tabs = items
            .into_iter()
            .map(|item| Tab {
                id: item.id,
                name: item.name,
                content: match item.content {
                    ItemContent::Diagram(d) => Content::Diagram(Some(TabState::new(d, true))),
                    ItemContent::Text(t) => Content::Text { saved: t.clone(), text: t },
                    ItemContent::Image(p) => Content::Image { saved: p.clone(), path: p },
                    ItemContent::FileText { diagram, node } => Content::FileText { diagram, node },
                },
            })
            .collect();
        let active = project
            .active
            .and_then(|id| self.index_of(id))
            .filter(|&i| self.tabs[i].is_diagram())
            .or_else(|| self.tabs.iter().position(Tab::is_diagram));
        self.active = None;
        self.restore(TabState::new(Diagram::default(), true));
        if let Some(active) = active {
            self.swap_active(active);
        }
        self.next_item_id = self.tabs.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        self.saved_names = self.content_tab_names();
        self.renaming = None;
        self.path = path;
    }

    /// Every tab of the project with its current contents.
    pub fn project_items(&self) -> Vec<ProjectItem> {
        self.tabs
            .iter()
            .map(|t| ProjectItem {
                id: t.id,
                name: t.name.clone(),
                content: match &t.content {
                    Content::Diagram(Some(s)) => ItemContent::Diagram(s.diagram.clone()),
                    Content::Diagram(None) => ItemContent::Diagram(self.diagram.clone()),
                    Content::Text { text, .. } => ItemContent::Text(text.clone()),
                    Content::Image { path, .. } => ItemContent::Image(path.clone()),
                    Content::FileText { diagram, node } => ItemContent::FileText { diagram: *diagram, node: *node },
                },
            })
            .collect()
    }

    fn tab_dirty(&self, t: &Tab) -> bool {
        match &t.content {
            Content::Diagram(Some(s)) => s.diagram != s.saved,
            Content::Diagram(None) => self.diagram != self.saved,
            Content::Text { text, saved } => text != saved,
            Content::Image { path, saved } => path != saved,
            Content::FileText { .. } => false,
        }
    }

    /// Names of the tabs that are project content (file editor tabs are only views of a diagram's
    /// file, so opening or closing one is not a change).
    fn content_tab_names(&self) -> Vec<String> {
        self.tabs
            .iter()
            .filter(|t| !matches!(t.content, Content::FileText { .. }))
            .map(|t| t.name.clone())
            .collect()
    }

    /// Unsaved changes in any tab, or tabs added, removed or renamed.
    pub fn project_dirty(&self) -> bool {
        self.tabs.iter().any(|t| self.tab_dirty(t)) || self.content_tab_names() != self.saved_names
    }

    /// After saving: every tab's current contents count as saved.
    pub fn mark_project_saved(&mut self) {
        self.saved = self.diagram.clone();
        for t in &mut self.tabs {
            match &mut t.content {
                Content::Diagram(Some(s)) => s.saved = s.diagram.clone(),
                Content::Diagram(None) => {}
                Content::Text { text, saved } => *saved = text.clone(),
                Content::Image { path, saved } => *saved = path.clone(),
                Content::FileText { .. } => {}
            }
        }
        self.saved_names = self.content_tab_names();
    }

    // ---------------------------------------------------------------- the dock

    /// Draws every tab group and floating window, then applies what was asked for while drawing.
    pub fn workspace(&mut self, ui: &mut Ui) {
        self.last_visible_diagrams = std::mem::take(&mut self.visible_diagrams);
        let mut dock = std::mem::replace(&mut self.dock, Layout::new(Vec::new()));
        let mut pending = Vec::new();
        self.tab_rects.clear();
        let style = Style::from_egui(ui.style().as_ref());
        {
            let mut viewer = Viewer { app: self, pending: &mut pending };
            DockArea::new(&mut dock)
                .style(style)
                .show_add_buttons(true)
                // No collapse triangle in front of each tab group.
                .show_leaf_collapse_buttons(false)
                // The + button opens `add_popup` (Diagram, Text note, Image) rather than calling `on_add`.
                .show_add_popup(true)
                .show_leaf_close_all_buttons(false)
                .show_inside(ui, &mut viewer);
        }
        self.dock = dock;

        if let Some(node) = self.open_file_request.take() {
            self.open_file_tab(node);
        }

        // The active diagram should be one that can be seen: when it is hidden behind another tab,
        // the first visible diagram takes over.
        let active_id = self.active_id();
        if !self.visible_diagrams.iter().any(|(id, _)| Some(*id) == active_id) {
            if let Some(i) = self.visible_diagrams.first().and_then(|(id, _)| self.index_of(*id)) {
                self.switch_tab(i);
            }
        }

        for p in pending {
            match p {
                Pending::Add(kind, group) => {
                    self.add_tab(kind, group);
                }
                Pending::Closed(id) => self.forget_tab(id),
                Pending::Activate(id) => {
                    if let Some(i) = self.index_of(id) {
                        self.switch_tab(i);
                    }
                }
                Pending::Duplicate(id) => self.duplicate_tab(id),
                Pending::Rename(id) => {
                    if let Some(i) = self.index_of(id) {
                        // A file's tab shows the file's name, so that is what is renamed.
                        let name = match self.tabs[i].content {
                            Content::FileText { diagram, node } => self.file_node(diagram, node).map(|n| n.text.clone()),
                            _ => None,
                        };
                        self.renaming = Some((i, name.unwrap_or_else(|| self.tabs[i].name.clone())));
                        self.rename_focus = true;
                    }
                }
            }
        }
        self.rename_window(ui.ctx());
    }

    /// Renaming a tab in place (double-click on a tab, or its context menu): a text field drawn
    /// over the tab. Enter or clicking elsewhere keeps the new name, Esc cancels.
    fn rename_window(&mut self, ctx: &egui::Context) {
        let Some((i, mut text)) = self.renaming.take() else { return };
        if i >= self.tabs.len() {
            return;
        }
        // A tab that is not shown (e.g. in a hidden window) cannot be renamed in place.
        let Some(&rect) = self.tab_rects.get(&self.tabs[i].id) else { return };
        let mut keep = true;
        let mut commit = false;
        let edit_id = egui::Id::unique("rename_tab_edit");
        if self.rename_focus {
            // Start with the whole name selected, as in VS Code.
            let mut state = egui::text_edit::TextEditState::load(ctx, edit_id).unwrap_or_default();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::select_all_str(&text)));
            state.store(ctx, edit_id);
        }
        egui::Area::new(egui::Id::unique("rename_tab"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                let font = egui::TextStyle::Body.resolve(ui.style());
                let text_w = ui.fonts_mut(|f| f.layout_no_wrap(text.clone(), font, egui::Color32::WHITE).size().x);
                let width = (text_w + 16.0).max(rect.width());
                let r = ui.add_sized(
                    vec2(width, rect.height()),
                    egui::TextEdit::singleline(&mut text).id(edit_id).align(egui::Align2::LEFT_CENTER),
                );
                if self.rename_focus {
                    r.request_focus();
                    self.rename_focus = false;
                } else if ui.input(|inp| inp.key_pressed(egui::Key::Escape)) {
                    keep = false;
                } else if r.lost_focus() {
                    commit = true;
                }
            });
        if commit {
            let name = text.trim().to_owned();
            if !name.is_empty() {
                self.rename_tab(i, name);
            }
        } else if keep {
            self.renaming = Some((i, text));
        }
    }

    /// Gives tab `i` a new name. For a file's editor tab this renames the file in its diagram
    /// (and so its format, which follows the extension).
    pub fn rename_tab(&mut self, i: usize, name: String) {
        if let Content::FileText { diagram, node } = self.tabs[i].content {
            if let Some(n) = self.file_node_mut(diagram, node) {
                n.text = name.clone();
            }
        }
        self.tabs[i].name = name;
    }

    /// Text file object `node` of diagram tab `diagram`, wherever that diagram's state is.
    fn file_node(&self, diagram: ItemId, node: Id) -> Option<&Node> {
        let i = self.index_of(diagram)?;
        match &self.tabs[i].content {
            Content::Diagram(None) => self.diagram.node(node),
            Content::Diagram(Some(s)) => s.diagram.node(node),
            _ => None,
        }
    }

    fn file_node_mut(&mut self, diagram: ItemId, node: Id) -> Option<&mut Node> {
        let i = self.index_of(diagram)?;
        match &mut self.tabs[i].content {
            Content::Diagram(None) => self.diagram.node_mut(node),
            Content::Diagram(Some(s)) => s.diagram.node_mut(node),
            _ => None,
        }
    }

    /// Opens (or brings forward) the tab editing the contents of text file `node` of the active
    /// diagram.
    pub fn open_file_tab(&mut self, node: Id) {
        let Some(diagram) = self.active_id() else { return };
        if self.diagram.node(node).is_none_or(|n| n.kind != ShapeKind::File) {
            return;
        }
        let existing = self.tabs.iter().find(|t| {
            matches!(t.content, Content::FileText { diagram: d, node: n } if d == diagram && n == node)
        });
        if let Some(t) = existing {
            if let Some(path) = self.dock.find_tab(&t.id) {
                let _ = self.dock.set_active_tab(path);
                self.dock.set_focused_node_and_surface(path.node_path());
            }
            return;
        }
        let id = self.alloc_item_id();
        self.next_item_id = id + 1;
        let name = self.diagram.node(node).map_or_else(String::new, |n| n.text.clone());
        self.tabs.push(Tab { id, name, content: Content::FileText { diagram, node } });
        // Next to the diagram the file belongs to.
        if let Some(path) = self.dock.find_tab(&diagram) {
            self.dock.set_focused_node_and_surface(path.node_path());
        }
        self.dock.push_to_focused_leaf(id);
    }

    fn show_file_text(&mut self, i: usize, ui: &mut Ui) {
        let id = self.tabs[i].id;
        let Content::FileText { diagram, node } = self.tabs[i].content else { return };
        let Some(n) = self.file_node_mut(diagram, node) else {
            ui.weak("This file is no longer in its diagram. You can close this tab.");
            return;
        };
        ui.horizontal(|ui| {
            ui.strong(&n.text);
            ui.weak(n.file_tag().unwrap_or_default());
            ui.weak("\u{2014} saved with the diagram");
        });
        ui.separator();
        let format = crate::model::text_format(&n.text);
        if let Some((a, b)) = crate::code::code_editor(ui, ("file", id), &mut n.content, format) {
            let sel = crate::code::selection_of(&n.content, a, b, &n.text, diagram, node);
            if self.code_selection.as_ref() != Some(&sel) {
                self.code_selection = Some(sel);
                // The status bar was drawn before this tab; show the change at once.
                ui.ctx().request_repaint();
            }
        }
    }

    fn show_text(&mut self, i: usize, ui: &mut Ui) {
        let id = self.tabs[i].id;
        let Content::Text { text, .. } = &mut self.tabs[i].content else { return };
        egui::ScrollArea::vertical().id_salt(("text", id)).show(ui, |ui| {
            ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(text).hint_text("Notes, requirements, anything...").desired_width(f32::INFINITY),
            );
        });
    }

    fn show_image(&mut self, i: usize, ui: &mut Ui) {
        let id = self.tabs[i].id;
        let Content::Image { path, .. } = &mut self.tabs[i].content else { return };
        ui.horizontal(|ui| {
            ui.label("File");
            if ui.button("Choose...").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg"]).pick_file() {
                    *path = p.display().to_string();
                }
            }
            ui.add(egui::TextEdit::singleline(path).hint_text("path to a .png or .jpg").desired_width(f32::INFINITY));
        });
        ui.separator();
        if path.trim().is_empty() {
            ui.weak("No picture yet: choose a PNG or JPEG file.");
            return;
        }
        let uri = file_uri(path.trim());
        egui::ScrollArea::both().id_salt(("image", id)).show(ui, |ui| {
            ui.add(egui::Image::new(uri).max_size(ui.available_size()).shrink_to_fit());
        });
    }
}

/// A `file://` URI for a path: `file:///C:/x.png` on Windows (two slashes would make `C:` a host
/// name), `file:///home/x.png` on Linux.
fn file_uri(path: &str) -> String {
    let path = path.replace('\\', "/");
    if path.starts_with('/') { format!("file://{path}") } else { format!("file:///{path}") }
}

/// Connects the dock to the app: what each tab shows, its title, closing and adding.
struct Viewer<'a> {
    app: &'a mut App,
    pending: &'a mut Vec<Pending>,
}

impl TabViewer for Viewer<'_> {
    type Tab = ItemId;

    fn id(&mut self, tab: &mut ItemId) -> egui::Id {
        egui::Id::unique(("workspace_tab", *tab))
    }

    fn title(&mut self, tab: &mut ItemId) -> WidgetText {
        let Some(i) = self.app.index_of(*tab) else { return "?".into() };
        if let Content::FileText { diagram, node } = self.app.tabs[i].content {
            // The file's current name (it may have been renamed in the diagram).
            return match self.app.file_node(diagram, node) {
                Some(n) => n.text.clone().into(),
                None => "(missing file)".into(),
            };
        }
        let t = &self.app.tabs[i];
        let dirty = if self.app.tab_dirty(t) { " *" } else { "" };
        let active = if self.app.active == Some(i) { "\u{25cf} " } else { "" };
        format!("{active}{}{dirty}", t.name).into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut ItemId) {
        let Some(i) = self.app.index_of(*tab) else { return };
        match self.app.tabs[i].content {
            Content::Diagram(_) => self.app.show_diagram(i, ui),
            Content::Text { .. } => self.app.show_text(i, ui),
            Content::Image { .. } => self.app.show_image(i, ui),
            Content::FileText { .. } => self.app.show_file_text(i, ui),
        }
    }

    fn context_menu(&mut self, ui: &mut Ui, tab: &mut ItemId, _path: TabPath) {
        if ui.button("Rename").clicked() {
            self.pending.push(Pending::Rename(*tab));
        }
        if ui.button("Duplicate").clicked() {
            self.pending.push(Pending::Duplicate(*tab));
        }
    }

    fn on_tab_button(&mut self, tab: &mut ItemId, response: &egui::Response) {
        self.app.tab_rects.insert(*tab, response.rect);
        if response.double_clicked() {
            self.pending.push(Pending::Rename(*tab));
        } else if response.clicked() {
            // Choosing a diagram's tab makes it the one the format panel edits.
            self.pending.push(Pending::Activate(*tab));
        }
    }

    fn on_close(&mut self, tab: &mut ItemId) -> OnCloseResponse {
        let Some(i) = self.app.index_of(*tab) else { return OnCloseResponse::Close };
        if !self.app.confirm_close(i) {
            return OnCloseResponse::Ignore;
        }
        self.pending.push(Pending::Closed(*tab));
        OnCloseResponse::Close
    }

    fn add_popup(&mut self, ui: &mut Ui, path: NodePath) {
        ui.set_min_width(140.0);
        for (label, kind) in [("Diagram", NewKind::Diagram), ("Text note", NewKind::Text), ("Image", NewKind::Image)] {
            if ui.button(label).clicked() {
                self.pending.push(Pending::Add(kind, Some(path)));
            }
        }
    }

    fn scroll_bars(&self, _tab: &ItemId) -> [bool; 2] {
        // Diagrams pan themselves; text and images scroll inside their own areas.
        [false, false]
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
    fn each_diagram_keeps_its_own_state_and_history() {
        let mut app = app_with_tabs();
        assert_eq!((app.tabs.len(), app.active), (2, Some(1)));
        assert_eq!(app.diagram.nodes.len(), 2);

        app.switch_tab(0);
        assert_eq!(app.diagram.nodes.len(), 1);
        assert_eq!(app.diagram.nodes[0].kind, ShapeKind::Class);

        app.switch_tab(1);
        assert_eq!(app.diagram.nodes.len(), 2);
        app.undo();
        assert!(app.diagram.nodes.len() < 2);
    }

    #[test]
    fn text_and_image_tabs_are_not_diagrams() {
        let mut app = app_with_tabs();
        let text = app.add_tab(NewKind::Text, None);
        let image = app.add_tab(NewKind::Image, None);
        assert_eq!(app.tabs.len(), 4);
        assert_eq!(app.active, Some(1), "adding a note or image keeps the active diagram");
        assert_eq!(app.dock.iter_all_tabs().count(), 4, "every tab is in the layout");
        assert!(!app.tabs[app.index_of(text).unwrap()].is_diagram());

        // Switching to a non-diagram does nothing.
        let at = app.index_of(image).unwrap();
        app.switch_tab(at);
        assert_eq!(app.active, Some(1));
    }

    #[test]
    fn project_round_trip_with_all_kinds_and_layout() {
        let mut app = app_with_tabs();
        let text = app.add_tab(NewKind::Text, None);
        let at = app.index_of(text).unwrap();
        if let Content::Text { text: t, .. } = &mut app.tabs[at].content {
            *t = "hello".into();
        }
        assert!(app.project_dirty());
        let items = app.project_items();
        assert_eq!(items.iter().map(|i| i.content.kind_name()).collect::<Vec<_>>(), ["diagram", "diagram", "text"]);

        app.mark_project_saved();
        assert!(!app.project_dirty());
        app.tabs[0].name = "Renamed".into();
        assert!(app.project_dirty(), "renaming a tab is a change");

        let layout = app.dock.clone();
        let active_id = app.active_id().unwrap();
        app.reset_project(Project { items, active: Some(active_id), layout: Some(layout) }, None);
        assert_eq!(app.tabs.len(), 3);
        assert_eq!(app.active_id(), Some(active_id));
        assert_eq!(app.diagram.nodes.len(), 2);
        assert!(!app.project_dirty());
    }

    #[test]
    fn closing_the_active_diagram_moves_on_and_the_last_one_can_go_too() {
        let mut app = app_with_tabs(); // two diagrams, the second active (non-empty)
        let first = app.tabs[0].id;
        app.switch_tab(0);
        // Closing an empty-content check is skipped here: forget_tab is what the dock calls after
        // it has removed the tab.
        app.forget_tab(first);
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.active, Some(0));
        assert_eq!(app.diagram.nodes.len(), 2, "now showing the remaining diagram");

        // The last diagram may be closed: then there is none, and the project has no diagram.
        let last = app.tabs[0].id;
        app.forget_tab(last);
        assert_eq!((app.tabs.len(), app.active), (0, None));
        assert!(app.diagram.nodes.is_empty());
        assert!(app.project_items().is_empty());

        // Adding a shape from the palette then starts a new diagram.
        app.add_shape_from_palette(ShapeKind::Class);
        assert_eq!(app.tabs.len(), 1);
        assert!(app.tabs[0].is_diagram() && app.active == Some(0));
        assert_eq!(app.diagram.nodes.len(), 1);
    }

    #[test]
    fn double_clicked_text_files_open_once_and_edit_the_diagram() {
        let mut app = App::default();
        let f = app.diagram.add_node(ShapeKind::File, pos2(0.0, 0.0));
        let b = app.diagram.add_node(ShapeKind::BinaryFile, pos2(200.0, 0.0));
        app.open_file_tab(b);
        assert_eq!(app.tabs.len(), 1, "binary files have no text to edit");

        app.open_file_tab(f);
        app.open_file_tab(f);
        assert_eq!(app.tabs.len(), 2, "opening again brings the same tab forward");
        let tab = app.tabs[1].id;
        let Content::FileText { diagram, node } = app.tabs[1].content else { panic!() };
        assert_eq!((diagram, node), (app.tabs[0].id, f));
        assert_eq!(app.dock.iter_all_tabs().count(), 2);

        // Its text is the file object's contents.
        app.file_node_mut(diagram, node).unwrap().content = "{ }".into();
        assert_eq!(app.diagram.node(f).unwrap().content, "{ }");

        // It is saved as part of the project and reopens.
        let items = app.project_items();
        assert!(items.iter().any(|i| i.id == tab && i.content == ItemContent::FileText { diagram, node }));
        let layout = app.dock.clone();
        app.reset_project(Project { items, active: Some(diagram), layout: Some(layout) }, None);
        assert_eq!(app.file_node(diagram, node).unwrap().content, "{ }");

        // Opening a file's tab is not a change to the project.
        app.mark_project_saved();
        app.open_file_tab(app.diagram.nodes.iter().find(|n| n.kind == ShapeKind::File).unwrap().id);
        assert!(!app.project_dirty());

        // Renaming its tab renames the file, and its format follows the new extension.
        let i = app.index_of(tab).unwrap();
        app.rename_tab(i, "main.cpp".into());
        let n = app.file_node(diagram, node).unwrap();
        assert_eq!((n.text.as_str(), n.file_tag().as_deref()), ("main.cpp", Some("CPP")));
    }

    #[test]
    fn the_app_starts_empty_and_the_palette_makes_a_diagram() {
        let mut app = App::default();
        app.start_empty();
        assert!(app.tabs.is_empty() && app.active.is_none() && app.palette_view.is_none());
        assert_eq!(app.dock.iter_all_tabs().count(), 0);
        assert!(!app.project_dirty());

        app.add_shape_from_palette(ShapeKind::Trait);
        assert_eq!(app.dock.iter_all_tabs().count(), 1, "a new diagram tab is shown");
        assert_eq!((app.active, app.diagram.nodes.len()), (Some(0), 1));
    }

    #[test]
    fn image_paths_become_file_uris() {
        assert_eq!(file_uri(r"C:\pics\a.png"), "file:///C:/pics/a.png");
        assert_eq!(file_uri("/home/me/a.png"), "file:///home/me/a.png");
    }

    #[test]
    fn duplicate_copies_contents_into_a_new_unsaved_tab() {
        let mut app = app_with_tabs();
        let id = app.active_id().unwrap();
        app.duplicate_tab(id);
        assert_eq!(app.tabs.len(), 3);
        assert!(app.active_tab_name().unwrap().ends_with("copy"));
        assert_eq!(app.diagram.nodes.len(), 2);
    }
}
