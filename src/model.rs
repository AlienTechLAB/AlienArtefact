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
    /// Interface (Rust trait, C# interface, C++ pure virtual class): like a class,
    /// with a `«trait»` stereotype above the name.
    Trait,
}

/// Height of an object's header compartment (stereotype + name), for a given font size
/// (same units as `font_size`).
pub fn header_height(kind: ShapeKind, font_size: f32, box_height: f32) -> f32 {
    let lines = if kind.stereotype().is_some() { 3.6 } else { 2.2 };
    (font_size * lines).min(box_height)
}

/// Height of the stereotype band at the top of an object's header.
pub fn stereotype_height(font_size: f32, box_height: f32) -> f32 {
    (font_size * 1.7).min(box_height)
}

impl ShapeKind {
    /// Shapes offered in the "Objects" palette section. The other kinds can no longer be
    /// created, but stay so diagrams that contain them still open and draw.
    pub const OBJECTS: [ShapeKind; 2] = [ShapeKind::Class, ShapeKind::Trait];

    /// Class-like objects: a header with the name, body below.
    pub fn is_object(self) -> bool {
        matches!(self, ShapeKind::Class | ShapeKind::Trait)
    }

    /// UML stereotype shown above the name, e.g. `«trait»`.
    pub fn stereotype(self) -> Option<&'static str> {
        match self {
            ShapeKind::Trait => Some("\u{ab}trait\u{bb}"),
            _ => None,
        }
    }

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
            ShapeKind::Trait => "Trait / interface",
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
            ShapeKind::Class | ShapeKind::Trait => vec2(160.0, 100.0),
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
                ShapeKind::Trait => "TraitName".into(),
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

    /// Area where the shape's text (an object's name) is drawn: below the stereotype in an
    /// object's header, otherwise the whole shape.
    pub fn text_rect(&self) -> Rect {
        let r = self.rect();
        if !self.kind.is_object() {
            return r;
        }
        let header = header_height(self.kind, self.font_size, r.height());
        let top = if self.kind.stereotype().is_some() {
            // Overlaps the stereotype band a little; the name is centered in what remains.
            (self.font_size * 1.3).min(header)
        } else {
            0.0
        };
        Rect::from_min_max(pos2(r.left(), r.top() + top), pos2(r.right(), r.top() + header))
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

/// Decoration at one end of a line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Head {
    #[default]
    None,
    /// Filled triangle.
    Arrow,
    /// Two strokes, like `>`.
    OpenArrow,
    /// Filled diamond (sole ownership).
    Diamond,
    /// Outlined diamond (shared ownership).
    HollowDiamond,
    /// Outlined triangle (UML realization: "implements").
    HollowArrow,
}

impl Head {
    pub const ALL: [Head; 6] =
        [Head::None, Head::Arrow, Head::OpenArrow, Head::Diamond, Head::HollowDiamond, Head::HollowArrow];

    pub fn name(self) -> &'static str {
        match self {
            Head::None => "None",
            Head::Arrow => "Arrow",
            Head::OpenArrow => "Open arrow",
            Head::Diamond => "Diamond",
            Head::HollowDiamond => "Hollow diamond",
            Head::HollowArrow => "Hollow arrow",
        }
    }
}

/// What a pointer line means. Each kind has a fixed look so diagrams read consistently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointerKind {
    Raw,
    Reference,
    Unique,
    Shared,
    Weak,
}

impl PointerKind {
    pub const ALL: [PointerKind; 5] =
        [PointerKind::Raw, PointerKind::Reference, PointerKind::Unique, PointerKind::Shared, PointerKind::Weak];

    pub fn name(self) -> &'static str {
        match self {
            PointerKind::Raw => "Raw pointer",
            PointerKind::Reference => "Reference",
            PointerKind::Unique => "Unique pointer",
            PointerKind::Shared => "Shared pointer",
            PointerKind::Weak => "Weak pointer",
        }
    }

    /// Line pattern, source head, target head.
    pub fn look(self) -> (LinePattern, Head, Head) {
        match self {
            PointerKind::Raw => (LinePattern::Dashed, Head::None, Head::OpenArrow),
            PointerKind::Reference => (LinePattern::Solid, Head::None, Head::OpenArrow),
            PointerKind::Unique => (LinePattern::Solid, Head::Diamond, Head::Arrow),
            PointerKind::Shared => (LinePattern::Solid, Head::HollowDiamond, Head::Arrow),
            PointerKind::Weak => (LinePattern::Dotted, Head::HollowDiamond, Head::OpenArrow),
        }
    }

    /// Short text shown near the source end, e.g. `const *` or `unique_ptr<const>`.
    pub fn badge(self, const_target: bool) -> String {
        let smart = |name: &str| if const_target { format!("{name}<const>") } else { name.to_owned() };
        match self {
            PointerKind::Raw => (if const_target { "const *" } else { "*" }).to_owned(),
            PointerKind::Reference => (if const_target { "const &" } else { "&" }).to_owned(),
            PointerKind::Unique => smart("unique_ptr"),
            PointerKind::Shared => smart("shared_ptr"),
            PointerKind::Weak => smart("weak_ptr"),
        }
    }
}

/// Non-pointer relations between objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelationKind {
    /// Loosely related (not a technical link), e.g. a config file and the struct loaded from it.
    Related,
    /// A class implements a trait. Only valid from a class to a trait.
    Implements,
}

impl RelationKind {
    pub const ALL: [RelationKind; 2] = [RelationKind::Related, RelationKind::Implements];

    pub fn name(self) -> &'static str {
        match self {
            RelationKind::Related => "Related",
            RelationKind::Implements => "Implements",
        }
    }

    /// Line pattern, source head, target head.
    pub fn look(self) -> (LinePattern, Head, Head) {
        match self {
            RelationKind::Related => (LinePattern::Dotted, Head::None, Head::None),
            RelationKind::Implements => (LinePattern::Dashed, Head::None, Head::HollowArrow),
        }
    }
}

/// What a typed line means: a pointer or a relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Pointer(PointerKind),
    Relation(RelationKind),
}

impl LinkKind {
    pub fn name(self) -> &'static str {
        match self {
            LinkKind::Pointer(k) => k.name(),
            LinkKind::Relation(k) => k.name(),
        }
    }
}

/// Fields from older file versions: read so those files still open, never written.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LegacyEdge {
    #[serde(default, skip_serializing)]
    pub dashed: bool,
    #[serde(default, skip_serializing)]
    pub start_arrow: bool,
    #[serde(default, skip_serializing)]
    pub end_arrow: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: Id,
    pub from: End,
    pub to: End,
    pub label: String,
    pub color: Rgba,
    pub width: f32,
    /// Look of plain lines; pointers use their kind's fixed look instead.
    #[serde(default)]
    pub pattern: LinePattern,
    #[serde(default)]
    pub start_head: Head,
    #[serde(default)]
    pub end_head: Head,
    /// `Some` makes this line a pointer relation from `from` to `to`.
    #[serde(default)]
    pub pointer: Option<PointerKind>,
    /// The pointed-to object is const.
    #[serde(default)]
    pub const_target: bool,
    /// `Some` makes this line a relation (mutually exclusive with `pointer`).
    #[serde(default)]
    pub relation: Option<RelationKind>,
    pub style: EdgeStyle,
    #[serde(flatten, default)]
    pub legacy: LegacyEdge,
}

impl Edge {
    fn new(id: Id, from: End, to: End, style: EdgeStyle) -> Self {
        Self {
            id,
            from,
            to,
            label: String::new(),
            color: BLACK,
            width: 1.0,
            pattern: LinePattern::Solid,
            start_head: Head::None,
            end_head: Head::None,
            pointer: None,
            const_target: false,
            relation: None,
            style,
            legacy: LegacyEdge::default(),
        }
    }

    /// Effective line pattern and end decorations.
    pub fn look(&self) -> (LinePattern, Head, Head) {
        match self.link() {
            Some(LinkKind::Pointer(kind)) => kind.look(),
            Some(LinkKind::Relation(kind)) => kind.look(),
            None => (self.pattern, self.start_head, self.end_head),
        }
    }

    /// Pointer or relation type; `None` for a plain connector.
    pub fn link(&self) -> Option<LinkKind> {
        self.relation.map(LinkKind::Relation).or(self.pointer.map(LinkKind::Pointer))
    }

    pub fn set_link(&mut self, kind: LinkKind) {
        match kind {
            LinkKind::Pointer(k) => {
                self.pointer = Some(k);
                self.relation = None;
            }
            LinkKind::Relation(k) => {
                self.relation = Some(k);
                self.pointer = None;
            }
        }
    }

    pub fn badge(&self) -> Option<String> {
        match self.link() {
            Some(LinkKind::Pointer(k)) => Some(k.badge(self.const_target)),
            _ => None,
        }
    }
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

    /// Adds an orthogonal arrow connector between two shapes.
    pub fn add_edge(&mut self, from: Id, to: Id) -> Id {
        let id = self.alloc_id();
        let mut e = Edge::new(id, End::Node(from), End::Node(to), EdgeStyle::Orthogonal);
        e.end_head = Head::Arrow;
        self.edges.push(e);
        id
    }

    /// Adds a straight pointer line.
    #[cfg(test)]
    pub fn add_pointer(&mut self, from: End, to: End, kind: PointerKind, const_target: bool) -> Id {
        self.add_link(from, to, LinkKind::Pointer(kind), const_target)
    }

    /// Adds a straight pointer or relation line. `const_target` only matters for pointers.
    pub fn add_link(&mut self, from: End, to: End, kind: LinkKind, const_target: bool) -> Id {
        let id = self.alloc_id();
        let mut e = Edge::new(id, from, to, EdgeStyle::Straight);
        e.set_link(kind);
        e.const_target = const_target;
        self.edges.push(e);
        id
    }

    fn node_kind(&self, end: End) -> Option<ShapeKind> {
        end.node().and_then(|id| self.node(id)).map(|n| n.kind)
    }

    /// Whether a line of this kind may start at `from` (checked before the target is known).
    pub fn link_can_start(&self, kind: LinkKind, from: End) -> bool {
        match kind {
            LinkKind::Relation(RelationKind::Implements) => self.node_kind(from) == Some(ShapeKind::Class),
            _ => true,
        }
    }

    /// Whether a line of this kind may connect `from` to `to`.
    pub fn link_allowed(&self, kind: LinkKind, from: End, to: End) -> bool {
        match kind {
            LinkKind::Relation(RelationKind::Implements) => {
                self.node_kind(from) == Some(ShapeKind::Class) && self.node_kind(to) == Some(ShapeKind::Trait)
            }
            _ => true,
        }
    }

    /// Converts fields from older file versions.
    pub fn migrate(&mut self) {
        for e in &mut self.edges {
            let old = std::mem::take(&mut e.legacy);
            if old.dashed {
                e.pattern = LinePattern::Dashed;
            }
            if old.start_arrow {
                e.start_head = Head::Arrow;
            }
            if old.end_arrow {
                e.end_head = Head::Arrow;
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
        if let (End::Node(a), End::Node(b)) = (e.from, e.to) {
            if a == b {
                return self.node(a).map(self_loop_path);
            }
        }
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

/// Route for a line from a shape back to itself: a rectangular loop off the right side,
/// leaving above the middle and coming back in below it.
pub fn self_loop_path(n: &Node) -> Vec<Pos2> {
    let r = n.rect();
    let c = r.center();
    let d = (r.height() / 4.0).min(20.0);
    let out = r.right() + 40.0;
    // Aim at the right edge at the loop's heights (exact for boxes, close for other outlines).
    let start = n.boundary_point(pos2(r.right(), c.y - d));
    let end = n.boundary_point(pos2(r.right(), c.y + d));
    vec![start, pos2(out, start.y), pos2(out, end.y), end]
}

/// Point `dist` along a polyline (clamped to its ends).
pub fn polyline_point_at(pts: &[Pos2], dist: f32) -> Pos2 {
    let mut remaining = dist.max(0.0);
    for s in pts.windows(2) {
        let len = s[0].distance(s[1]);
        if len >= remaining && len > 0.0 {
            return s[0] + (s[1] - s[0]) * (remaining / len);
        }
        remaining -= len;
    }
    pts.last().copied().unwrap_or(Pos2::ZERO)
}

/// Point halfway along a polyline (by length).
pub fn polyline_midpoint(pts: &[Pos2]) -> Pos2 {
    let total: f32 = pts.windows(2).map(|s| s[0].distance(s[1])).sum();
    polyline_point_at(pts, total / 2.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadFill {
    /// Filled with the line color.
    Solid,
    /// Outline with background fill.
    Hollow,
    /// Open polyline.
    Open,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HeadShape {
    pub points: Vec<Pos2>,
    pub fill: HeadFill,
}

/// Shortens a polyline so the end decorations fit, and returns their shapes.
/// `len` is the arrowhead length; diamonds are twice as long.
pub fn edge_geometry(pts: &[Pos2], len: f32, start: Head, end: Head) -> (Vec<Pos2>, Vec<HeadShape>) {
    let mut line = pts.to_vec();
    let mut heads = Vec::new();
    let n = line.len();
    if n < 2 {
        return (line, heads);
    }
    let mut head = |tip: Pos2, from: Pos2, kind: Head| -> Option<Pos2> {
        let v = tip - from;
        if kind == Head::None || v.length() < 0.01 {
            return None;
        }
        let dir = v.normalized();
        let perp = vec2(-dir.y, dir.x);
        let w = len * 0.4;
        let base = tip - dir * len;
        let (points, fill, line_end) = match kind {
            Head::Arrow => (vec![tip, base + perp * w, base - perp * w], HeadFill::Solid, base),
            Head::HollowArrow => {
                let w = len * 0.55;
                (vec![tip, base + perp * w, base - perp * w], HeadFill::Hollow, base)
            }
            Head::OpenArrow => (vec![base + perp * w, tip, base - perp * w], HeadFill::Open, tip),
            Head::Diamond | Head::HollowDiamond => {
                let back = tip - dir * len * 2.0;
                let fill = if kind == Head::Diamond { HeadFill::Solid } else { HeadFill::Hollow };
                (vec![tip, base + perp * w, back, base - perp * w], fill, back)
            }
            Head::None => return None,
        };
        heads.push(HeadShape { points, fill });
        Some(line_end)
    };
    if let Some(p) = head(line[n - 1], line[n - 2], end) {
        line[n - 1] = p;
    }
    if let Some(p) = head(line[0], line[1], start) {
        line[0] = p;
    }
    (line, heads)
}

/// Center for a line's label of the given size: the midpoint for plain lines; for pointers, near
/// the target end on the opposite side from the badge (where UML puts member/role names).
pub fn label_center(pointer: bool, pts: &[Pos2], len: f32, size: Vec2) -> Pos2 {
    if pointer {
        let reversed: Vec<Pos2> = pts.iter().rev().copied().collect();
        badge_center(&reversed, len, size, false)
    } else {
        polyline_midpoint(pts)
    }
}

/// Center for a pointer's badge of the given size: just past the source decoration (which is at
/// most `2 * len` long), beside the line (above it, or to the right of vertical lines) so it
/// covers neither the decoration nor the line's own label. `above = false` puts it on the other side.
pub fn badge_center(pts: &[Pos2], len: f32, size: Vec2, above: bool) -> Pos2 {
    if pts.len() < 2 {
        return pts.first().copied().unwrap_or(Pos2::ZERO);
    }
    let total: f32 = pts.windows(2).map(|s| s[0].distance(s[1])).sum();
    let start = (len * 2.0 + 4.0).min(total / 2.0);
    let at = polyline_point_at(pts, start);
    // Direction of the segment containing `at`.
    let mut walked = 0.0;
    let mut dir = (pts[1] - pts[0]).normalized();
    for s in pts.windows(2) {
        let l = s[0].distance(s[1]);
        if walked + l >= start && l > 0.0 {
            dir = (s[1] - s[0]) / l;
            break;
        }
        walked += l;
    }
    if !dir.x.is_finite() {
        return at;
    }
    let mut normal = vec2(-dir.y, dir.x);
    if normal.y > 0.1 || (normal.y.abs() <= 0.1 && normal.x < 0.0) {
        normal = -normal;
    }
    if !above {
        normal = -normal;
    }
    let half = size / 2.0;
    let along = half.x * dir.x.abs() + half.y * dir.y.abs();
    let across = half.x * normal.x.abs() + half.y * normal.y.abs() + 3.0;
    at + dir * along + normal * across
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
        let free = d.add_pointer(End::point(pos2(200.0, 0.0)), End::point(pos2(300.0, 50.0)), PointerKind::Raw, false);
        let path = d.edge_path(d.edge(free).unwrap()).unwrap();
        assert_eq!(path, vec![pos2(200.0, 0.0), pos2(300.0, 50.0)]);

        // Half-attached: starts at the shape's outline, aimed at the free end.
        let half = d.add_pointer(End::Node(n), End::point(pos2(200.0, 0.0)), PointerKind::Unique, false);
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
        d.add_pointer(End::Node(n), End::point(pos2(5.0, 6.0)), PointerKind::Shared, true);
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains(r#""from":1"#) && json.contains(r#""to":[5.0,6.0]"#), "{json}");
        assert!(!json.contains("dashed") && !json.contains("end_arrow"));
        assert!(json.contains(r#""pointer":"Shared""#) && json.contains(r#""const_target":true"#), "{json}");
        let back: Diagram = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);

        // Format before line patterns: plain ids and a `dashed` flag.
        let old = r#"{"nodes":[],"edges":[{"id":3,"from":1,"to":2,"label":"","color":[0,0,0,255],
            "width":1.0,"dashed":true,"start_arrow":false,"end_arrow":true,"style":"Orthogonal"}],"next_id":3}"#;
        let mut d: Diagram = serde_json::from_str(old).unwrap();
        d.migrate();
        assert_eq!(d.edges[0].from, End::Node(1));
        assert_eq!(d.edges[0].pattern, LinePattern::Dashed);
        assert_eq!(d.edges[0].end_head, Head::Arrow);
        assert_eq!(d.edges[0].start_head, Head::None);
        assert_eq!(d.edges[0].pointer, None);
    }

    #[test]
    fn copy_includes_selected_free_lines_only() {
        let mut d = Diagram::default();
        let a = d.add_pointer(End::point(pos2(0.0, 0.0)), End::point(pos2(10.0, 0.0)), PointerKind::Reference, false);
        let _b = d.add_pointer(End::point(pos2(0.0, 9.0)), End::point(pos2(10.0, 9.0)), PointerKind::Reference, false);
        let clip = d.copy_subset(&[a].into_iter().collect());
        assert_eq!(clip.edges.len(), 1);
        let new = d.paste(&clip, vec2(5.0, 5.0));
        let pasted = d.edge(*new.iter().next().unwrap()).unwrap();
        assert_eq!(pasted.from, End::Point([5.0, 5.0]));
    }
}

#[cfg(test)]
mod pointer_tests {
    use super::*;

    #[test]
    fn pointer_kinds_have_distinct_looks_and_badges() {
        let looks: Vec<_> = PointerKind::ALL.iter().map(|k| k.look()).collect();
        for (i, a) in looks.iter().enumerate() {
            for b in &looks[i + 1..] {
                assert_ne!(a, b, "two pointer kinds look the same");
            }
        }
        assert_eq!(PointerKind::Raw.badge(true), "const *");
        assert_eq!(PointerKind::Reference.badge(false), "&");
        assert_eq!(PointerKind::Unique.badge(true), "unique_ptr<const>");
    }

    #[test]
    fn pointer_look_overrides_plain_line_settings() {
        let mut d = Diagram::default();
        let id = d.add_pointer(End::point(pos2(0.0, 0.0)), End::point(pos2(100.0, 0.0)), PointerKind::Unique, false);
        let e = d.edge_mut(id).unwrap();
        e.pattern = LinePattern::Dotted; // ignored while it is a pointer
        assert_eq!(e.look(), (LinePattern::Solid, Head::Diamond, Head::Arrow));
        e.pointer = None;
        assert_eq!(e.look(), (LinePattern::Dotted, Head::None, Head::None));
    }

    #[test]
    fn heads_shorten_the_line_to_their_base() {
        let pts = [pos2(0.0, 0.0), pos2(100.0, 0.0)];
        let (line, heads) = edge_geometry(&pts, 10.0, Head::Diamond, Head::OpenArrow);
        assert_eq!(line[0], pos2(20.0, 0.0), "diamond is two head lengths long");
        assert_eq!(line[1], pos2(100.0, 0.0), "open arrow keeps the line to the tip");
        assert_eq!(heads.len(), 2);
    }
}

#[cfg(test)]
mod self_pointer_tests {
    use super::*;

    #[test]
    fn self_pointer_loops_off_the_right_side() {
        let mut d = Diagram::default();
        let n = d.add_node(ShapeKind::Class, pos2(0.0, 0.0)); // 160x100 around origin
        let id = d.add_pointer(End::Node(n), End::Node(n), PointerKind::Raw, false);
        let path = d.edge_path(d.edge(id).unwrap()).unwrap();
        assert_eq!(path.len(), 4);
        // Leaves and re-enters on the right edge, at different heights, with the loop outside.
        assert!((path[0].x - 80.0).abs() < 1e-3 && (path[3].x - 80.0).abs() < 1e-3);
        assert!(path[0].y < path[3].y);
        assert!(path[1].x > 80.0 && path[2].x > 80.0);
        assert!((path[3].y - path[0].y - 40.0).abs() < 1e-3, "loop spans 2 * 20 units");
        // Selectable by clicking the loop.
        assert_eq!(d.edge_at(pos2(120.0, 0.0), 3.0), Some(id));
    }
}

#[cfg(test)]
mod trait_tests {
    use super::*;

    #[test]
    fn trait_name_sits_below_its_stereotype() {
        let t = Node::new(1, ShapeKind::Trait, pos2(0.0, 0.0));
        let c = Node::new(2, ShapeKind::Class, pos2(0.0, 0.0));
        assert_eq!(t.text, "TraitName");
        assert!(t.kind.is_object() && t.kind.stereotype().is_some() && c.kind.stereotype().is_none());
        let (tr, cr) = (t.text_rect(), c.text_rect());
        // The name band starts below the top (stereotype above it) and the header is taller.
        assert!(tr.top() > t.rect().top());
        assert!(tr.bottom() > cr.bottom());
        assert!(tr.height() >= cr.height() * 0.9, "name band keeps about a line and a half");
    }

    #[test]
    fn trait_round_trips_and_accepts_pointers() {
        let mut d = Diagram::default();
        let c = d.add_node(ShapeKind::Class, pos2(0.0, 0.0));
        let t = d.add_node(ShapeKind::Trait, pos2(300.0, 0.0));
        d.add_pointer(End::Node(c), End::Node(t), PointerKind::Unique, false);
        let back: Diagram = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
        assert!(d.edge_path(&d.edges[0]).is_some());
    }
}

#[cfg(test)]
mod relation_tests {
    use super::*;

    const IMPL: LinkKind = LinkKind::Relation(RelationKind::Implements);
    const RELATED: LinkKind = LinkKind::Relation(RelationKind::Related);

    #[test]
    fn implements_only_from_class_to_trait() {
        let mut d = Diagram::default();
        let c = End::Node(d.add_node(ShapeKind::Class, pos2(0.0, 0.0)));
        let c2 = End::Node(d.add_node(ShapeKind::Class, pos2(300.0, 0.0)));
        let t = End::Node(d.add_node(ShapeKind::Trait, pos2(0.0, 300.0)));
        let free = End::point(pos2(500.0, 500.0));

        assert!(d.link_allowed(IMPL, c, t));
        assert!(!d.link_allowed(IMPL, t, c), "trait to class");
        assert!(!d.link_allowed(IMPL, c, c2), "class to class");
        assert!(!d.link_allowed(IMPL, c, free), "dangling end");
        assert!(d.link_can_start(IMPL, c));
        assert!(!d.link_can_start(IMPL, t));
        assert!(!d.link_can_start(IMPL, free));

        // Related and pointers connect anything.
        assert!(d.link_allowed(RELATED, t, free));
        assert!(d.link_allowed(LinkKind::Pointer(PointerKind::Raw), t, c));
    }

    #[test]
    fn relation_looks_and_kind_switching() {
        let mut d = Diagram::default();
        let id = d.add_link(End::point(pos2(0.0, 0.0)), End::point(pos2(100.0, 0.0)), RELATED, true);
        let e = d.edge_mut(id).unwrap();
        assert_eq!(e.look(), (LinePattern::Dotted, Head::None, Head::None));
        assert_eq!(e.badge(), None, "relations have no pointer badge");

        e.set_link(LinkKind::Pointer(PointerKind::Shared));
        assert_eq!((e.pointer, e.relation), (Some(PointerKind::Shared), None));
        e.set_link(IMPL);
        assert_eq!((e.pointer, e.relation), (None, Some(RelationKind::Implements)));
        assert_eq!(e.look(), (LinePattern::Dashed, Head::None, Head::HollowArrow));

        // Each pointer/relation kind looks different from every other.
        let looks: Vec<_> = PointerKind::ALL
            .iter()
            .map(|k| k.look())
            .chain(RelationKind::ALL.iter().map(|k| k.look()))
            .collect();
        for (i, a) in looks.iter().enumerate() {
            assert!(!looks[i + 1..].contains(a));
        }
    }

    #[test]
    fn relation_field_round_trips_and_old_files_default_to_none() {
        let mut d = Diagram::default();
        let c = End::Node(d.add_node(ShapeKind::Class, pos2(0.0, 0.0)));
        let t = End::Node(d.add_node(ShapeKind::Trait, pos2(0.0, 300.0)));
        d.add_link(c, t, IMPL, false);
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains(r#""relation":"Implements""#), "{json}");
        let back: Diagram = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);

        let old = json.replace(r#","relation":"Implements""#, "");
        let back: Diagram = serde_json::from_str(&old).unwrap();
        assert_eq!(back.edges[0].relation, None);
    }
}
