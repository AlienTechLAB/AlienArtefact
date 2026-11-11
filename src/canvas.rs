//! The drawing canvas: pan/zoom, selection, moving, resizing, connecting, in-place text editing.

use std::collections::BTreeSet;

use egui::{
    pos2, vec2, Color32, FontId, Id as EguiId, Key, LayerId, Order, Painter, PointerButton,
    Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};

use crate::app::App;
use crate::model::{color, polyline_midpoint, Id, ShapeKind};
use crate::render::{layout_centered, paint_edge, paint_shape, paint_text};

pub const GRID: f32 = 10.0;
const MIN_SIZE: f32 = 10.0;
const HANDLE_HIT: f32 = 7.0;
const ARROW_GAP: f32 = 22.0;
const ARROW_HIT: f32 = 10.0;
const EDGE_HIT: f32 = 6.0;
const SEL: Color32 = Color32::from_rgb(0, 120, 215);
const HOVER: Color32 = Color32::from_rgb(41, 182, 242);

pub enum Drag {
    None,
    Move { start: Pos2, orig: Vec<(Id, Pos2)> },
    Resize { id: Id, handle: usize, orig: Rect, start: Pos2 },
    Rubber { start: Pos2 },
    Connect { from: Id },
    Pan,
}

fn snap(v: f32, on: bool) -> f32 {
    if on { (v / GRID).round() * GRID } else { v }
}

/// Resize handle positions: TL, T, TR, R, BR, B, BL, L.
fn handle_positions(r: Rect) -> [Pos2; 8] {
    let (l, cx, rr) = (r.left(), r.center().x, r.right());
    let (t, cy, b) = (r.top(), r.center().y, r.bottom());
    [pos2(l, t), pos2(cx, t), pos2(rr, t), pos2(rr, cy), pos2(rr, b), pos2(cx, b), pos2(l, b), pos2(l, cy)]
}

/// Connection arrows around a shape (screen space): up, right, down, left.
fn arrow_positions(r: Rect) -> [(Pos2, Vec2); 4] {
    let c = r.center();
    [
        (pos2(c.x, r.top() - ARROW_GAP), vec2(0.0, -1.0)),
        (pos2(r.right() + ARROW_GAP, c.y), vec2(1.0, 0.0)),
        (pos2(c.x, r.bottom() + ARROW_GAP), vec2(0.0, 1.0)),
        (pos2(r.left() - ARROW_GAP, c.y), vec2(-1.0, 0.0)),
    ]
}

impl App {
    pub fn to_screen(&self, p: Pos2) -> Pos2 {
        self.canvas_rect.min + self.pan + p.to_vec2() * self.zoom
    }

    pub fn to_world(&self, s: Pos2) -> Pos2 {
        ((s - self.canvas_rect.min - self.pan) / self.zoom).to_pos2()
    }

    pub fn rect_to_screen(&self, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(r.min), self.to_screen(r.max))
    }

    /// Zooms by `factor` keeping the world point under `screen` fixed.
    pub fn zoom_at(&mut self, screen: Pos2, factor: f32) {
        let world = self.to_world(screen);
        self.zoom = (self.zoom * factor).clamp(0.1, 8.0);
        self.pan = screen - self.canvas_rect.min - world.to_vec2() * self.zoom;
    }

    pub fn fit_view(&mut self) {
        let Some(b) = self.diagram.bounds() else {
            self.zoom = 1.0;
            self.pan = vec2(40.0, 40.0);
            return;
        };
        let avail = self.canvas_rect.size();
        let zoom = ((avail.x - 80.0) / b.width()).min((avail.y - 80.0) / b.height());
        self.zoom = zoom.clamp(0.1, 2.0);
        self.pan = avail / 2.0 - b.center().to_vec2() * self.zoom;
    }

    pub fn snap_pos(&self, p: Pos2) -> Pos2 {
        pos2(snap(p.x, self.snap), snap(p.y, self.snap))
    }

    fn single_selected_node(&self) -> Option<Id> {
        if self.selection.len() != 1 {
            return None;
        }
        let id = *self.selection.iter().next()?;
        self.diagram.node(id).map(|_| id)
    }

    fn hover_target(&self, p: Pos2) -> Option<Id> {
        if let Some(id) = self.diagram.node_at(self.to_world(p)) {
            return Some(id);
        }
        // Stay on the last hovered shape while the pointer is near it, so its arrows can be reached.
        let id = self.hovered?;
        let n = self.diagram.node(id)?;
        self.rect_to_screen(n.rect()).expand(ARROW_GAP + ARROW_HIT).contains(p).then_some(id)
    }

    fn arrow_hit(&self, p: Pos2) -> Option<(Id, usize)> {
        let id = self.hovered?;
        if self.editing == Some(id) {
            return None;
        }
        let n = self.diagram.node(id)?;
        let sr = self.rect_to_screen(n.rect());
        arrow_positions(sr)
            .iter()
            .position(|(a, _)| a.distance(p) <= ARROW_HIT)
            .map(|i| (id, i))
    }

    pub fn canvas(&mut self, ui: &mut Ui) {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.canvas_rect = rect;
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let (alt, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift));
        self.snap_active = self.snap && !alt;

        if resp.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            if zoom != 1.0 {
                if let Some(p) = pointer {
                    self.zoom_at(p, zoom);
                }
            } else if scroll != Vec2::ZERO {
                self.pan += scroll;
            }
        }

        if matches!(self.drag, Drag::None) {
            self.hovered = pointer.filter(|p| rect.contains(*p)).and_then(|p| self.hover_target(p));
        }

        self.handle_input(ui, &resp, pointer, shift);

        let painter = ui.painter_at(rect);
        self.paint(&painter, pointer);
        self.paint_palette_ghost(ui, pointer);
        self.text_editor(ui);
        self.context_menu(&resp, pointer);
    }

    fn handle_input(&mut self, ui: &Ui, resp: &Response, pointer: Option<Pos2>, shift: bool) {
        if resp.drag_started_by(PointerButton::Primary) {
            if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
                self.begin_drag(p, shift);
            }
        } else if resp.drag_started_by(PointerButton::Middle) || resp.drag_started_by(PointerButton::Secondary) {
            self.drag = Drag::Pan;
        }

        if resp.dragged() {
            if let Some(p) = pointer {
                self.update_drag(p, resp.drag_delta());
            }
        }

        if resp.drag_stopped() {
            if let Some(p) = pointer {
                self.end_drag(p);
            }
            self.drag = Drag::None;
        }

        if resp.double_clicked() {
            if let Some(p) = pointer {
                self.on_double_click(p);
            }
        } else if resp.clicked() {
            if let Some(p) = pointer {
                self.on_click(p, shift);
            }
        }
    }

    fn select_only(&mut self, id: Id) {
        self.selection.clear();
        self.selection.insert(id);
    }

    fn begin_drag(&mut self, p: Pos2, shift: bool) {
        let w = self.to_world(p);

        if let Some(id) = self.single_selected_node() {
            let orig = self.diagram.node(id).unwrap().rect();
            let sr = self.rect_to_screen(orig);
            if let Some(handle) = handle_positions(sr).iter().position(|h| h.distance(p) <= HANDLE_HIT) {
                self.drag = Drag::Resize { id, handle, orig, start: w };
                return;
            }
        }

        if let Some((from, _)) = self.arrow_hit(p) {
            self.drag = Drag::Connect { from };
            return;
        }

        if let Some(id) = self.diagram.node_at(w) {
            if !self.selection.contains(&id) {
                if !shift {
                    self.selection.clear();
                }
                self.selection.insert(id);
            }
            let orig = self
                .selection
                .iter()
                .filter_map(|id| self.diagram.node(*id).map(|n| (*id, n.rect().min)))
                .collect();
            self.drag = Drag::Move { start: w, orig };
            return;
        }

        if let Some(id) = self.diagram.edge_at(w, EDGE_HIT / self.zoom) {
            if !shift {
                self.selection.clear();
            }
            self.selection.insert(id);
            return;
        }

        if !shift {
            self.selection.clear();
        }
        self.drag = Drag::Rubber { start: w };
    }

    fn update_drag(&mut self, p: Pos2, delta: Vec2) {
        let w = self.to_world(p);
        let on = self.snap_active;
        match &self.drag {
            Drag::Move { start, orig } => {
                let mut d = w - *start;
                // Snap the first shape to the grid and move the rest by the same amount.
                if let Some((_, o)) = orig.first() {
                    let t = *o + d;
                    d = pos2(snap(t.x, on), snap(t.y, on)) - *o;
                }
                for (id, o) in orig {
                    if let Some(n) = self.diagram.node_mut(*id) {
                        n.pos = [o.x + d.x, o.y + d.y];
                    }
                }
            }
            Drag::Resize { id, handle, orig, start } => {
                let d = w - *start;
                let (mut min, mut max) = (orig.min, orig.max);
                let h = *handle;
                if matches!(h, 0 | 6 | 7) {
                    min.x = snap(orig.min.x + d.x, on).min(max.x - MIN_SIZE);
                }
                if matches!(h, 2..=4) {
                    max.x = snap(orig.max.x + d.x, on).max(min.x + MIN_SIZE);
                }
                if matches!(h, 0..=2) {
                    min.y = snap(orig.min.y + d.y, on).min(max.y - MIN_SIZE);
                }
                if matches!(h, 4..=6) {
                    max.y = snap(orig.max.y + d.y, on).max(min.y + MIN_SIZE);
                }
                let id = *id;
                if let Some(n) = self.diagram.node_mut(id) {
                    n.set_rect(Rect::from_min_max(min, max));
                }
            }
            Drag::Pan => self.pan += delta,
            _ => {}
        }
    }

    fn end_drag(&mut self, p: Pos2) {
        let w = self.to_world(p);
        match std::mem::replace(&mut self.drag, Drag::None) {
            Drag::Rubber { start } => {
                let r = Rect::from_two_pos(start, w);
                for n in &self.diagram.nodes {
                    if r.contains_rect(n.rect()) {
                        self.selection.insert(n.id);
                    }
                }
                for e in &self.diagram.edges {
                    if self.diagram.edge_path(e).is_some_and(|path| path.iter().all(|p| r.contains(*p))) {
                        self.selection.insert(e.id);
                    }
                }
            }
            Drag::Connect { from } => match self.diagram.node_at(w) {
                Some(to) if to != from => {
                    let id = self.diagram.add_edge(from, to);
                    self.select_only(id);
                }
                Some(_) => {}
                None => {
                    // Dropped on empty canvas: create a copy of the source shape there and connect it.
                    let Some(src) = self.diagram.node(from).cloned() else { return };
                    let mut n = src;
                    n.id = self.diagram.alloc_id();
                    n.text.clear();
                    let min = self.snap_pos(w - n.rect().size() / 2.0);
                    n.pos = [min.x, min.y];
                    let new_id = n.id;
                    self.diagram.nodes.push(n);
                    self.diagram.add_edge(from, new_id);
                    self.select_only(new_id);
                }
            },
            _ => {}
        }
    }

    fn on_click(&mut self, p: Pos2, shift: bool) {
        if let Some((id, dir)) = self.arrow_hit(p) {
            self.clone_towards(id, dir);
            return;
        }
        let w = self.to_world(p);
        let hit = self.diagram.node_at(w).or_else(|| self.diagram.edge_at(w, EDGE_HIT / self.zoom));
        match hit {
            Some(id) if shift => {
                if !self.selection.remove(&id) {
                    self.selection.insert(id);
                }
            }
            Some(id) => self.select_only(id),
            None if !shift => self.selection.clear(),
            None => {}
        }
    }

    fn on_double_click(&mut self, p: Pos2) {
        let w = self.to_world(p);
        let hit = self.diagram.node_at(w).or_else(|| self.diagram.edge_at(w, EDGE_HIT / self.zoom));
        match hit {
            Some(id) => self.start_editing(id),
            None => {
                let id = self.add_node_at(ShapeKind::Text, w);
                self.start_editing(id);
            }
        }
    }

    pub fn start_editing(&mut self, id: Id) {
        self.select_only(id);
        self.editing = Some(id);
        self.edit_focus = true;
    }

    /// Clones a shape next to itself in the given arrow direction and connects them.
    fn clone_towards(&mut self, id: Id, dir: usize) {
        let Some(src) = self.diagram.node(id).cloned() else { return };
        let r = src.rect();
        let gap = 60.0;
        let off = match dir {
            0 => vec2(0.0, -(r.height() + gap)),
            1 => vec2(r.width() + gap, 0.0),
            2 => vec2(0.0, r.height() + gap),
            _ => vec2(-(r.width() + gap), 0.0),
        };
        let mut n = src;
        n.id = self.diagram.alloc_id();
        n.text.clear();
        n.pos = [r.min.x + off.x, r.min.y + off.y];
        let new_id = n.id;
        self.diagram.nodes.push(n);
        self.diagram.add_edge(id, new_id);
        self.select_only(new_id);
        self.hovered = Some(new_id);
    }

    pub fn add_node_at(&mut self, kind: ShapeKind, world_center: Pos2) -> Id {
        let id = self.diagram.add_node(kind, world_center);
        let snapped = {
            let n = self.diagram.node(id).unwrap();
            self.snap_pos(n.rect().min)
        };
        if let Some(n) = self.diagram.node_mut(id) {
            n.pos = [snapped.x, snapped.y];
        }
        self.select_only(id);
        id
    }

    fn paint(&self, painter: &Painter, pointer: Option<Pos2>) {
        let rect = self.canvas_rect;
        let z = self.zoom;
        painter.rect_filled(rect, 0.0, Color32::WHITE);

        if self.show_grid {
            let mut step = GRID * z;
            while step < 6.0 {
                step *= 5.0;
            }
            let origin = rect.min + self.pan;
            let grid_stroke = Stroke::new(1.0, Color32::from_gray(235));
            let mut x = rect.left() + (origin.x - rect.left()).rem_euclid(step);
            while x < rect.right() {
                painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], grid_stroke);
                x += step;
            }
            let mut y = rect.top() + (origin.y - rect.top()).rem_euclid(step);
            while y < rect.bottom() {
                painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], grid_stroke);
                y += step;
            }
        }

        for n in &self.diagram.nodes {
            let sr = self.rect_to_screen(n.rect());
            paint_shape(painter, n.kind, sr, color(n.fill), Stroke::new(n.stroke_width * z, color(n.stroke)), z);
            if self.editing != Some(n.id) {
                paint_text(painter, sr, &n.text, n.font_size * z, color(n.text_color));
            }
        }

        for e in &self.diagram.edges {
            let Some(path) = self.diagram.edge_path(e) else { continue };
            let sp: Vec<Pos2> = path.iter().map(|p| self.to_screen(*p)).collect();
            if self.selection.contains(&e.id) {
                painter.add(Shape::line(sp.clone(), Stroke::new(e.width * z + 6.0, SEL.gamma_multiply(0.25))));
            }
            paint_edge(painter, &sp, color(e.color), e.width * z, e.dashed, e.start_arrow, e.end_arrow, z);
            if !e.label.is_empty() && self.editing != Some(e.id) {
                let mid = polyline_midpoint(&sp);
                let galley = layout_centered(painter, &e.label, 11.0 * z, color(e.color), f32::INFINITY);
                let size = galley.size();
                painter.rect_filled(Rect::from_center_size(mid, size + vec2(4.0, 2.0)), 0.0, Color32::WHITE);
                painter.galley(pos2(mid.x, mid.y - size.y / 2.0), galley, color(e.color));
            }
            if self.selection.contains(&e.id) {
                for p in [sp[0], sp[sp.len() - 1]] {
                    painter.rect(Rect::from_center_size(p, vec2(7.0, 7.0)), 0.0, Color32::WHITE, Stroke::new(1.0, SEL), StrokeKind::Middle);
                }
            }
        }

        let single = self.single_selected_node();
        for id in &self.selection {
            let Some(n) = self.diagram.node(*id) else { continue };
            let sr = self.rect_to_screen(n.rect()).expand(2.0);
            let corners = [sr.left_top(), sr.right_top(), sr.right_bottom(), sr.left_bottom(), sr.left_top()];
            painter.extend(Shape::dashed_line(&corners, Stroke::new(1.0, SEL), 4.0, 3.0));
            if single == Some(*id) {
                for h in handle_positions(self.rect_to_screen(n.rect())) {
                    painter.rect(Rect::from_center_size(h, vec2(8.0, 8.0)), 1.0, Color32::WHITE, Stroke::new(1.0, SEL), StrokeKind::Middle);
                }
            }
        }

        if let (Some(id), Drag::None) = (self.hovered, &self.drag) {
            if let Some(n) = self.diagram.node(id).filter(|_| self.editing != Some(id)) {
                let hit = pointer.and_then(|p| self.arrow_hit(p)).map(|(_, i)| i);
                for (i, (c, dir)) in arrow_positions(self.rect_to_screen(n.rect())).into_iter().enumerate() {
                    let perp = vec2(-dir.y, dir.x);
                    let col = if hit == Some(i) { SEL } else { HOVER.gamma_multiply(0.8) };
                    let tip = c + dir * 7.0;
                    let base = c - dir * 5.0;
                    painter.add(Shape::convex_polygon(vec![tip, base + perp * 6.0, base - perp * 6.0], col, Stroke::NONE));
                }
            }
        }

        match &self.drag {
            Drag::Connect { from } => {
                if let (Some(p), Some(src)) = (pointer, self.diagram.node(*from)) {
                    let w = self.to_world(p);
                    let target = self.diagram.node_at(w).filter(|t| t != from).and_then(|t| self.diagram.node(t));
                    let aim = target.map(|t| t.rect().center()).unwrap_or(w);
                    let start = self.to_screen(src.boundary_point(aim));
                    let end = target.map(|t| self.to_screen(t.boundary_point(src.rect().center()))).unwrap_or(p);
                    if let Some(t) = target {
                        painter.rect_stroke(self.rect_to_screen(t.rect()).expand(3.0), 2.0, Stroke::new(2.0, HOVER), StrokeKind::Outside);
                    }
                    painter.extend(Shape::dashed_line(&[start, end], Stroke::new(1.5, SEL), 6.0, 4.0));
                }
            }
            Drag::Rubber { start } => {
                if let Some(p) = pointer {
                    let r = Rect::from_two_pos(self.to_screen(*start), p);
                    painter.rect(r, 0.0, SEL.gamma_multiply(0.1), Stroke::new(1.0, SEL), StrokeKind::Inside);
                }
            }
            _ => {}
        }
    }

    /// Outline following the pointer while a shape is dragged from the palette.
    fn paint_palette_ghost(&self, ui: &Ui, pointer: Option<Pos2>) {
        let (Some(kind), Some(p)) = (self.palette_drag, pointer) else { return };
        let painter = ui.ctx().layer_painter(LayerId::new(Order::Foreground, EguiId::unique("palette_ghost")));
        let r = Rect::from_center_size(p, kind.default_size() * self.zoom);
        paint_shape(&painter, kind, r, SEL.gamma_multiply(0.08), Stroke::new(1.0, SEL), self.zoom);
    }

    fn text_editor(&mut self, ui: &mut Ui) {
        let Some(id) = self.editing else { return };
        let z = self.zoom;
        let (rect, font_size) = if let Some(n) = self.diagram.node(id) {
            let sr = self.rect_to_screen(n.rect());
            let h = (n.font_size * z * 1.6).max(20.0);
            (Rect::from_center_size(sr.center(), vec2(sr.width().max(60.0), sr.height().max(h))), n.font_size * z)
        } else if let Some(path) = self.diagram.edge(id).and_then(|e| self.diagram.edge_path(e)) {
            let sp: Vec<Pos2> = path.iter().map(|p| self.to_screen(*p)).collect();
            (Rect::from_center_size(polyline_midpoint(&sp), vec2(160.0, 28.0)), 11.0 * z)
        } else {
            self.editing = None;
            return;
        };

        let text = if let Some(n) = self.diagram.node_mut(id) {
            &mut n.text
        } else {
            &mut self.diagram.edge_mut(id).unwrap().label
        };
        let edit = egui::TextEdit::multiline(text)
            .font(FontId::proportional(font_size.max(8.0)))
            .align(egui::Align2::CENTER_CENTER)
            .desired_width(rect.width() - 8.0)
            .desired_rows(1);
        let r = ui.put(rect, edit);
        if self.edit_focus {
            r.request_focus();
            self.edit_focus = false;
        } else if r.lost_focus() || ui.input(|i| i.key_pressed(Key::Escape)) {
            r.surrender_focus();
            self.editing = None;
        }
    }

    fn context_menu(&mut self, resp: &Response, pointer: Option<Pos2>) {
        if resp.secondary_clicked() {
            if let Some(p) = pointer {
                let w = self.to_world(p);
                match self.diagram.node_at(w).or_else(|| self.diagram.edge_at(w, EDGE_HIT / self.zoom)) {
                    Some(id) if !self.selection.contains(&id) => self.select_only(id),
                    Some(_) => {}
                    None => self.selection.clear(),
                }
            }
        }
        resp.context_menu(|ui| {
            if self.selection.is_empty() {
                if ui.button("Paste").clicked() {
                    self.paste_internal();
                }
                if ui.button("Select all").clicked() {
                    self.select_all();
                }
                return;
            }
            if self.selection.len() == 1 && ui.button("Edit text").clicked() {
                let id = *self.selection.iter().next().unwrap();
                self.start_editing(id);
            }
            if ui.button("Duplicate").clicked() {
                self.duplicate();
            }
            if ui.button("Copy").clicked() {
                self.copy(ui.ctx());
            }
            if ui.button("Delete").clicked() {
                self.delete_selected();
            }
            ui.separator();
            if ui.button("To front").clicked() {
                self.diagram.reorder(&self.selection, true);
            }
            if ui.button("To back").clicked() {
                self.diagram.reorder(&self.selection, false);
            }
        });
    }

    pub fn select_all(&mut self) {
        self.selection = self
            .diagram
            .nodes
            .iter()
            .map(|n| n.id)
            .chain(self.diagram.edges.iter().map(|e| e.id))
            .collect::<BTreeSet<_>>();
    }
}
