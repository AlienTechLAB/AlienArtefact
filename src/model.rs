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
    /// UML-style class/struct box: name in a header compartment, body below.
    Class,
}

/// Height of a class box's name compartment, for a given font size (same units as `font_size`).
pub fn class_header_height(font_size: f32, box_height: f32) -> f32 {
    (font_size * 2.2).min(box_height)
}

impl ShapeKind {
    /// Shapes in the "UML" palette section.
    pub const UML: [ShapeKind; 1] = [ShapeKind::Class];

    /// Shapes in the "General" palette section.
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
            ShapeKind::Class => "Class / struct",
        }
    }

    /// Every shape kind, for pickers.
    pub fn every() -> impl Iterator<Item = ShapeKind> {
        Self::ALL.into_iter().chain(Self::UML)
    }

    pub fn default_size(self) -> Vec2 {
        match self {
            ShapeKind::Ellipse => vec2(120.0, 80.0),
            ShapeKind::Diamond => vec2(100.0, 80.0),
            ShapeKind::Triangle => vec2(80.0, 70.0),
            ShapeKind::Hexagon => vec2(120.0, 80.0),
            ShapeKind::Cylinder => vec2(60.0, 80.0),
            ShapeKind::Text => vec2(80.0, 30.0),
            ShapeKind::Class => vec2(160.0, 100.0),
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
            text: match kind {
                ShapeKind::Text => "Text".into(),
                ShapeKind::Class => "ClassName".into(),
                _ => String::new(),
            },
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

    /// Area where the shape's text is drawn: the name compartment for classes, otherwise the whole shape.
    pub fn text_rect(&self) -> Rect {
        let r = self.rect();
        if self.kind == ShapeKind::Class {
            Rect::from_min_size(r.min, vec2(r.width(), class_header_height(self.font_size, r.height())))
        } else {
            r
        }
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinePattern {
    #[default]
    Solid,
    Dashed,
    Dotted,
    DashDot,
}

impl LinePattern {
    pub const ALL: [LinePattern; 4] = [LinePattern::Solid, LinePattern::Dashed, LinePattern::Dotted, LinePattern::DashDot];

    pub fn name(self) -> &'static str {
        match self {
            LinePattern::Solid => "Solid",
            LinePattern::Dashed => "Dashed",
            LinePattern::Dotted => "Dotted",
            LinePattern::DashDot => "Dash-dot",
        }
    }

    /// Alternating dash and gap lengths for a line of width `w` (empty for solid).
    pub fn dashes(self, w: f32) -> (Vec<f32>, Vec<f32>) {
        let w = w.max(1.0);
        match self {
            LinePattern::Solid => (vec![], vec![]),
            LinePattern::Dashed => (vec![6.0 * w], vec![4.0 * w]),
            LinePattern::Dotted => (vec![w], vec![2.0 * w]),
            LinePattern::DashDot => (vec![8.0 * w, w], vec![3.0 * w, 3.0 * w]),
        }
    }
}

/// One end of a line: attached to a shape, or a free point on the canvas.
/// Untagged so a node end serializes as a plain id and a free end as `[x, y]`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum End {
    Node(Id),
    Point([f32; 2]),
}

impl End {
    pub fn point(p: Pos2) -> Self {
        End::Point([p.x, p.y])
    }

    pub fn node(self) -> Option<Id> {
        match self {
            End::Node(id) => Some(id),
            End::Point(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: Id,
    pub from: End,
    pub to: End,
    pub label: String,
    pub color: Rgba,
    pub width: f32,
    #[serde(default)]
    pub pattern: LinePattern,
    /// Pre-pattern files stored `dashed: true`; read it so they still open, never write it.
    #[serde(default, skip_serializing)]
    pub dashed: bool,
    pub start_arrow: bool,
    pub end_arrow: bool,
    pub style: EdgeStyle,
}

/// Line presets offered in the palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinePreset {
    pub name: &'static str,
    pub start_arrow: bool,
    pub end_arrow: bool,
    pub pattern: LinePattern,
}

pub const LINE_PRESETS: [LinePreset; 5] = [
    LinePreset { name: "Line", start_arrow: false, end_arrow: false, pattern: LinePattern::Solid },
    LinePreset { name: "Arrow", start_arrow: false, end_arrow: true, pattern: LinePattern::Solid },
    LinePreset { name: "Double arrow", start_arrow: true, end_arrow: true, pattern: LinePattern::Solid },
    LinePreset { name: "Dashed arrow", start_arrow: false, end_arrow: true, pattern: LinePattern::Dashed },
    LinePreset { name: "Dotted line", start_arrow: false, end_arrow: false, pattern: LinePattern::Dotted },
];

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

    /// Adds an orthogonal arrow connector between two shapes.
    pub fn add_edge(&mut self, from: Id, to: Id) -> Id {
        let id = self.alloc_id();
        self.edges.push(Edge {
            id,
            from: End::Node(from),
            to: End::Node(to),
            label: String::new(),
            color: BLACK,
            width: 1.0,
            pattern: LinePattern::Solid,
            dashed: false,
            start_arrow: false,
            end_arrow: true,
            style: EdgeStyle::Orthogonal,
        });
        id
    }

    /// Adds a straight line from a preset.
    pub fn add_line(&mut self, from: End, to: End, preset: &LinePreset) -> Id {
        let id = self.alloc_id();
        self.edges.push(Edge {
            id,
            from,
            to,
            label: String::new(),
            color: BLACK,
            width: 1.0,
            pattern: preset.pattern,
            dashed: false,
            start_arrow: preset.start_arrow,
            end_arrow: preset.end_arrow,
            style: EdgeStyle::Straight,
        });
        id
    }

    /// Converts fields from older file versions.
    pub fn migrate(&mut self) {
        for e in &mut self.edges {
            if std::mem::take(&mut e.dashed) {
                e.pattern = LinePattern::Dashed;
            }
        }
    }

    /// Removes nodes/edges with the given ids, plus edges attached to removed nodes.
    pub fn remove(&mut self, ids: &BTreeSet<Id>) {
        self.nodes.retain(|n| !ids.contains(&n.id));
        let attached = |end: End| end.node().is_some_and(|n| ids.contains(&n));
        self.edges.retain(|e| !ids.contains(&e.id) && !attached(e.from) && !attached(e.to));
    }

    /// Reference point of a line end: shape center or the free point.
    pub fn end_center(&self, end: End) -> Option<Pos2> {
        match end {
            End::Node(id) => self.node(id).map(|n| n.rect().center()),
            End::Point(p) => Some(pos2(p[0], p[1])),
        }
    }

    /// Where a line end touches its shape when aimed at `toward` (the free point itself if unattached).
    fn end_boundary(&self, end: End, toward: Pos2) -> Option<Pos2> {
        match end {
            End::Node(id) => self.node(id).map(|n| n.boundary_point(toward)),
            End::Point(p) => Some(pos2(p[0], p[1])),
        }
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
        let ca = self.end_center(e.from)?;
        let cb = self.end_center(e.to)?;
        let (a, b) = (e.from, e.to);
        Some(match e.style {
            EdgeStyle::Straight => vec![self.end_boundary(a, cb)?, self.end_boundary(b, ca)?],
            EdgeStyle::Orthogonal => {
                const FAR: f32 = 1e5;
                let d = cb - ca;
                if d.x.abs() >= d.y.abs() {
                    let sx = if d.x >= 0.0 { 1.0 } else { -1.0 };
                    let s = self.end_boundary(a, ca + vec2(sx * FAR, 0.0))?;
                    let t = self.end_boundary(b, cb - vec2(sx * FAR, 0.0))?;
                    if (s.y - t.y).abs() < 0.5 {
                        vec![s, t]
                    } else {
                        let mx = (s.x + t.x) / 2.0;
                        vec![s, pos2(mx, s.y), pos2(mx, t.y), t]
                    }
                } else {
                    let sy = if d.y >= 0.0 { 1.0 } else { -1.0 };
                    let s = self.end_boundary(a, ca + vec2(0.0, sy * FAR))?;
                    let t = self.end_boundary(b, cb - vec2(0.0, sy * FAR))?;
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
        let points = self
            .edges
            .iter()
            .filter_map(|e| self.edge_path(e))
            .flatten()
            .map(|p| Rect::from_min_max(p, p));
        let mut it = self.nodes.iter().map(|n| n.rect()).chain(points);
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

    /// Selected nodes, the connectors between them, and selected lines whose attached shapes are
    /// also selected (or that are not attached at all).
    pub fn copy_subset(&self, ids: &BTreeSet<Id>) -> Clip {
        let ends_inside = |e: &Edge| [e.from, e.to].iter().all(|end| end.node().is_none_or(|n| ids.contains(&n)));
        let attached = |e: &Edge| e.from.node().is_some() || e.to.node().is_some();
        Clip {
            architect_ai_clip: 1,
            nodes: self.nodes.iter().filter(|n| ids.contains(&n.id)).cloned().collect(),
            edges: self
                .edges
                .iter()
                .filter(|e| ends_inside(e) && (ids.contains(&e.id) || attached(e)))
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
        let remap = |end: End| -> Option<End> {
            match end {
                End::Node(id) => map.get(&id).map(|&n| End::Node(n)),
                End::Point(p) => Some(End::Point([p[0] + offset.x, p[1] + offset.y])),
            }
        };
        let edges: Vec<Edge> = clip
            .edges
            .iter()
            .filter_map(|e| Some(Edge { from: remap(e.from)?, to: remap(e.to)?, ..e.clone() }))
            .collect();
        for mut e in edges {
            e.id = self.alloc_id();
            new.insert(e.id);
            self.edges.push(e);
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
        assert!(new.contains(&pasted_edge.from.node().unwrap()) && new.contains(&pasted_edge.to.node().unwrap()));

        d.remove(&[a].into_iter().collect());
        assert_eq!(d.nodes.len(), 3);
        assert_eq!(d.edges.len(), 1, "edge attached to the removed node goes too");
    }
}

#[cfg(test)]
mod class_tests {
    use super::*;

    #[test]
    fn class_name_sits_in_header() {
        let n = Node::new(1, ShapeKind::Class, pos2(0.0, 0.0));
        assert_eq!(n.text, "ClassName");
        let (r, t) = (n.rect(), n.text_rect());
        assert_eq!(t.top(), r.top());
        assert!(t.height() < r.height());
        assert_eq!(t.width(), r.width());
    }
}

#[cfg(test)]
mod line_tests {
    use super::*;

    #[test]
    fn free_line_path_and_attach() {
        let mut d = Diagram::default();
        let n = d.add_node(ShapeKind::Rectangle, pos2(0.0, 0.0)); // 120x60 around origin
        let free = d.add_line(End::point(pos2(200.0, 0.0)), End::point(pos2(300.0, 50.0)), &LINE_PRESETS[1]);
        let path = d.edge_path(d.edge(free).unwrap()).unwrap();
        assert_eq!(path, vec![pos2(200.0, 0.0), pos2(300.0, 50.0)]);

        // Half-attached: starts at the shape's outline, aimed at the free end.
        let half = d.add_line(End::Node(n), End::point(pos2(200.0, 0.0)), &LINE_PRESETS[0]);
        let path = d.edge_path(d.edge(half).unwrap()).unwrap();
        assert!(path[0].distance(pos2(60.0, 0.0)) < 1e-3, "{:?}", path[0]);

        // Deleting the shape removes the attached line but keeps the free one.
        d.remove(&[n].into_iter().collect());
        assert!(d.edge(half).is_none());
        assert!(d.edge(free).is_some());
    }

    #[test]
    fn ends_serialize_untagged_and_old_files_load() {
        let mut d = Diagram::default();
        let n = d.add_node(ShapeKind::Rectangle, pos2(0.0, 0.0));
        d.add_line(End::Node(n), End::point(pos2(5.0, 6.0)), &LINE_PRESETS[3]);
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains(r#""from":1"#) && json.contains(r#""to":[5.0,6.0]"#), "{json}");
        assert!(!json.contains("dashed"));
        let back: Diagram = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);

        // Format before line patterns: plain ids and a `dashed` flag.
        let old = r#"{"nodes":[],"edges":[{"id":3,"from":1,"to":2,"label":"","color":[0,0,0,255],
            "width":1.0,"dashed":true,"start_arrow":false,"end_arrow":true,"style":"Orthogonal"}],"next_id":3}"#;
        let mut d: Diagram = serde_json::from_str(old).unwrap();
        d.migrate();
        assert_eq!(d.edges[0].from, End::Node(1));
        assert_eq!(d.edges[0].pattern, LinePattern::Dashed);
    }

    #[test]
    fn copy_includes_selected_free_lines_only() {
        let mut d = Diagram::default();
        let a = d.add_line(End::point(pos2(0.0, 0.0)), End::point(pos2(10.0, 0.0)), &LINE_PRESETS[0]);
        let _b = d.add_line(End::point(pos2(0.0, 9.0)), End::point(pos2(10.0, 9.0)), &LINE_PRESETS[0]);
        let clip = d.copy_subset(&[a].into_iter().collect());
        assert_eq!(clip.edges.len(), 1);
        let new = d.paste(&clip, vec2(5.0, 5.0));
        let pasted = d.edge(*new.iter().next().unwrap()).unwrap();
        assert_eq!(pasted.from, End::Point([5.0, 5.0]));
    }
}
