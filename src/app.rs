//! Application shell: menus, toolbar, shape palette, format panel, undo/redo and files.

use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::{vec2, Color32, Event, Key, Modifiers, Pos2, Rect, Sense, Stroke, Ui, Vec2, ViewportCommand};

use crate::canvas::Drag;
use crate::model::{Clip, Diagram, EdgeStyle, Id, ShapeKind};
use crate::render::paint_shape;

const MAX_UNDO: usize = 200;

pub struct App {
    pub diagram: Diagram,
    pub selection: BTreeSet<Id>,
    pub pan: Vec2,
    pub zoom: f32,
    pub canvas_rect: Rect,
    pub drag: Drag,
    pub hovered: Option<Id>,
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
    status: String,
    allow_close: bool,
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
            hovered: None,
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
        egui::Panel::right("format").resizable(true).default_size(240.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.format_panel(ui));
        });
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));

        self.commit_undo(&ctx);
        self.update_title(&ctx);
    }
}

impl App {
    // ---------------------------------------------------------------- history

    fn commit_undo(&mut self, ctx: &egui::Context) {
        if self.diagram == self.stable {
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
        self.hovered = None;
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
        let Some(path) = rfd::FileDialog::new().add_filter("Diagram", &["json"]).pick_file() else { return };
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Diagram>(&s).map_err(|e| e.to_string()))
        {
            Ok(d) => {
                self.status = format!("Opened {}", path.display());
                self.reset_document(d, Some(path));
                self.fit_view();
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

    /// Pastes clipboard text: our own shapes, or plain text as a new text shape.
    fn paste_text(&mut self, text: &str) {
        if let Ok(clip) = serde_json::from_str::<Clip>(text) {
            self.paste_clip(clip);
        } else if !text.trim().is_empty() {
            let center = self.to_world(self.canvas_rect.center());
            let id = self.add_node_at(ShapeKind::Text, center);
            if let Some(n) = self.diagram.node_mut(id) {
                n.text = text.trim().to_owned();
            }
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
        if ctx.egui_wants_keyboard_input() {
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
            self.selection.clear();
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
                "{} shapes, {} connectors  |  zoom {:.0}%",
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
        ui.strong("Shapes");
        ui.separator();
        let tile = vec2(52.0, 44.0);
        ui.horizontal_wrapped(|ui| {
            for kind in ShapeKind::ALL {
                let (rect, resp) = ui.allocate_exact_size(tile, Sense::click_and_drag());
                let resp = resp.on_hover_text(format!("{} (click or drag onto the canvas)", kind.name()));
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
                }
                let size = kind.default_size();
                let scale = ((rect.width() - 14.0) / size.x).min((rect.height() - 14.0) / size.y);
                let icon = Rect::from_center_size(rect.center(), size * scale);
                let stroke_col = ui.visuals().text_color();
                if kind == ShapeKind::Text {
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Text", egui::FontId::proportional(13.0), stroke_col);
                } else {
                    paint_shape(ui.painter(), kind, icon, Color32::WHITE, Stroke::new(1.2, stroke_col), scale);
                }

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
                 - Hover a shape and drag a blue arrow to another shape to connect them. \
                 Click an arrow to clone the shape in that direction.\n\
                 - Double-click to edit text; double-click empty space to add text.\n\
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
        ui.strong(if ids.len() == 1 { "Shape".to_owned() } else { format!("{} shapes", ids.len()) });
        ui.separator();
        egui::Grid::new("node_style").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Type");
            egui::ComboBox::from_id_salt("node_kind").selected_text(n.kind.name()).show_ui(ui, |ui| {
                for k in ShapeKind::ALL {
                    ui.selectable_value(&mut n.kind, k, k.name());
                }
            });
            ui.end_row();
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
            ui.add_space(6.0);
            ui.label("Text");
            ui.add(egui::TextEdit::multiline(&mut n.text).desired_rows(2).desired_width(f32::INFINITY));
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
        ui.strong(if ids.len() == 1 { "Connector".to_owned() } else { format!("{} connectors", ids.len()) });
        ui.separator();
        egui::Grid::new("edge_style").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Style");
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
            ui.label("Pattern");
            ui.checkbox(&mut e.dashed, "Dashed");
            ui.end_row();
            ui.label("Arrows");
            ui.horizontal(|ui| {
                ui.checkbox(&mut e.start_arrow, "Start");
                ui.checkbox(&mut e.end_arrow, "End");
            });
            ui.end_row();
        });
        if ids.len() == 1 {
            ui.add_space(6.0);
            ui.label("Label");
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
            if e.dashed != before.dashed {
                t.dashed = e.dashed;
            }
            if e.start_arrow != before.start_arrow {
                t.start_arrow = e.start_arrow;
            }
            if e.end_arrow != before.end_arrow {
                t.end_arrow = e.end_arrow;
            }
        }
    }
}
