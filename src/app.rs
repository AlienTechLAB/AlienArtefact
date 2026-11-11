//! Application shell: menus, toolbar, shape palette, format panel, undo/redo and files.

use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::{vec2, Color32, Event, Key, Modifiers, Pos2, Rect, Sense, Stroke, Ui, Vec2, ViewportCommand};

use crate::canvas::{Drag, LinkError};
use crate::model::{
    Clip, Diagram, EdgeStyle, Head, Id, LinePattern, LinkKind, Member, MemberKind, Node, PointerKind, RelationKind,
    ShapeKind, Visibility,
};
use crate::render::{paint_edge, paint_shape};

const MAX_UNDO: usize = 200;

pub struct App {
    pub diagram: Diagram,
    pub selection: BTreeSet<Id>,
    pub pan: Vec2,
    pub zoom: f32,
    pub canvas_rect: Rect,
    pub drag: Drag,
    pub editing: Option<Id>,
    pub edit_focus: bool,
    pub palette_drag: Option<ShapeKind>,
    pub show_grid: bool,
    pub snap: bool,
    pub snap_active: bool,

    undo: Vec<Diagram>,
    redo: Vec<Diagram>,
    /// Last committed state; differences from it become one undo step.
    stable: Diagram,
    saved: Diagram,
    path: Option<PathBuf>,
    title: String,
    clipboard: Option<Clip>,
    paste_count: u32,
    pub status: String,
    allow_close: bool,
    /// Fit the view once the canvas size is known (after opening a file).
    pub fit_pending: bool,
    /// Active pointer/relation tool: the next drag (or click) on the canvas draws this kind of line.
    pub link_tool: Option<LinkKind>,
    /// Incorrect line shown with an error window until the user clicks OK.
    pub link_error: Option<LinkError>,
    /// New pointers point to a const object.
    pub pointer_const: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            diagram: Diagram::default(),
            selection: BTreeSet::new(),
            pan: vec2(40.0, 40.0),
            zoom: 1.0,
            canvas_rect: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
            drag: Drag::None,
            editing: None,
            edit_focus: false,
            palette_drag: None,
            show_grid: true,
            snap: true,
            snap_active: true,
            undo: Vec::new(),
            redo: Vec::new(),
            stable: Diagram::default(),
            saved: Diagram::default(),
            path: None,
            title: String::new(),
            clipboard: None,
            paste_count: 0,
            status: String::new(),
            allow_close: false,
            fit_pending: false,
            link_tool: None,
            link_error: None,
            pointer_const: false,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_close(&ctx);
        self.handle_shortcuts(&ctx);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("shapes").resizable(true).default_size(140.0).show(ui, |ui| self.palette(ui));
        egui::Panel::right("format").resizable(true).default_size(320.0).size_range(220.0..=560.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.format_panel(ui));
        });
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
        self.link_error_window(&ctx);

        self.commit_undo(&ctx);
        self.update_title(&ctx);
    }
}

impl App {
    // ---------------------------------------------------------------- history

    fn commit_undo(&mut self, ctx: &egui::Context) {
        // An incorrect line on screen is temporary; it must not become an undo step.
        if self.diagram == self.stable || self.link_error.is_some() {
            return;
        }
        // Wait until a drag or text edit is finished so it becomes a single undo step.
        if ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input() {
            return;
        }
        self.force_commit();
    }

    fn force_commit(&mut self) {
        if self.diagram == self.stable {
            return;
        }
        let prev = std::mem::replace(&mut self.stable, self.diagram.clone());
        self.undo.push(prev);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        self.force_commit();
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.diagram, prev));
            self.after_history_change();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.diagram, next));
            self.after_history_change();
        }
    }

    fn after_history_change(&mut self) {
        self.stable = self.diagram.clone();
        let d = &self.diagram;
        self.selection.retain(|id| d.node(*id).is_some() || d.edge(*id).is_some());
        self.editing = None;
        self.drag = Drag::None;
    }

    fn reset_document(&mut self, diagram: Diagram, path: Option<PathBuf>) {
        self.diagram = diagram;
        self.stable = self.diagram.clone();
        self.saved = self.diagram.clone();
        self.undo.clear();
        self.redo.clear();
        self.selection.clear();
        self.editing = None;
        self.drag = Drag::None;
        self.path = path;
    }

    // ---------------------------------------------------------------- files

    fn dirty(&self) -> bool {
        self.diagram != self.saved
    }

    fn confirm_discard(&self) -> bool {
        !self.dirty()
            || rfd::MessageDialog::new()
                .set_title("Unsaved changes")
                .set_description("The diagram has unsaved changes. Discard them?")
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                == rfd::MessageDialogResult::Yes
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.confirm_discard() {
                self.allow_close = true;
            } else {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            }
        }
    }

    fn new_file(&mut self) {
        if self.confirm_discard() {
            self.reset_document(Diagram::default(), None);
            self.zoom = 1.0;
            self.pan = vec2(40.0, 40.0);
            self.status = "New diagram".into();
        }
    }

    fn open(&mut self) {
        if !self.confirm_discard() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new().add_filter("Diagram", &["json"]).pick_file() {
            self.open_path(path);
        }
    }

    pub fn open_path(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Diagram>(&s).map_err(|e| e.to_string()))
        {
            Ok(mut d) => {
                d.migrate();
                self.status = format!("Opened {}", path.display());
                self.reset_document(d, Some(path));
                self.fit_pending = true;
            }
            Err(e) => self.status = format!("Could not open {}: {e}", path.display()),
        }
    }

    fn save(&mut self) {
        match self.path.clone() {
            Some(p) => self.write_to(p),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Diagram", &["json"])
            .set_file_name("diagram.json")
            .save_file()
        {
            self.write_to(p);
        }
    }

    fn write_to(&mut self, path: PathBuf) {
        self.force_commit();
        let json = serde_json::to_string_pretty(&self.diagram).expect("diagram serializes");
        match std::fs::write(&path, json) {
            Ok(()) => {
                self.saved = self.diagram.clone();
                self.status = format!("Saved {}", path.display());
                self.path = Some(path);
            }
            Err(e) => self.status = format!("Could not save {}: {e}", path.display()),
        }
    }

    fn export_svg(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("SVG image", &["svg"])
            .set_file_name("diagram.svg")
            .save_file()
        else {
            return;
        };
        self.status = match std::fs::write(&path, crate::svg::export(&self.diagram)) {
            Ok(()) => format!("Exported {}", path.display()),
            Err(e) => format!("Could not export {}: {e}", path.display()),
        };
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        let title = format!("{name}{} - architect.ai", if self.dirty() { "*" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    // ---------------------------------------------------------------- incorrect relations

    /// Modal error window for an incorrect relation; OK (or Enter/Esc) undoes the relation.
    fn link_error_window(&mut self, ctx: &egui::Context) {
        let Some(err) = &self.link_error else { return };
        let mut ok = false;
        // Near the top, so the incorrect (red) line on the canvas stays visible.
        let id = egui::Id::unique("link_error");
        let area = egui::Modal::default_area(id).anchor(egui::Align2::CENTER_TOP, vec2(0.0, 120.0));
        egui::Modal::new(id).area(area).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading("Incorrect relation");
            ui.add_space(8.0);
            ui.label(&err.message);
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ok = ui.add(egui::Button::new("   OK   ")).clicked();
            });
        });
        if ok || ctx.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Escape)) {
            self.dismiss_link_error();
        }
    }

    fn dismiss_link_error(&mut self) {
        let Some(err) = self.link_error.take() else { return };
        match err.restore {
            Some((end, orig)) => {
                if let Some(e) = self.diagram.edge_mut(err.id) {
                    if end == 0 { e.from = orig } else { e.to = orig }
                }
            }
            None => {
                self.diagram.edges.retain(|e| e.id != err.id);
                self.selection.remove(&err.id);
            }
        }
    }

    // ---------------------------------------------------------------- edit actions

    pub fn delete_selected(&mut self) {
        self.diagram.remove(&self.selection);
        self.selection.clear();
        self.editing = None;
    }

    pub fn duplicate(&mut self) {
        let clip = self.diagram.copy_subset(&self.selection);
        if !clip.nodes.is_empty() {
            self.selection = self.diagram.paste(&clip, vec2(20.0, 20.0));
        }
    }

    pub fn copy(&mut self, ctx: &egui::Context) {
        let clip = self.diagram.copy_subset(&self.selection);
        if clip.nodes.is_empty() {
            return;
        }
        if let Ok(json) = serde_json::to_string(&clip) {
            ctx.copy_text(json);
        }
        self.clipboard = Some(clip);
        self.paste_count = 0;
    }

    fn paste_clip(&mut self, clip: Clip) {
        self.paste_count += 1;
        let off = 20.0 * self.paste_count as f32;
        self.selection = self.diagram.paste(&clip, vec2(off, off));
    }

    pub fn paste_internal(&mut self) {
        if let Some(clip) = self.clipboard.clone() {
            self.paste_clip(clip);
        }
    }

    /// Pastes objects and pointers copied from this app; other clipboard text is ignored.
    fn paste_text(&mut self, text: &str) {
        if let Ok(clip) = serde_json::from_str::<Clip>(text) {
            self.paste_clip(clip);
        }
    }

    fn nudge(&mut self, d: Vec2) {
        for id in &self.selection {
            if let Some(n) = self.diagram.node_mut(*id) {
                n.pos[0] += d.x;
                n.pos[1] += d.y;
            }
        }
    }

    fn zoom_by(&mut self, factor: f32) {
        self.zoom_at(self.canvas_rect.center(), factor);
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || self.link_error.is_some() {
            return;
        }
        let key = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;

        if key(cmd_shift, Key::Z) || key(cmd, Key::Y) {
            self.redo();
        }
        if key(cmd, Key::Z) {
            self.undo();
        }
        if key(cmd_shift, Key::S) {
            self.save_as();
        }
        if key(cmd, Key::S) {
            self.save();
        }
        if key(cmd, Key::O) {
            self.open();
        }
        if key(cmd, Key::N) {
            self.new_file();
        }
        if key(cmd, Key::A) {
            self.select_all();
        }
        if key(cmd, Key::D) {
            self.duplicate();
        }
        if key(Modifiers::NONE, Key::Delete) || key(Modifiers::NONE, Key::Backspace) {
            self.delete_selected();
        }
        if key(Modifiers::NONE, Key::F2) {
            if let Some(&id) = self.selection.iter().next() {
                self.start_editing(id);
            }
        }
        if key(Modifiers::NONE, Key::Escape) {
            if self.link_tool.take().is_none() {
                self.selection.clear();
            }
        }
        for (k, d) in [
            (Key::ArrowLeft, vec2(-1.0, 0.0)),
            (Key::ArrowRight, vec2(1.0, 0.0)),
            (Key::ArrowUp, vec2(0.0, -1.0)),
            (Key::ArrowDown, vec2(0.0, 1.0)),
        ] {
            if key(Modifiers::SHIFT, k) {
                self.nudge(d * crate::canvas::GRID);
            } else if key(Modifiers::NONE, k) {
                self.nudge(d);
            }
        }

        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            match ev {
                Event::Copy => self.copy(ctx),
                Event::Cut => {
                    self.copy(ctx);
                    self.delete_selected();
                }
                Event::Paste(text) => self.paste_text(&text),
                _ => {}
            }
        }
    }

    // ---------------------------------------------------------------- panels

    fn menu_bar(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.add(egui::Button::new("New").shortcut_text("Ctrl+N")).clicked() {
                    self.new_file();
                }
                if ui.add(egui::Button::new("Open...").shortcut_text("Ctrl+O")).clicked() {
                    self.open();
                }
                if ui.add(egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                    self.save();
                }
                if ui.add(egui::Button::new("Save as...").shortcut_text("Ctrl+Shift+S")).clicked() {
                    self.save_as();
                }
                ui.separator();
                if ui.button("Export as SVG...").clicked() {
                    self.export_svg();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.add_enabled(!self.undo.is_empty() || self.diagram != self.stable, egui::Button::new("Undo").shortcut_text("Ctrl+Z")).clicked() {
                    self.undo();
                }
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Redo").shortcut_text("Ctrl+Y")).clicked() {
                    self.redo();
                }
                ui.separator();
                let has_sel = !self.selection.is_empty();
                if ui.add_enabled(has_sel, egui::Button::new("Cut").shortcut_text("Ctrl+X")).clicked() {
                    self.copy(ui.ctx());
                    self.delete_selected();
                }
                if ui.add_enabled(has_sel, egui::Button::new("Copy").shortcut_text("Ctrl+C")).clicked() {
                    self.copy(ui.ctx());
                }
                if ui.add_enabled(self.clipboard.is_some(), egui::Button::new("Paste").shortcut_text("Ctrl+V")).clicked() {
                    self.paste_internal();
                }
                if ui.add_enabled(has_sel, egui::Button::new("Duplicate").shortcut_text("Ctrl+D")).clicked() {
                    self.duplicate();
                }
                if ui.add_enabled(has_sel, egui::Button::new("Delete").shortcut_text("Del")).clicked() {
                    self.delete_selected();
                }
                ui.separator();
                if ui.add(egui::Button::new("Select all").shortcut_text("Ctrl+A")).clicked() {
                    self.select_all();
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Zoom in").clicked() {
                    self.zoom_by(1.25);
                }
                if ui.button("Zoom out").clicked() {
                    self.zoom_by(0.8);
                }
                if ui.button("Actual size").clicked() {
                    self.zoom_by(1.0 / self.zoom);
                }
                if ui.button("Fit").clicked() {
                    self.fit_view();
                }
                ui.separator();
                ui.checkbox(&mut self.show_grid, "Grid");
                ui.checkbox(&mut self.snap, "Snap to grid");
            });
            ui.menu_button("Arrange", |ui| {
                let has_sel = !self.selection.is_empty();
                if ui.add_enabled(has_sel, egui::Button::new("To front")).clicked() {
                    self.diagram.reorder(&self.selection, true);
                }
                if ui.add_enabled(has_sel, egui::Button::new("To back")).clicked() {
                    self.diagram.reorder(&self.selection, false);
                }
            });
        });
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("New").clicked() {
                self.new_file();
            }
            if ui.button("Open").clicked() {
                self.open();
            }
            if ui.button("Save").clicked() {
                self.save();
            }
            ui.separator();
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo")).clicked() {
                self.undo();
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Redo")).clicked() {
                self.redo();
            }
            ui.separator();
            let has_sel = !self.selection.is_empty();
            if ui.add_enabled(has_sel, egui::Button::new("Delete")).clicked() {
                self.delete_selected();
            }
            if ui.add_enabled(has_sel, egui::Button::new("Duplicate")).clicked() {
                self.duplicate();
            }
            if ui.add_enabled(has_sel, egui::Button::new("To front")).clicked() {
                self.diagram.reorder(&self.selection, true);
            }
            if ui.add_enabled(has_sel, egui::Button::new("To back")).clicked() {
                self.diagram.reorder(&self.selection, false);
            }
            ui.separator();
            if ui.button(" - ").clicked() {
                self.zoom_by(0.8);
            }
            if ui.button(format!("{:.0}%", self.zoom * 100.0)).on_hover_text("Reset zoom").clicked() {
                self.zoom_by(1.0 / self.zoom);
            }
            if ui.button(" + ").clicked() {
                self.zoom_by(1.25);
            }
            if ui.button("Fit").clicked() {
                self.fit_view();
            }
            ui.separator();
            ui.checkbox(&mut self.show_grid, "Grid");
            ui.checkbox(&mut self.snap, "Snap");
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} shapes, {} lines  |  zoom {:.0}%",
                self.diagram.nodes.len(),
                self.diagram.edges.len(),
                self.zoom * 100.0
            ));
            if !self.status.is_empty() {
                ui.separator();
                ui.label(&self.status);
            }
        });
    }

    fn palette(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        self.palette_section(ui, "Objects", &ShapeKind::OBJECTS);
        ui.add_space(8.0);
        self.pointer_palette(ui);
        ui.add_space(8.0);
        self.relation_palette(ui);
        if self.link_tool.is_some() {
            ui.add_space(4.0);
            ui.weak("Drag from one object to another. Esc cancels.");
        }
    }

    /// One palette tile showing a line style with a caption; returns true when clicked.
    fn link_tile(&mut self, ui: &mut Ui, kind: LinkKind, caption: &str, hint: &str) {
        let (rect, resp) = ui.allocate_exact_size(vec2(52.0, 44.0), Sense::click());
        let resp = resp.on_hover_text(format!("{} ({hint})", kind.name()));
        let active = self.link_tool == Some(kind);
        if active {
            ui.painter().rect(rect, 4.0, ui.visuals().selection.bg_fill, ui.visuals().selection.stroke, egui::StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
        }
        let col = ui.visuals().text_color();
        let y = rect.top() + 14.0;
        let pts = [Pos2::new(rect.left() + 6.0, y), Pos2::new(rect.right() - 6.0, y)];
        let (pattern, start, end) = match kind {
            LinkKind::Pointer(k) => k.look(),
            LinkKind::Relation(k) => k.look(),
        };
        paint_edge(ui.painter(), &pts, col, 1.2, pattern, start, end, 0.6);
        ui.painter().text(
            Pos2::new(rect.center().x, rect.bottom() - 12.0),
            egui::Align2::CENTER_CENTER,
            caption,
            egui::FontId::monospace(10.0),
            col,
        );
        if resp.clicked() {
            self.link_tool = if active { None } else { Some(kind) };
        }
    }

    fn relation_palette(&mut self, ui: &mut Ui) {
        ui.strong("Relations");
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            self.link_tile(ui, LinkKind::Relation(RelationKind::Related), "related", "click, then drag between two objects");
            self.link_tile(ui, LinkKind::Relation(RelationKind::Implements), "impl", "click, then drag from a class to a trait");
            self.link_tile(
                ui,
                LinkKind::Relation(RelationKind::Inherits),
                "inherit",
                "click, then drag from the derived class to the base class",
            );
        });
    }

    fn pointer_palette(&mut self, ui: &mut Ui) {
        ui.strong("Pointers");
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for kind in PointerKind::ALL {
                let short = match kind {
                    PointerKind::Raw => "T*",
                    PointerKind::Reference => "T&",
                    PointerKind::Unique => "unique",
                    PointerKind::Shared => "shared",
                    PointerKind::Weak => "weak",
                };
                self.link_tile(ui, LinkKind::Pointer(kind), short, "click, then drag from the owner to the pointed-to object");
            }
        });
        ui.checkbox(&mut self.pointer_const, "Points to const")
            .on_hover_text("New pointers point to a const object");
    }

    fn palette_section(&mut self, ui: &mut Ui, title: &str, kinds: &[ShapeKind]) {
        ui.strong(title);
        ui.separator();
        let tile = vec2(52.0, 44.0);
        ui.horizontal_wrapped(|ui| {
            for &kind in kinds {
                let (rect, resp) = ui.allocate_exact_size(tile, Sense::click_and_drag());
                let resp = resp.on_hover_text(format!("{} (click or drag onto the canvas)", kind.name()));
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
                }
                // Icon in the top part, a short caption below (class and trait icons look alike when small).
                let size = kind.default_size();
                let icon_area = Rect::from_min_max(rect.min + vec2(7.0, 5.0), rect.right_bottom() - vec2(7.0, 17.0));
                let scale = (icon_area.width() / size.x).min(icon_area.height() / size.y);
                let icon = Rect::from_center_size(icon_area.center(), size * scale);
                let stroke_col = ui.visuals().text_color();
                paint_shape(ui.painter(), kind, icon, Color32::WHITE, Stroke::new(1.2, stroke_col), scale, 12.0 * scale);
                let caption = match kind {
                    ShapeKind::Trait => "trait",
                    _ => "class",
                };
                ui.painter().text(
                    egui::pos2(rect.center().x, rect.bottom() - 9.0),
                    egui::Align2::CENTER_CENTER,
                    caption,
                    egui::FontId::monospace(10.0),
                    stroke_col,
                );

                if resp.clicked() {
                    let n = self.diagram.nodes.len() as f32;
                    let center = self.to_world(self.canvas_rect.center()) + vec2(1.0, 1.0) * (n % 8.0) * 10.0;
                    self.add_node_at(kind, center);
                }
                if resp.drag_started() {
                    self.palette_drag = Some(kind);
                }
                if resp.drag_stopped() {
                    if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
                        if self.canvas_rect.contains(p) {
                            let w = self.to_world(p);
                            self.add_node_at(kind, w);
                        }
                    }
                    self.palette_drag = None;
                }
            }
        });
    }

    fn format_panel(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        let node_ids: Vec<Id> = self.selection.iter().copied().filter(|id| self.diagram.node(*id).is_some()).collect();
        let edge_ids: Vec<Id> = self.selection.iter().copied().filter(|id| self.diagram.edge(*id).is_some()).collect();

        if node_ids.is_empty() && edge_ids.is_empty() {
            ui.strong("Diagram");
            ui.separator();
            ui.checkbox(&mut self.show_grid, "Grid");
            ui.checkbox(&mut self.snap, "Snap to grid (hold Alt to bypass)");
            ui.add_space(12.0);
            ui.weak(
                "Tips:\n\
                 - Click or drag shapes from the left panel.\n\
                 - Pick a pointer type in the Pointers section, then drag from the owner to the pointed-to object. \
                 Drag a selected pointer's end handles to re-attach it.\n\
                 - Double-click an object or pointer to edit its name or label.\n\
                 - Ctrl+wheel zooms, wheel scrolls, middle/right-drag pans.",
            );
            return;
        }

        if !node_ids.is_empty() {
            self.node_format(ui, &node_ids);
        }
        if !edge_ids.is_empty() {
            if !node_ids.is_empty() {
                ui.add_space(8.0);
            }
            self.edge_format(ui, &edge_ids);
        }

        ui.add_space(8.0);
        ui.strong("Arrange");
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("To front").clicked() {
                self.diagram.reorder(&self.selection, true);
            }
            if ui.button("To back").clicked() {
                self.diagram.reorder(&self.selection, false);
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Duplicate").clicked() {
                self.duplicate();
            }
            if ui.button("Delete").clicked() {
                self.delete_selected();
            }
        });
    }

    fn node_format(&mut self, ui: &mut Ui, ids: &[Id]) {
        let before = self.diagram.node(ids[0]).unwrap().clone();
        let mut n = before.clone();
        let single_class = ids.len() == 1 && n.kind.is_object();

        if single_class {
            ui.strong(if n.kind == ShapeKind::Trait { "Trait" } else { "Class" });
            ui.separator();
            egui::Grid::new("class_props").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut n.text).desired_width(f32::INFINITY));
                ui.end_row();
            });
            members_ui(ui, &mut n);
            ui.add_space(12.0);
            ui.strong("Style");
        } else {
            ui.strong(if ids.len() == 1 { "Object".to_owned() } else { format!("{} objects", ids.len()) });
        }
        ui.separator();
        egui::Grid::new("node_style").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Fill");
            ui.color_edit_button_srgba_unmultiplied(&mut n.fill);
            ui.end_row();
            ui.label("Line");
            ui.color_edit_button_srgba_unmultiplied(&mut n.stroke);
            ui.end_row();
            ui.label("Line width");
            ui.add(egui::DragValue::new(&mut n.stroke_width).range(0.0..=20.0).speed(0.1).suffix(" pt"));
            ui.end_row();
            ui.label("Font color");
            ui.color_edit_button_srgba_unmultiplied(&mut n.text_color);
            ui.end_row();
            ui.label("Font size");
            ui.add(egui::DragValue::new(&mut n.font_size).range(4.0..=96.0).speed(0.2).suffix(" pt"));
            ui.end_row();
        });

        if ids.len() == 1 {
            if !single_class {
                ui.add_space(6.0);
                ui.label("Text");
                ui.add(egui::TextEdit::multiline(&mut n.text).desired_rows(2).desired_width(f32::INFINITY));
            }
            ui.add_space(6.0);
            egui::Grid::new("node_geom").num_columns(4).spacing([6.0, 6.0]).show(ui, |ui| {
                ui.label("X");
                ui.add(egui::DragValue::new(&mut n.pos[0]));
                ui.label("Y");
                ui.add(egui::DragValue::new(&mut n.pos[1]));
                ui.end_row();
                ui.label("W");
                ui.add(egui::DragValue::new(&mut n.size[0]).range(10.0..=10000.0));
                ui.label("H");
                ui.add(egui::DragValue::new(&mut n.size[1]).range(10.0..=10000.0));
                ui.end_row();
            });
        }

        if n == before {
            return;
        }
        if ids.len() == 1 {
            // Grow the box so the name and all members stay visible.
            if n.kind.is_object()
                && (n.members != before.members || n.text != before.text || n.font_size != before.font_size)
            {
                let painter = ui.painter();
                let font = egui::FontId::proportional(n.font_size);
                let need = n.required_size(|s| painter.layout_no_wrap(s.to_owned(), font.clone(), Color32::BLACK).size().x);
                n.size = [n.size[0].max(need.x), n.size[1].max(need.y)];
            }
            *self.diagram.node_mut(ids[0]).unwrap() = n;
            return;
        }
        // Apply only the fields that changed to every selected shape.
        for id in ids {
            let t = self.diagram.node_mut(*id).unwrap();
            if n.kind != before.kind {
                t.kind = n.kind;
            }
            if n.fill != before.fill {
                t.fill = n.fill;
            }
            if n.stroke != before.stroke {
                t.stroke = n.stroke;
            }
            if n.stroke_width != before.stroke_width {
                t.stroke_width = n.stroke_width;
            }
            if n.text_color != before.text_color {
                t.text_color = n.text_color;
            }
            if n.font_size != before.font_size {
                t.font_size = n.font_size;
            }
        }
    }

    fn edge_format(&mut self, ui: &mut Ui, ids: &[Id]) {
        let before = self.diagram.edge(ids[0]).unwrap().clone();
        let mut e = before.clone();
        let heading = match (ids.len(), e.link()) {
            (1, Some(LinkKind::Pointer(_))) => "Pointer".to_owned(),
            (1, Some(LinkKind::Relation(_))) => "Relation".to_owned(),
            (1, None) => "Line".to_owned(),
            (n, _) => format!("{n} lines"),
        };
        ui.strong(heading);
        ui.separator();
        // A relation is only offered when every selected line fits its rule (e.g. class -> trait).
        let relation_ok: Vec<bool> = RelationKind::ALL
            .iter()
            .map(|k| {
                ids.iter().all(|id| {
                    let l = self.diagram.edge(*id).unwrap();
                    self.diagram.link_allowed(LinkKind::Relation(*k), l.from, l.to)
                })
            })
            .collect();
        egui::Grid::new("edge_style").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Type");
            let mut link = e.link();
            egui::ComboBox::from_id_salt("edge_link")
                .selected_text(link.map_or("Plain connector", |k| k.name()))
                .show_ui(ui, |ui| {
                    for k in PointerKind::ALL {
                        ui.selectable_value(&mut link, Some(LinkKind::Pointer(k)), k.name());
                    }
                    ui.separator();
                    for (k, ok) in RelationKind::ALL.into_iter().zip(&relation_ok) {
                        ui.add_enabled_ui(*ok, |ui| {
                            ui.selectable_value(&mut link, Some(LinkKind::Relation(k)), k.name())
                                .on_disabled_hover_text(k.rule());
                        });
                    }
                });
            if link != e.link() {
                if let Some(k) = link {
                    e.set_link(k);
                }
            }
            ui.end_row();
            if e.pointer.is_some() {
                ui.label("Target");
                ui.checkbox(&mut e.const_target, "const");
                ui.end_row();
            } else {
                ui.label("Pattern");
                egui::ComboBox::from_id_salt("edge_pattern").selected_text(e.pattern.name()).show_ui(ui, |ui| {
                    for p in LinePattern::ALL {
                        ui.selectable_value(&mut e.pattern, p, p.name());
                    }
                });
                ui.end_row();
                for (label, head, salt) in [("Start", &mut e.start_head, "start_head"), ("End", &mut e.end_head, "end_head")] {
                    ui.label(label);
                    egui::ComboBox::from_id_salt(salt).selected_text(head.name()).show_ui(ui, |ui| {
                        for h in Head::ALL {
                            ui.selectable_value(head, h, h.name());
                        }
                    });
                    ui.end_row();
                }
            }
            ui.label("Routing");
            egui::ComboBox::from_id_salt("edge_style")
                .selected_text(match e.style {
                    EdgeStyle::Orthogonal => "Orthogonal",
                    EdgeStyle::Straight => "Straight",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut e.style, EdgeStyle::Orthogonal, "Orthogonal");
                    ui.selectable_value(&mut e.style, EdgeStyle::Straight, "Straight");
                });
            ui.end_row();
            ui.label("Color");
            ui.color_edit_button_srgba_unmultiplied(&mut e.color);
            ui.end_row();
            ui.label("Width");
            ui.add(egui::DragValue::new(&mut e.width).range(0.5..=20.0).speed(0.1).suffix(" pt"));
            ui.end_row();
        });
        if ids.len() == 1 {
            ui.add_space(6.0);
            ui.label(if e.pointer.is_some() { "Label (e.g. member name)" } else { "Label" });
            ui.add(egui::TextEdit::singleline(&mut e.label).desired_width(f32::INFINITY));
        }

        if e == before {
            return;
        }
        if ids.len() == 1 {
            *self.diagram.edge_mut(ids[0]).unwrap() = e;
            return;
        }
        for id in ids {
            let t = self.diagram.edge_mut(*id).unwrap();
            if e.style != before.style {
                t.style = e.style;
            }
            if e.color != before.color {
                t.color = e.color;
            }
            if e.width != before.width {
                t.width = e.width;
            }
            if e.pattern != before.pattern {
                t.pattern = e.pattern;
            }
            if e.start_head != before.start_head {
                t.start_head = e.start_head;
            }
            if e.end_head != before.end_head {
                t.end_head = e.end_head;
            }
            if e.pointer != before.pointer || e.relation != before.relation {
                t.pointer = e.pointer;
                t.relation = e.relation;
            }
            if e.const_target != before.const_target {
                t.const_target = e.const_target;
            }
        }
    }
}

/// Fields and Methods sections of the format panel for one class or trait.
fn members_ui(ui: &mut Ui, n: &mut Node) {
    if n.kind == ShapeKind::Class {
        member_section(ui, n, MemberKind::Field, "Fields");
    }
    member_section(ui, n, MemberKind::Method, "Methods");
}

enum RowAction {
    Up,
    Down,
    Remove,
}

fn member_section(ui: &mut Ui, n: &mut Node, kind: MemberKind, title: &str) {
    let owner = n.kind;
    let in_trait = owner == ShapeKind::Trait;
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.strong(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if kind == MemberKind::Field { "+ Add field" } else { "+ Add method" };
            if ui.small_button(label).clicked() {
                let m = Member::new(kind, owner, &n.members);
                n.members.push(m);
            }
        });
    });
    ui.separator();

    let rows: Vec<usize> = (0..n.members.len()).filter(|&i| n.members[i].kind == kind).collect();
    if rows.is_empty() {
        ui.weak(if kind == MemberKind::Field { "No fields." } else { "No methods." });
        return;
    }
    let mut action: Option<(usize, RowAction)> = None;
    for (pos, &i) in rows.iter().enumerate() {
        let m = &mut n.members[i];
        ui.push_id(i, |ui| {
            egui::Frame::group(ui.style()).inner_margin(6.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if in_trait {
                        // Trait members are part of the interface: always public.
                        m.visibility = Visibility::Public;
                        ui.label("+").on_hover_text("Trait members are always public");
                    } else {
                        egui::ComboBox::from_id_salt("vis")
                            .width(36.0)
                            .selected_text(m.visibility.symbol())
                            .show_ui(ui, |ui| {
                                for v in Visibility::ALL {
                                    ui.selectable_value(&mut m.visibility, v, format!("{}  {}", v.symbol(), v.name()));
                                }
                            })
                            .response
                            .on_hover_text(m.visibility.name());
                    }
                    // Right-to-left so the buttons get their real width and the name field fills
                    // exactly what is left (fixed width estimates make the panel creep wider).
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("x").on_hover_text("Remove").clicked() {
                            action = Some((pos, RowAction::Remove));
                        }
                        if ui
                            .add_enabled(pos + 1 < rows.len(), egui::Button::new("v").small())
                            .on_hover_text("Move down")
                            .clicked()
                        {
                            action = Some((pos, RowAction::Down));
                        }
                        if ui.add_enabled(pos > 0, egui::Button::new("^").small()).on_hover_text("Move up").clicked() {
                            action = Some((pos, RowAction::Up));
                        }
                        ui.add(egui::TextEdit::singleline(&mut m.name).hint_text("name").desired_width(f32::INFINITY));
                    });
                });
                if m.kind == MemberKind::Method {
                    ui.horizontal(|ui| {
                        ui.label("params");
                        ui.add(
                            egui::TextEdit::singleline(&mut m.params)
                                .hint_text("a: int, b: &str")
                                .desired_width(f32::INFINITY),
                        );
                    });
                }
                ui.horizontal(|ui| {
                    ui.label(if m.kind == MemberKind::Method { "returns" } else { "type" });
                    let hint = if m.kind == MemberKind::Method { "none" } else { "int" };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.checkbox(&mut m.is_static, "static").on_hover_text("Belongs to the type, not to each value");
                        ui.add(egui::TextEdit::singleline(&mut m.ty).hint_text(hint).desired_width(f32::INFINITY));
                    });
                });
            });
        });
    }
    match action {
        Some((pos, RowAction::Up)) => n.members.swap(rows[pos], rows[pos - 1]),
        Some((pos, RowAction::Down)) => n.members.swap(rows[pos], rows[pos + 1]),
        Some((pos, RowAction::Remove)) => {
            n.members.remove(rows[pos]);
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::End;
    use egui::pos2;

    fn app_with_classes() -> (App, Id, Id, Id) {
        let mut app = App::default();
        let c1 = app.diagram.add_node(ShapeKind::Class, pos2(0.0, 0.0));
        let c2 = app.diagram.add_node(ShapeKind::Class, pos2(300.0, 0.0));
        let t = app.diagram.add_node(ShapeKind::Trait, pos2(0.0, 300.0));
        (app, c1, c2, t)
    }

    #[test]
    fn ok_removes_an_incorrect_new_relation() {
        let (mut app, c1, c2, _) = app_with_classes();
        let implements = LinkKind::Relation(RelationKind::Implements);
        let id = app.diagram.add_link(End::Node(c1), End::Node(c2), implements, false);
        let message = app.diagram.link_error(implements, End::Node(c1), End::Node(c2)).unwrap();
        app.link_error = Some(LinkError { id, restore: None, message });

        app.dismiss_link_error();
        assert!(app.diagram.edge(id).is_none());
        assert!(app.link_error.is_none());
    }

    #[test]
    fn ok_restores_a_moved_end_of_a_correct_relation() {
        let (mut app, c1, c2, t) = app_with_classes();
        let implements = LinkKind::Relation(RelationKind::Implements);
        let id = app.diagram.add_link(End::Node(c1), End::Node(t), implements, false);
        // The user dragged the trait end onto another class.
        app.diagram.edge_mut(id).unwrap().to = End::Node(c2);
        app.link_error = Some(LinkError { id, restore: Some((1, End::Node(t))), message: String::new() });

        app.dismiss_link_error();
        assert_eq!(app.diagram.edge(id).unwrap().to, End::Node(t));
    }
}
