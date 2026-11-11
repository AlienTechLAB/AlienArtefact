//! Diagram data model and geometry helpers. All coordinates here are in
//! "world" units (diagram space), independent of pan/zoom.

use std::collections::{BTreeSet, HashMap};

use egui::{pos2, vec2, Color32, Pos2, Rect, Vec2};
use serde::{Deserialize, Serialize};

pub type Id = u64;
/// Unmultiplied sRGBA.
pub type Rgba = [u8; 4];

pub const BLACK: Rgba = [0, 0, 0, 255];
pub const WHITE: Rgba = [255, 255, 255, 255];
pub const NONE: Rgba = [255, 255, 255, 0];

pub fn color(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeKind {
    Rectangle,
    Rounded,
    Ellipse,
    Diamond,
    Triangle,
    Hexagon,
    Parallelogram,
    Cylinder,
    Text,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 9] = [
        ShapeKind::Rectangle,
        ShapeKind::Rounded,
        ShapeKind::Ellipse,
        ShapeKind::Diamond,
        ShapeKind::Triangle,
        ShapeKind::Hexagon,
        ShapeKind::Parallelogram,
        ShapeKind::Cylinder,
        ShapeKind::Text,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::Rounded => "Rounded rectangle",
            ShapeKind::Ellipse => "Ellipse",
            ShapeKind::Diamond => "Diamond",
            ShapeKind::Triangle => "Triangle",
            ShapeKind::Hexagon => "Hexagon",
            ShapeKind::Parallelogram => "Parallelogram",
            ShapeKind::Cylinder => "Cylinder",
            ShapeKind::Text => "Text",
        }
    }

    pub fn default_size(self) -> Vec2 {
        match self {
            ShapeKind::Ellipse => vec2(120.0, 80.0),
            ShapeKind::Diamond => vec2(100.0, 80.0),
            ShapeKind::Triangle => vec2(80.0, 70.0),
            ShapeKind::Hexagon => vec2(120.0, 80.0),
            ShapeKind::Cylinder => vec2(60.0, 80.0),
            ShapeKind::Text => vec2(80.0, 30.0),
            _ => vec2(120.0, 60.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: Id,
    pub kind: ShapeKind,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub text: String,
    pub fill: Rgba,
    pub stroke: Rgba,
    pub stroke_width: f32,
    pub text_color: Rgba,
    pub font_size: f32,
}

impl Node {
    pub fn new(id: Id, kind: ShapeKind, center: Pos2) -> Self {
        let size = kind.default_size();
        let min = center - size / 2.0;
        let is_text = kind == ShapeKind::Text;
        Self {
            id,
            kind,
            pos: [min.x, min.y],
            size: [size.x, size.y],
            text: if is_text { "Text".into() } else { String::new() },
            fill: if is_text { NONE } else { WHITE },
            stroke: if is_text { NONE } else { BLACK },
            stroke_width: 1.0,
            text_color: BLACK,
            font_size: 12.0,
        }
    }

    pub fn rect(&self) -> Rect {
        Rect::from_min_size(pos2(self.pos[0], self.pos[1]), vec2(self.size[0], self.size[1]))
    }

    pub fn set_rect(&mut self, r: Rect) {
        self.pos = [r.min.x, r.min.y];
        self.size = [r.width(), r.height()];
    }

    /// Point on the shape outline in the direction of `toward`, as seen from the center.
    pub fn boundary_point(&self, toward: Pos2) -> Pos2 {
        let r = self.rect();
        let c = r.center();
        let d = toward - c;
        if d.length_sq() < 1e-6 {
            return c;
        }
        match outline(self.kind, r) {
            None => {
                let (hw, hh) = (r.width() / 2.0, r.height() / 2.0);
                let k = ((d.x / hw).powi(2) + (d.y / hh).powi(2)).sqrt();
                if k > 0.0 { c + d / k } else { c }
            }
            Some(pts) => {
                let mut best = f32::INFINITY;
                for i in 0..pts.len() {
                    let a = pts[i];
                    let e = pts[(i + 1) % pts.len()] - a;
                    let denom = cross(d, e);
                    if denom.abs() < 1e-9 {
                        continue;
                    }
                    let ac = a - c;
                    let t = cross(ac, e) / denom;
                    let s = cross(ac, d) / denom;
                    if t > 0.0 && (-1e-4..=1.0 + 1e-4).contains(&s) && t < best {
                        best = t;
                    }
                }
                if best.is_finite() { c + d * best } else { c }
            }
        }
    }
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// Polygon outline of a shape inside `r` (clockwise on screen), or `None` for ellipses.
pub fn outline(kind: ShapeKind, r: Rect) -> Option<Vec<Pos2>> {
    let p = |x: f32, y: f32| pos2(r.min.x + x * r.width(), r.min.y + y * r.height());
    Some(match kind {
        ShapeKind::Ellipse => return None,
        ShapeKind::Diamond => vec![p(0.5, 0.0), p(1.0, 0.5), p(0.5, 1.0), p(0.0, 0.5)],
        ShapeKind::Triangle => vec![p(0.5, 0.0), p(1.0, 1.0), p(0.0, 1.0)],
        ShapeKind::Hexagon => vec![
            p(0.25, 0.0),
            p(0.75, 0.0),
            p(1.0, 0.5),
            p(0.75, 1.0),
            p(0.25, 1.0),
            p(0.0, 0.5),
        ],
        ShapeKind::Parallelogram => vec![p(0.2, 0.0), p(1.0, 0.0), p(0.8, 1.0), p(0.0, 1.0)],
        _ => vec![p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0), p(0.0, 1.0)],
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeStyle {
    Orthogonal,
    Straight,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: Id,
    pub from: Id,
    pub to: Id,
    pub label: String,
    pub color: Rgba,
    pub width: f32,
    pub dashed: bool,
    pub start_arrow: bool,
    pub end_arrow: bool,
    pub style: EdgeStyle,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Diagram {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub next_id: Id,
}

/// Subset of a diagram, used for copy/paste and duplicate.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Clip {
    /// Marker so arbitrary JSON in the clipboard isn't mistaken for a clip.
    pub architect_ai_clip: u32,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Diagram {
    pub fn alloc_id(&mut self) -> Id {
        self.next_id += 1;
        self.next_id
    }

    pub fn node(&self, id: Id) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: Id) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn edge(&self, id: Id) -> Option<&Edge> {
        self.edges.iter().find(|e| e.id == id)
    }

    pub fn edge_mut(&mut self, id: Id) -> Option<&mut Edge> {
        self.edges.iter_mut().find(|e| e.id == id)
    }

    pub fn add_node(&mut self, kind: ShapeKind, center: Pos2) -> Id {
        let id = self.alloc_id();
        self.nodes.push(Node::new(id, kind, center));
        id
    }

    pub fn add_edge(&mut self, from: Id, to: Id) -> Id {
        let id = self.alloc_id();
        self.edges.push(Edge {
            id,
            from,
            to,
            label: String::new(),
            color: BLACK,
            width: 1.0,
            dashed: false,
            start_arrow: false,
            end_arrow: true,
            style: EdgeStyle::Orthogonal,
        });
        id
    }

    /// Removes nodes/edges with the given ids, plus edges attached to removed nodes.
    pub fn remove(&mut self, ids: &BTreeSet<Id>) {
        self.nodes.retain(|n| !ids.contains(&n.id));
        self.edges
            .retain(|e| !ids.contains(&e.id) && !ids.contains(&e.from) && !ids.contains(&e.to));
    }

    /// Topmost node containing `p`.
    pub fn node_at(&self, p: Pos2) -> Option<Id> {
        self.nodes.iter().rev().find(|n| n.rect().contains(p)).map(|n| n.id)
    }

    /// Topmost edge within `tol` of `p`.
    pub fn edge_at(&self, p: Pos2, tol: f32) -> Option<Id> {
        self.edges
            .iter()
            .rev()
            .find(|e| {
                self.edge_path(e)
                    .is_some_and(|path| path.windows(2).any(|s| dist_to_segment(p, s[0], s[1]) <= tol))
            })
            .map(|e| e.id)
    }

    /// Route of an edge, from the source outline to the target outline.
    pub fn edge_path(&self, e: &Edge) -> Option<Vec<Pos2>> {
        let a = self.node(e.from)?;
        let b = self.node(e.to)?;
        let (ca, cb) = (a.rect().center(), b.rect().center());
        Some(match e.style {
            EdgeStyle::Straight => vec![a.boundary_point(cb), b.boundary_point(ca)],
            EdgeStyle::Orthogonal => {
                const FAR: f32 = 1e5;
                let d = cb - ca;
                if d.x.abs() >= d.y.abs() {
                    let sx = if d.x >= 0.0 { 1.0 } else { -1.0 };
                    let s = a.boundary_point(ca + vec2(sx * FAR, 0.0));
                    let t = b.boundary_point(cb - vec2(sx * FAR, 0.0));
                    if (s.y - t.y).abs() < 0.5 {
                        vec![s, t]
                    } else {
                        let mx = (s.x + t.x) / 2.0;
                        vec![s, pos2(mx, s.y), pos2(mx, t.y), t]
                    }
                } else {
                    let sy = if d.y >= 0.0 { 1.0 } else { -1.0 };
                    let s = a.boundary_point(ca + vec2(0.0, sy * FAR));
                    let t = b.boundary_point(cb - vec2(0.0, sy * FAR));
                    if (s.x - t.x).abs() < 0.5 {
                        vec![s, t]
                    } else {
                        let my = (s.y + t.y) / 2.0;
                        vec![s, pos2(s.x, my), pos2(t.x, my), t]
                    }
                }
            }
        })
    }

    pub fn bounds(&self) -> Option<Rect> {
        let mut it = self.nodes.iter().map(|n| n.rect());
        let first = it.next()?;
        Some(it.fold(first, |a, b| a.union(b)))
    }

    /// Moves the given items to the end (front) or start (back) of the draw order.
    pub fn reorder(&mut self, ids: &BTreeSet<Id>, front: bool) {
        fn go<T>(v: &mut Vec<T>, sel: impl Fn(&T) -> bool, front: bool) {
            let (rest, chosen): (Vec<T>, Vec<T>) = v.drain(..).partition(|x| !sel(x));
            *v = if front {
                rest.into_iter().chain(chosen).collect()
            } else {
                chosen.into_iter().chain(rest).collect()
            };
        }
        go(&mut self.nodes, |n| ids.contains(&n.id), front);
        go(&mut self.edges, |e| ids.contains(&e.id), front);
    }

    /// Selected nodes plus the edges that run between them.
    pub fn copy_subset(&self, ids: &BTreeSet<Id>) -> Clip {
        Clip {
            architect_ai_clip: 1,
            nodes: self.nodes.iter().filter(|n| ids.contains(&n.id)).cloned().collect(),
            edges: self
                .edges
                .iter()
                .filter(|e| ids.contains(&e.from) && ids.contains(&e.to))
                .cloned()
                .collect(),
        }
    }

    /// Inserts copies of a clip with fresh ids, returning the new ids.
    pub fn paste(&mut self, clip: &Clip, offset: Vec2) -> BTreeSet<Id> {
        let mut map = HashMap::new();
        let mut new = BTreeSet::new();
        for n in &clip.nodes {
            let mut n = n.clone();
            let id = self.alloc_id();
            map.insert(n.id, id);
            n.id = id;
            n.pos[0] += offset.x;
            n.pos[1] += offset.y;
            new.insert(id);
            self.nodes.push(n);
        }
        for e in &clip.edges {
            if let (Some(&from), Some(&to)) = (map.get(&e.from), map.get(&e.to)) {
                let mut e = e.clone();
                e.id = self.alloc_id();
                e.from = from;
                e.to = to;
                new.insert(e.id);
                self.edges.push(e);
            }
        }
        new
    }
}

pub fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq < 1e-9 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Point halfway along a polyline (by length).
pub fn polyline_midpoint(pts: &[Pos2]) -> Pos2 {
    let total: f32 = pts.windows(2).map(|s| s[0].distance(s[1])).sum();
    let mut remaining = total / 2.0;
    for s in pts.windows(2) {
        let len = s[0].distance(s[1]);
        if len >= remaining && len > 0.0 {
            return s[0] + (s[1] - s[0]) * (remaining / len);
        }
        remaining -= len;
    }
    pts.first().copied().unwrap_or(Pos2::ZERO)
}

/// Shortens a polyline so arrowheads fit, and returns the arrowhead triangles.
pub fn arrow_geometry(pts: &[Pos2], len: f32, start: bool, end: bool) -> (Vec<Pos2>, Vec<[Pos2; 3]>) {
    let mut line = pts.to_vec();
    let mut heads = Vec::new();
    let n = line.len();
    if n < 2 {
        return (line, heads);
    }
    let mut head = |tip: Pos2, from: Pos2| -> Option<Pos2> {
        let v = tip - from;
        if v.length() < 0.01 {
            return None;
        }
        let dir = v.normalized();
        let perp = vec2(-dir.y, dir.x);
        let base = tip - dir * len;
        let w = len * 0.4;
        heads.push([tip, base + perp * w, base - perp * w]);
        Some(base)
    };
    if end {
        if let Some(base) = head(line[n - 1], line[n - 2]) {
            line[n - 1] = base;
        }
    }
    if start {
        if let Some(base) = head(line[0], line[1]) {
            line[0] = base;
        }
    }
    (line, heads)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(kind: ShapeKind, x: f32, y: f32, w: f32, h: f32) -> Node {
        let mut n = Node::new(1, kind, Pos2::ZERO);
        n.set_rect(Rect::from_min_size(pos2(x, y), vec2(w, h)));
        n
    }

    fn close(a: Pos2, b: Pos2) -> bool {
        a.distance(b) < 1e-3
    }

    #[test]
    fn boundary_points_lie_on_outline() {
        let r = node(ShapeKind::Rectangle, 0.0, 0.0, 100.0, 50.0);
        assert!(close(r.boundary_point(pos2(1000.0, 25.0)), pos2(100.0, 25.0)));
        assert!(close(r.boundary_point(pos2(50.0, -1000.0)), pos2(50.0, 0.0)));

        let e = node(ShapeKind::Ellipse, 0.0, 0.0, 100.0, 50.0);
        assert!(close(e.boundary_point(pos2(-500.0, 25.0)), pos2(0.0, 25.0)));

        let d = node(ShapeKind::Diamond, 0.0, 0.0, 100.0, 100.0);
        // Toward the corner direction (1,1) the diamond edge is at (75,75).
        assert!(close(d.boundary_point(pos2(200.0, 200.0)), pos2(75.0, 75.0)));
    }

    #[test]
    fn orthogonal_edge_routes_with_elbows() {
        let mut d = Diagram::default();
        let a = d.add_node(ShapeKind::Rectangle, pos2(0.0, 0.0));
        let b = d.add_node(ShapeKind::Rectangle, pos2(400.0, 100.0));
        let e = d.add_edge(a, b);
        let path = d.edge_path(d.edge(e).unwrap()).unwrap();
        assert_eq!(path.len(), 4);
        assert!(close(path[0], pos2(60.0, 0.0)));
        assert!(close(path[3], pos2(340.0, 100.0)));
        // Every segment is axis-aligned.
        assert!(path.windows(2).all(|s| s[0].x == s[1].x || s[0].y == s[1].y));
    }

    #[test]
    fn paste_remaps_ids_and_removal_cascades() {
        let mut d = Diagram::default();
        let a = d.add_node(ShapeKind::Rectangle, pos2(0.0, 0.0));
        let b = d.add_node(ShapeKind::Ellipse, pos2(300.0, 0.0));
        d.add_edge(a, b);
        let clip = d.copy_subset(&[a, b].into_iter().collect());
        let new = d.paste(&clip, vec2(10.0, 10.0));
        assert_eq!(new.len(), 3);
        assert_eq!(d.nodes.len(), 4);
        let pasted_edge = d.edges.last().unwrap();
        assert!(new.contains(&pasted_edge.from) && new.contains(&pasted_edge.to));

        d.remove(&[a].into_iter().collect());
        assert_eq!(d.nodes.len(), 3);
        assert_eq!(d.edges.len(), 1, "edge attached to the removed node goes too");
    }
}
