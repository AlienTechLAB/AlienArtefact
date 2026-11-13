//! The drawing canvas: pan/zoom, selection, moving, resizing, connecting, in-place text editing.

use std::collections::BTreeSet;

use egui::{
    pos2, vec2, Color32, FontId, Id as EguiId, Key, LayerId, Order, Painter, PointerButton,
    Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};

use crate::app::App;
use crate::model::{color, label_center, End, Id, LinkKind, ShapeKind};
use crate::render::{
    canvas_color, grid_color, head_len, layout_centered, paint_badge, paint_edge, paint_members, paint_shape,
    paint_text, tone_fill, tone_frame, tone_ink, Deco,
};

pub const GRID: f32 = 10.0;
const MIN_SIZE: f32 = 10.0;
const HANDLE_HIT: f32 = 7.0;
const EDGE_HIT: f32 = 6.0;
const SEL: Color32 = Color32::from_rgb(0, 120, 215);
const HOVER: Color32 = Color32::from_rgb(41, 182, 242);
const ERROR: Color32 = Color32::from_rgb(214, 39, 40);

/// An incorrect line waiting for the user to acknowledge the error window.
pub struct LinkError {
    pub id: Id,
    /// For an existing line whose end was moved: which end and where it was before.
    /// `None` for a line being drawn, which is removed instead.
    pub restore: Option<(usize, End)>,
    pub message: String,
}

pub enum Drag {
    None,
    /// Moves selected shapes and the free ends of selected lines.
    Move { start: Pos2, orig: Vec<(Id, Pos2)>, edge_orig: Vec<(Id, End, End)> },
    Resize { id: Id, handle: usize, orig: Rect, start: Pos2 },
    Rubber { start: Pos2 },
    /// Dragging one end of a line (0 = start, 1 = end); dropping it on a shape attaches it.
    /// `orig` is where that end was, restored if the drop is not allowed.
    Endpoint { id: Id, end: usize, orig: End },
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

    /// Draws the active diagram and handles its input. Returns true when a mouse button was pressed
    /// on it this frame (the workspace then keeps this diagram active).
    pub fn canvas(&mut self, ui: &mut Ui) -> bool {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let pressed = resp.hovered() && ui.input(|i| i.pointer.any_pressed());
        self.canvas_rect = rect;
        if self.fit_pending {
            self.fit_pending = false;
            self.fit_view();
        }
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

        if self.link_tool.is_some() && resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }

        self.handle_input(ui, &resp, pointer, shift);

        let painter = ui.painter_at(rect);
        self.paint(&painter, pointer);
        self.paint_palette_ghost(ui, pointer);
        self.text_editor(ui);
        self.context_menu(&resp, pointer);
        pressed
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

    /// Screen positions of a line's two ends.
    fn edge_ends_screen(&self, id: Id) -> Option<[Pos2; 2]> {
        let path = self.diagram.edge(id).and_then(|e| self.diagram.edge_path(e))?;
        Some([self.to_screen(path[0]), self.to_screen(*path.last()?)])
    }

    /// End of a line to attach at `w`: the shape under it, or a (snapped) free point.
    fn end_at(&self, w: Pos2, exclude: Option<Id>) -> End {
        match self.diagram.node_at(w).filter(|n| Some(*n) != exclude) {
            Some(n) => End::Node(n),
            None => End::point(self.snap_pos(w)),
        }
    }

    fn begin_drag(&mut self, p: Pos2, shift: bool) {
        let w = self.to_world(p);

        // Pointer/relation tool: start a new line here and drag its end.
        if let Some(kind) = self.link_tool.take() {
            let from = self.end_at(w, None);
            let orig = End::point(w);
            let id = self.diagram.add_link(from, orig, kind, self.pointer_const);
            self.select_only(id);
            self.drag = Drag::Endpoint { id, end: 1, orig };
            return;
        }

        // Endpoint handles of selected lines.
        for &id in &self.selection {
            if let Some(ends) = self.edge_ends_screen(id) {
                if let Some(end) = ends.iter().position(|e| e.distance(p) <= HANDLE_HIT) {
                    let e = self.diagram.edge(id).unwrap();
                    let orig = if end == 0 { e.from } else { e.to };
                    self.drag = Drag::Endpoint { id, end, orig };
                    return;
                }
            }
        }

        if let Some(id) = self.single_selected_node() {
            let orig = self.diagram.node(id).unwrap().rect();
            let sr = self.rect_to_screen(orig);
            if let Some(handle) = handle_positions(sr).iter().position(|h| h.distance(p) <= HANDLE_HIT) {
                self.drag = Drag::Resize { id, handle, orig, start: w };
                return;
            }
        }

        let hit = self.diagram.hit(w, EDGE_HIT / self.zoom);
        if let Some(id) = hit {
            if !self.selection.contains(&id) {
                if !shift {
                    self.selection.clear();
                }
                self.selection.insert(id);
            }
            // A package carries everything inside its frame.
            let mut moving: Vec<Id> = self.selection.iter().copied().collect();
            for id in self.selection.iter() {
                if self.diagram.node(*id).is_some_and(|n| n.kind == ShapeKind::Package) {
                    moving.extend(self.diagram.package_contents(*id));
                }
            }
            moving.sort_unstable();
            moving.dedup();
            let orig = moving
                .iter()
                .filter_map(|id| self.diagram.node(*id).map(|n| (*id, n.rect().min)))
                .collect();
            let edge_orig = self
                .selection
                .iter()
                .filter_map(|id| self.diagram.edge(*id).map(|e| (*id, e.from, e.to)))
                .collect();
            self.drag = Drag::Move { start: w, orig, edge_orig };
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
            Drag::Move { start, orig, edge_orig } => {
                let mut d = w - *start;
                // Snap the first shape (or free line end) to the grid and move the rest by the same amount.
                let free_point = |end: &End| match end {
                    End::Point(p) => Some(pos2(p[0], p[1])),
                    End::Node(_) => None,
                };
                let reference = orig
                    .first()
                    .map(|(_, o)| *o)
                    .or_else(|| edge_orig.iter().find_map(|(_, a, b)| free_point(a).or_else(|| free_point(b))));
                if let Some(o) = reference {
                    let t = o + d;
                    d = pos2(snap(t.x, on), snap(t.y, on)) - o;
                }
                for (id, o) in orig {
                    if let Some(n) = self.diagram.node_mut(*id) {
                        n.pos = [o.x + d.x, o.y + d.y];
                    }
                }
                let shift_end = |end: End| match end {
                    End::Point(p) => End::Point([p[0] + d.x, p[1] + d.y]),
                    attached => attached,
                };
                for (id, a, b) in edge_orig {
                    if let Some(e) = self.diagram.edge_mut(*id) {
                        e.from = shift_end(*a);
                        e.to = shift_end(*b);
                    }
                }
            }
            Drag::Endpoint { id, end, .. } => {
                let (id, end) = (*id, *end);
                let p = End::point(pos2(snap(w.x, on), snap(w.y, on)));
                if let Some(e) = self.diagram.edge_mut(id) {
                    if end == 0 { e.from = p } else { e.to = p }
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
            Drag::Endpoint { id, end, orig } => {
                let Some(e) = self.diagram.edge(id) else { return };
                let other = if end == 0 { e.to } else { e.from };
                // Pointers and relations may be dropped on their own source object: pointers allow
                // it (self-pointer), relations reject it with an explaining error. Only untyped
                // connectors skip their own object.
                let exclude = if e.link().is_some() { None } else { other.node() };
                let new_end = self.end_at(w, exclude);
                if let Some(kind) = e.link() {
                    let (from, to) = if end == 0 { (new_end, other) } else { (other, new_end) };
                    if let Some(message) = self.diagram.link_error_for(Some(id), kind, from, to) {
                        // Show the incorrect line where it was dropped until the user closes the
                        // error window; then it is undone (see `App::dismiss_link_error`).
                        let (old_from, old_to) = if end == 0 { (orig, other) } else { (other, orig) };
                        let restore =
                            self.diagram.link_allowed_for(Some(id), kind, old_from, old_to).then_some((end, orig));
                        let e = self.diagram.edge_mut(id).unwrap();
                        if end == 0 { e.from = new_end } else { e.to = new_end }
                        self.link_error = Some(LinkError { id, restore, message });
                        return;
                    }
                }
                let e = self.diagram.edge_mut(id).unwrap();
                if end == 0 { e.from = new_end } else { e.to = new_end }
                // A line dragged to (almost) nothing gets a default length instead of vanishing.
                if let Some(path) = self.diagram.edge(id).and_then(|e| self.diagram.edge_path(e)) {
                    if path[0].distance(path[path.len() - 1]) < 5.0 {
                        if let End::Point(p) = new_end {
                            let e = self.diagram.edge_mut(id).unwrap();
                            let moved = End::Point([p[0] + 120.0, p[1]]);
                            if end == 0 { e.from = moved } else { e.to = moved }
                        }
                    }
                }
            }
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
            _ => {}
        }
    }

    fn on_click(&mut self, p: Pos2, shift: bool) {
        // Pointer/relation tool + click (no drag): place a default-length horizontal line.
        if let Some(kind) = self.link_tool.take() {
            if let LinkKind::Relation(r) = kind {
                if r.needs_two_objects() {
                    self.status = format!("{}: drag {}.", r.name(), r.rule().to_lowercase());
                    return;
                }
            }
            let w = self.to_world(p);
            let from = self.end_at(w, None);
            let start = self.diagram.end_center(from).unwrap_or(w);
            let to = End::point(self.snap_pos(start + vec2(120.0, 0.0)));
            let id = self.diagram.add_link(from, to, kind, self.pointer_const);
            self.select_only(id);
            return;
        }
        let w = self.to_world(p);
        let hit = self.diagram.hit(w, EDGE_HIT / self.zoom);
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
        let hit = self.diagram.hit(w, EDGE_HIT / self.zoom);
        if let Some(id) = hit {
            self.start_editing(id);
        }
    }

    pub fn start_editing(&mut self, id: Id) {
        self.select_only(id);
        self.editing = Some(id);
        self.edit_focus = true;
        self.name_before_edit = self.diagram.node(id).map(|n| n.text.clone()).unwrap_or_default();
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
        painter.rect_filled(rect, 0.0, canvas_color());

        if self.show_grid {
            let mut step = GRID * z;
            while step < 6.0 {
                step *= 5.0;
            }
            let origin = rect.min + self.pan;
            let grid_stroke = Stroke::new(1.0, grid_color());
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

        // Packages first: they are frames behind the objects they hold.
        let (packages, objects): (Vec<&crate::model::Node>, Vec<&crate::model::Node>) =
            self.diagram.nodes.iter().partition(|n| n.kind == ShapeKind::Package);
        for n in packages.into_iter().chain(objects) {
            let sr = self.rect_to_screen(n.rect());
            let stroke = Stroke::new(n.stroke_width * z, tone_ink(color(n.stroke)));
            let stereotype = n.stereotype_text();
            let deco = Deco { stereotype: stereotype.as_deref(), name: &n.text };
            let fill = if n.kind == ShapeKind::Package { tone_frame(color(n.fill)) } else { tone_fill(color(n.fill)) };
            paint_shape(painter, n.kind, sr, fill, stroke, z, n.font_size * z, deco);
            if self.editing != Some(n.id) {
                let tr = self.rect_to_screen(n.text_rect());
                paint_text(painter, tr, &self.diagram.display_text(n), n.font_size * z, tone_ink(color(n.text_color)));
            }
            if n.kind.is_object() {
                paint_members(painter, n, sr, z, stroke);
            }
        }

        for e in &self.diagram.edges {
            let Some(path) = self.diagram.edge_path(e) else { continue };
            let sp: Vec<Pos2> = path.iter().map(|p| self.to_screen(*p)).collect();
            if self.selection.contains(&e.id) {
                painter.add(Shape::line(sp.clone(), Stroke::new(e.width * z + 6.0, SEL.gamma_multiply(0.25))));
            }
            let (pattern, start, end) = e.look();
            let invalid = self.link_error.as_ref().is_some_and(|err| err.id == e.id);
            let col = if invalid { ERROR } else { tone_ink(color(e.color)) };
            if invalid {
                painter.add(Shape::line(sp.clone(), Stroke::new(e.width * z + 6.0, ERROR.gamma_multiply(0.25))));
            }
            paint_edge(painter, &sp, col, e.width * z, pattern, start, end, z);
            if let Some(badge) = e.badge() {
                let len = head_len(e.width * z, z);
                paint_badge(painter, &sp, len, &badge, 10.0 * z, tone_ink(color(e.color)));
            }
            if !e.label.is_empty() && self.editing != Some(e.id) {
                let galley = layout_centered(painter, &e.label, 11.0 * z, tone_ink(color(e.color)), f32::INFINITY);
                let size = galley.size();
                let mid = label_center(e.pointer.is_some(), &sp, head_len(e.width * z, z), size + vec2(4.0, 2.0));
                painter.rect_filled(Rect::from_center_size(mid, size + vec2(4.0, 2.0)), 0.0, canvas_color());
                painter.galley(pos2(mid.x, mid.y - size.y / 2.0), galley, tone_ink(color(e.color)));
            }
            if self.selection.contains(&e.id) {
                for p in [sp[0], sp[sp.len() - 1]] {
                    painter.rect(Rect::from_center_size(p, vec2(7.0, 7.0)), 0.0, canvas_color(), Stroke::new(1.0, SEL), StrokeKind::Middle);
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
                    painter.rect(Rect::from_center_size(h, vec2(8.0, 8.0)), 1.0, canvas_color(), Stroke::new(1.0, SEL), StrokeKind::Middle);
                }
            }
        }

        match &self.drag {
            Drag::Endpoint { id, end, .. } => {
                // Highlight the shape the end would attach to (only where the line is allowed).
                let edge = self.diagram.edge(*id);
                let other_end = edge.map(|e| if *end == 0 { e.to } else { e.from });
                let other = edge.filter(|e| e.pointer.is_none()).and(other_end).and_then(|o| o.node());
                let target = pointer
                    .and_then(|p| self.diagram.node_at(self.to_world(p)))
                    .filter(|t| Some(*t) != other)
                    .filter(|t| {
                        let (Some(e), Some(o)) = (edge, other_end) else { return true };
                        let Some(kind) = e.link() else { return true };
                        let (from, to) = if *end == 0 { (End::Node(*t), o) } else { (o, End::Node(*t)) };
                        self.diagram.link_allowed_for(Some(e.id), kind, from, to)
                    })
                    .and_then(|t| self.diagram.node(t));
                if let Some(t) = target {
                    painter.rect_stroke(self.rect_to_screen(t.rect()).expand(3.0), 2.0, Stroke::new(2.0, HOVER), StrokeKind::Outside);
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
        let deco = Deco { stereotype: kind.stereotype(), name: kind.name() };
        paint_shape(&painter, kind, r, SEL.gamma_multiply(0.08), Stroke::new(1.0, SEL), self.zoom, 12.0 * self.zoom, deco);
    }

    fn text_editor(&mut self, ui: &mut Ui) {
        let Some(id) = self.editing else { return };
        let z = self.zoom;
        let (rect, font_size, single_line) = if let Some(n) = self.diagram.node(id) {
            let sr = self.rect_to_screen(n.text_rect());
            let h = (n.font_size * z * 1.6).max(20.0);
            let rect = Rect::from_center_size(sr.center(), vec2(sr.width().max(60.0), sr.height().max(h)));
            (rect, n.font_size * z, n.kind.is_named() || n.kind.is_collection())
        } else if let Some((e, path)) = self.diagram.edge(id).and_then(|e| Some((e, self.diagram.edge_path(e)?))) {
            let sp: Vec<Pos2> = path.iter().map(|p| self.to_screen(*p)).collect();
            let size = vec2(160.0, 28.0);
            let center = label_center(e.pointer.is_some(), &sp, head_len(e.width * z, z), size);
            (Rect::from_center_size(center, size), 11.0 * z, true)
        } else {
            self.editing = None;
            return;
        };

        let text = if let Some(n) = self.diagram.node_mut(id) {
            &mut n.text
        } else {
            &mut self.diagram.edge_mut(id).unwrap().label
        };
        // Class names are one line, so Enter finishes editing.
        let edit = if single_line { egui::TextEdit::singleline(text) } else { egui::TextEdit::multiline(text) };
        let edit = edit
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
            // Objects keep a name (collections hold an element type here, checked by Validate).
            if let Some(n) = self.diagram.node_mut(id).filter(|n| n.kind.is_named()) {
                crate::model::keep_name(&mut n.text, &self.name_before_edit, n.kind);
            }
        }
    }

    fn context_menu(&mut self, resp: &Response, pointer: Option<Pos2>) {
        if resp.secondary_clicked() {
            if let Some(p) = pointer {
                let w = self.to_world(p);
                match self.diagram.hit(w, EDGE_HIT / self.zoom) {
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
