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
    /// A file outside the program's memory (config, data, asset, log): a document shape with
    /// a folded corner. Not a type: no members, no pointers.
    File,
    /// Collections: containers of elements of one type. The element type comes from the
    /// collection's one outgoing pointer, or from its `text` (a type outside the diagram).
    /// Unique elements, no order.
    Set,
    /// Ordered, indexable sequence.
    Vector,
    /// Key -> value map; the key type is `Node::key`.
    Dictionary,
    /// Hierarchy: every element has child elements.
    Tree,
    /// First in, first out.
    Queue,
    /// Enumeration: a closed set of variants (Rust enums may carry data per variant).
    Enum,
    /// A type from outside the program (a library), e.g. `tokio::sync::Notify`.
    External,
    /// A crate, module or namespace: a frame that contains objects.
    Package,
    /// A directory of files outside the program's memory.
    Directory,
}

/// Process lifetime of a type's values, shown in the header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    /// Any number of values (the default; not shown).
    #[default]
    Many,
    /// Exactly one per process.
    Singleton,
    /// One per handled request.
    PerRequest,
    /// One per user session.
    PerSession,
}

impl Scope {
    pub const ALL: [Scope; 4] = [Scope::Many, Scope::Singleton, Scope::PerRequest, Scope::PerSession];

    pub fn name(self) -> &'static str {
        match self {
            Scope::Many => "many",
            Scope::Singleton => "singleton",
            Scope::PerRequest => "per request",
            Scope::PerSession => "per session",
        }
    }
}

/// Lock guarding what a pointer reaches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lock {
    #[default]
    None,
    Mutex,
    RwLock,
}

impl Lock {
    pub const ALL: [Lock; 3] = [Lock::None, Lock::Mutex, Lock::RwLock];

    pub fn name(self) -> &'static str {
        match self {
            Lock::None => "none",
            Lock::Mutex => "mutex",
            Lock::RwLock => "rwlock",
        }
    }
}

/// Height of a package frame's name tab.
pub fn package_tab_height(font_size: f32, box_height: f32) -> f32 {
    (font_size * 1.9).min(box_height)
}

/// Width of a package frame's name tab, estimated from the name length.
pub fn package_tab_width(name: &str, font_size: f32, box_width: f32) -> f32 {
    (name.chars().count() as f32 * font_size * 0.62 + font_size * 1.6).max(font_size * 6.0).min(box_width)
}

/// Offset between the two "stacked cards" of a collection shape (world units).
pub fn collection_offset(w: f32, h: f32) -> f32 {
    6.0f32.min(w / 6.0).min(h / 6.0)
}

/// Size of a file shape's folded corner for a shape of size `w` x `h`.
pub fn file_fold(w: f32, h: f32) -> f32 {
    (w.min(h) * 0.24).min(w * 0.5)
}

/// Height of an object's header compartment (stereotype + name), for a given font size
/// (same units as `font_size`).
pub fn header_height(has_stereotype: bool, font_size: f32, box_height: f32) -> f32 {
    let lines = if has_stereotype { 3.6 } else { 2.2 };
    (font_size * lines).min(box_height)
}

/// Height of the stereotype band at the top of an object's header.
pub fn stereotype_height(font_size: f32, box_height: f32) -> f32 {
    (font_size * 1.7).min(box_height)
}

impl ShapeKind {
    /// Shapes offered in the "Objects" palette section. The other kinds can no longer be
    /// created, but stay so diagrams that contain them still open and draw.
    pub const OBJECTS: [ShapeKind; 7] = [
        ShapeKind::Class,
        ShapeKind::Trait,
        ShapeKind::Enum,
        ShapeKind::External,
        ShapeKind::Package,
        ShapeKind::File,
        ShapeKind::Directory,
    ];

    /// Shapes offered in the "Collections" palette section.
    pub const COLLECTIONS: [ShapeKind; 5] =
        [ShapeKind::Set, ShapeKind::Vector, ShapeKind::Dictionary, ShapeKind::Tree, ShapeKind::Queue];

    pub fn is_collection(self) -> bool {
        Self::COLLECTIONS.contains(&self)
    }

    /// Class-like objects (types): a header with the name, members below.
    pub fn is_object(self) -> bool {
        matches!(self, ShapeKind::Class | ShapeKind::Trait | ShapeKind::Enum)
    }

    /// Language elements with a one-line name edited in the right panel.
    pub fn is_named(self) -> bool {
        self.is_object()
            || matches!(self, ShapeKind::File | ShapeKind::Directory | ShapeKind::External | ShapeKind::Package)
    }

    /// The name a new object of this kind gets (for a collection: its element type).
    pub fn default_name(self) -> &'static str {
        match self {
            ShapeKind::Text => "Text",
            ShapeKind::Class => "ClassName",
            ShapeKind::Trait => "TraitName",
            ShapeKind::File => "file.txt",
            ShapeKind::Enum => "EnumName",
            ShapeKind::External => "crate::Type",
            ShapeKind::Package => "package",
            ShapeKind::Directory => "data/",
            k if k.is_collection() => "T",
            _ => "",
        }
    }

    /// Files and directories: things outside the program's memory.
    pub fn is_storage(self) -> bool {
        matches!(self, ShapeKind::File | ShapeKind::Directory)
    }

    /// Kinds that belong to the language (the rest are legacy drawing shapes).
    pub fn is_language(self) -> bool {
        self.is_named() || self.is_collection()
    }

    /// UML stereotype shown above the name, e.g. `«trait»`.
    pub fn stereotype(self) -> Option<&'static str> {
        match self {
            ShapeKind::Trait => Some("\u{ab}trait\u{bb}"),
            ShapeKind::Set => Some("\u{ab}set\u{bb}"),
            ShapeKind::Vector => Some("\u{ab}vector\u{bb}"),
            ShapeKind::Dictionary => Some("\u{ab}dictionary\u{bb}"),
            ShapeKind::Tree => Some("\u{ab}tree\u{bb}"),
            ShapeKind::Queue => Some("\u{ab}queue\u{bb}"),
            ShapeKind::Enum => Some("\u{ab}enum\u{bb}"),
            ShapeKind::External => Some("\u{ab}external\u{bb}"),
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
            ShapeKind::File => "File",
            ShapeKind::Set => "Set",
            ShapeKind::Vector => "Vector",
            ShapeKind::Dictionary => "Dictionary",
            ShapeKind::Tree => "Tree",
            ShapeKind::Queue => "Queue",
            ShapeKind::Enum => "Enum",
            ShapeKind::External => "External type",
            ShapeKind::Package => "Package",
            ShapeKind::Directory => "Directory",
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
            // A page: taller than wide.
            ShapeKind::File => vec2(80.0, 100.0),
            ShapeKind::Enum => vec2(160.0, 100.0),
            ShapeKind::External => vec2(170.0, 64.0),
            ShapeKind::Package => vec2(420.0, 300.0),
            ShapeKind::Directory => vec2(150.0, 70.0),
            ShapeKind::Set | ShapeKind::Vector | ShapeKind::Dictionary | ShapeKind::Tree | ShapeKind::Queue => {
                vec2(140.0, 64.0)
            }
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
    /// Fields and methods of a class or trait, in display order.
    #[serde(default)]
    pub members: Vec<Member>,
    /// Key type of a dictionary collection.
    #[serde(default)]
    pub key: String,
    /// Generic type parameters of a class, trait or enum, e.g. `R: Row`.
    #[serde(default)]
    pub type_params: String,
    /// How many values of this type a process has.
    #[serde(default)]
    pub scope: Scope,
    /// `Some(period)` marks an active object (a task that runs on its own); the period may be
    /// empty, e.g. `every 1 s`.
    #[serde(default)]
    pub task: Option<String>,
    /// Source produced by a tool, not written by hand.
    #[serde(default)]
    pub generated: bool,
    /// For an enum: the variants are bit flags that combine.
    #[serde(default)]
    pub flags: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberKind {
    Field,
    Method,
    /// A variant of an enum; `ty` holds its data, if any.
    Variant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Protected,
    Private,
    /// Visible within the package (Rust `pub(crate)`, C# `internal`).
    Internal,
}

impl Visibility {
    pub const ALL: [Visibility; 4] =
        [Visibility::Public, Visibility::Internal, Visibility::Protected, Visibility::Private];

    /// UML visibility marker.
    pub fn symbol(self) -> &'static str {
        match self {
            Visibility::Public => "+",
            Visibility::Protected => "#",
            Visibility::Private => "-",
            Visibility::Internal => "~",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Protected => "protected",
            Visibility::Private => "private",
            Visibility::Internal => "internal",
        }
    }
}

/// A field or method of an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub kind: MemberKind,
    pub visibility: Visibility,
    pub name: String,
    /// Field type, or method return type. Empty means none (e.g. a method returning nothing).
    #[serde(default)]
    pub ty: String,
    /// Method parameter list as written between the parentheses. Unused for fields.
    #[serde(default)]
    pub params: String,
    /// Belongs to the type rather than to each value (UML: underlined).
    #[serde(default)]
    pub is_static: bool,
    /// A method that runs asynchronously (Rust/C# `async`).
    #[serde(default)]
    pub is_async: bool,
    /// This line stands for this many members of the same shape (0 or 1 = just this one), e.g.
    /// 74 generated table fields.
    #[serde(default)]
    pub repeat: u32,
}

impl Member {
    /// A new member with a placeholder name, numbered after the existing ones of its kind.
    /// Fields start private and methods public; trait members are always public.
    pub fn new(kind: MemberKind, owner: ShapeKind, existing: &[Member]) -> Self {
        let n = existing.iter().filter(|m| m.kind == kind).count() + 1;
        let (name, visibility) = match kind {
            MemberKind::Field => (format!("field{n}"), Visibility::Private),
            MemberKind::Method => (format!("method{n}"), Visibility::Public),
            MemberKind::Variant => (format!("Variant{n}"), Visibility::Public),
        };
        let visibility = if owner == ShapeKind::Trait { Visibility::Public } else { visibility };
        Self {
            kind,
            visibility,
            name,
            ty: String::new(),
            params: String::new(),
            is_static: false,
            is_async: false,
            repeat: 0,
        }
    }

    /// UML text, e.g. `- count: int`, `+ async draw(ctx: Context): bool`, `Login(LoginRequest)`.
    pub fn text(&self) -> String {
        if self.kind == MemberKind::Variant {
            let data = self.ty.trim();
            let mut v = if data.is_empty() { self.name.clone() } else { format!("{}({data})", self.name) };
            if self.repeat > 1 {
                v.push_str(&format!("  \u{d7}{}", self.repeat));
            }
            return v;
        }
        let mut s = format!("{} ", self.visibility.symbol());
        if self.is_async && self.kind == MemberKind::Method {
            s.push_str("async ");
        }
        s.push_str(&self.name);
        if self.kind == MemberKind::Method {
            s.push('(');
            s.push_str(&self.params);
            s.push(')');
        }
        if !self.ty.trim().is_empty() {
            s.push_str(": ");
            s.push_str(self.ty.trim());
        }
        if self.repeat > 1 {
            s.push_str(&format!("  \u{d7}{}", self.repeat));
        }
        s
    }
}

/// Where an object's members go, in world units relative to the object's top-left corner.
pub struct BodyLayout<'a> {
    /// Each member with the vertical center of its line.
    pub lines: Vec<(&'a Member, f32)>,
    /// Vertical positions of compartment separators below the header.
    pub separators: Vec<f32>,
    /// Height needed to show the header and all members.
    pub height: f32,
    /// Left text inset.
    pub pad: f32,
}

impl Node {
    /// Members laid out in UML compartments: for a class, fields then methods; for a trait,
    /// methods only. Classes without members keep a plain empty body.
    pub fn body_layout<'a>(&'a self) -> BodyLayout<'a> {
        let f = self.font_size;
        let line = f * 1.45;
        let pad = f * 0.45;
        let header = header_height(self.stereotype_text().is_some(), f, f32::INFINITY);
        let mut y = header;
        let mut lines = Vec::new();
        let mut separators = Vec::new();
        // First compartment: fields of a class, variants of an enum.
        let first_kind = if self.kind == ShapeKind::Enum { MemberKind::Variant } else { MemberKind::Field };
        let fields: Vec<&Member> = self.members.iter().filter(|m| m.kind == first_kind).collect();
        let methods: Vec<&Member> = self.members.iter().filter(|m| m.kind == MemberKind::Method).collect();
        let place = |list: &[&'a Member], y: &mut f32, lines: &mut Vec<(&'a Member, f32)>| {
            *y += pad;
            for m in list {
                lines.push((*m, *y + line / 2.0));
                *y += line;
            }
            *y += pad;
        };
        if matches!(self.kind, ShapeKind::Class | ShapeKind::Enum) && !self.members.is_empty() {
            place(&fields, &mut y, &mut lines);
            separators.push(y);
            place(&methods, &mut y, &mut lines);
        } else if !methods.is_empty() {
            place(&methods, &mut y, &mut lines);
        }
        // Keep at least a little body below the header, like an empty class.
        let height = y.max(header + line * 1.5);
        BodyLayout { lines, separators, height, pad }
    }

    /// Smallest size that shows the name and every member, given a text measurer
    /// (`measure(text)` = width in world units at this object's font size).
    pub fn required_size(&self, measure: impl Fn(&str) -> f32) -> Vec2 {
        let layout = self.body_layout();
        let name_w = measure(&self.title()) + layout.pad * 4.0;
        let stereo_w = self.stereotype_text().map_or(0.0, |s| measure(&s) * 0.85 + layout.pad * 4.0);
        let name_w = name_w.max(stereo_w);
        let members_w = self
            .members
            .iter()
            .map(|m| measure(&m.text()) + layout.pad * 2.0 + 4.0)
            .fold(0.0, f32::max);
        vec2(name_w.max(members_w), layout.height)
    }

    pub fn new(id: Id, kind: ShapeKind, center: Pos2) -> Self {
        let size = kind.default_size();
        let min = center - size / 2.0;
        let is_text = kind == ShapeKind::Text;
        Self {
            id,
            kind,
            pos: [min.x, min.y],
            size: [size.x, size.y],
            text: kind.default_name().into(),
            fill: match kind {
                ShapeKind::Text => NONE,
                ShapeKind::Package => [246, 247, 250, 255],
                _ => WHITE,
            },
            stroke: if is_text { NONE } else { BLACK },
            stroke_width: 1.0,
            text_color: BLACK,
            font_size: 12.0,
            members: Vec::new(),
            key: if kind == ShapeKind::Dictionary { "K".into() } else { String::new() },
            type_params: String::new(),
            scope: Scope::Many,
            task: None,
            generated: false,
            flags: false,
        }
    }

    /// Everything shown between guillemets above the name, e.g. `«trait, generated»` or
    /// `«singleton, task every 1 s»`. `None` when there is nothing to say.
    pub fn stereotype_text(&self) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        match self.kind {
            ShapeKind::Enum if self.flags => parts.push("flags".into()),
            ShapeKind::Package | ShapeKind::Directory | ShapeKind::File => {}
            k => {
                if let Some(s) = k.stereotype() {
                    parts.push(s.trim_matches(['\u{ab}', '\u{bb}']).to_owned());
                }
            }
        }
        if self.kind.is_object() {
            if self.scope != Scope::Many {
                parts.push(self.scope.name().to_owned());
            }
            if let Some(period) = &self.task {
                let period = period.trim();
                parts.push(if period.is_empty() { "task".to_owned() } else { format!("task {period}") });
            }
        }
        if self.generated {
            parts.push("generated".into());
        }
        (!parts.is_empty()).then(|| format!("\u{ab}{}\u{bb}", parts.join(", ")))
    }

    /// The name as shown in the header: with type parameters for generic types (`Table<R: Row>`).
    pub fn title(&self) -> String {
        let params = self.type_params.trim();
        if self.kind.is_object() && !params.is_empty() {
            format!("{}<{params}>", self.text)
        } else {
            self.text.clone()
        }
    }

    /// For a package: its name tab.
    pub fn package_tab(&self) -> Rect {
        let r = self.rect();
        Rect::from_min_size(
            r.min,
            vec2(
                package_tab_width(&self.text, self.font_size, r.width()),
                package_tab_height(self.font_size, r.height()),
            ),
        )
    }

    /// For a collection: the front card of its "stacked cards" shape.
    pub fn collection_front(&self) -> Rect {
        let r = self.rect();
        let off = collection_offset(r.width(), r.height());
        Rect::from_min_max(pos2(r.left(), r.top() + off), pos2(r.right() - off, r.bottom()))
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
        if self.kind == ShapeKind::Package {
            return self.package_tab();
        }
        if self.kind == ShapeKind::External || self.kind == ShapeKind::Directory {
            // External: below its stereotype. Directory: below the folder tab.
            let top = if self.kind == ShapeKind::External { self.font_size * 1.3 } else { self.font_size * 0.6 };
            return Rect::from_min_max(pos2(r.left(), (r.top() + top).min(r.bottom())), r.max);
        }
        if self.kind.is_collection() {
            // Below the stereotype on the front card.
            let front = self.collection_front();
            let top = (front.top() + self.font_size * 1.3).min(front.bottom());
            return Rect::from_min_max(pos2(front.left(), top), front.max);
        }
        if !self.kind.is_object() {
            return r;
        }
        let has_stereotype = self.stereotype_text().is_some();
        let header = header_height(has_stereotype, self.font_size, r.height());
        let top = if has_stereotype {
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
        ShapeKind::File => {
            let f = file_fold(r.width(), r.height());
            vec![r.left_top(), pos2(r.right() - f, r.top()), pos2(r.right(), r.top() + f), r.right_bottom(), r.left_bottom()]
        }
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

    /// Whether "never null" applies (References are never null anyway; Weak can always expire).
    pub fn can_be_required(self) -> bool {
        matches!(self, PointerKind::Raw | PointerKind::Unique | PointerKind::Shared)
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
    /// Derived -> base. Valid from a class to another class, or from a trait to another
    /// trait (supertrait / extended interface).
    Inherits,
    /// The source calls or depends on the target (not a member). Between packages it declares an
    /// allowed dependency.
    Uses,
}

impl RelationKind {
    pub const ALL: [RelationKind; 4] =
        [RelationKind::Related, RelationKind::Implements, RelationKind::Inherits, RelationKind::Uses];

    pub fn name(self) -> &'static str {
        match self {
            RelationKind::Related => "Related",
            RelationKind::Implements => "Implements",
            RelationKind::Inherits => "Inheritance",
            RelationKind::Uses => "Uses",
        }
    }

    /// Where this relation may go, for tooltips and the format panel.
    pub fn rule(self) -> &'static str {
        match self {
            RelationKind::Related => "Between any two objects",
            RelationKind::Implements => "From a class to the trait it implements",
            RelationKind::Inherits => "From the derived class to its base class (or trait to supertrait)",
            RelationKind::Uses => "From a type to a type it calls, or from a package to a package it depends on",
        }
    }

    /// Needs an object at both ends (cannot be placed by a single click).
    pub fn needs_two_objects(self) -> bool {
        !matches!(self, RelationKind::Related)
    }

    /// Line pattern, source head, target head.
    pub fn look(self) -> (LinePattern, Head, Head) {
        match self {
            RelationKind::Related => (LinePattern::Dotted, Head::None, Head::None),
            RelationKind::Implements => (LinePattern::Dashed, Head::None, Head::HollowArrow),
            RelationKind::Inherits => (LinePattern::Solid, Head::None, Head::HollowArrow),
            RelationKind::Uses => (LinePattern::DashDot, Head::None, Head::OpenArrow),
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
    /// Lock guarding what a pointer reaches (`Arc<Mutex<T>>`).
    #[serde(default)]
    pub lock: Lock,
    /// The pointer is never null (`[1]`); only for Raw, Unique and Shared.
    #[serde(default)]
    pub required: bool,
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
            lock: Lock::None,
            required: false,
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

    /// Pointer badge: kind and const (`unique_ptr<const>`), `[1]` when never null, and the lock
    /// (`+ mutex`).
    pub fn badge(&self) -> Option<String> {
        let Some(LinkKind::Pointer(k)) = self.link() else { return None };
        let mut b = k.badge(self.const_target);
        if self.required && k.can_be_required() {
            b.push_str(" [1]");
        }
        if self.lock != Lock::None {
            b.push_str(" + ");
            b.push_str(self.lock.name());
        }
        Some(b)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Diagram {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub next_id: Id,
}

/// One diagram of a project, with the name shown on its tab (project format 1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedDiagram {
    pub name: String,
    pub diagram: Diagram,
}

/// What a tab holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ItemContent {
    Diagram(Diagram),
    /// A free-text note.
    Text(String),
    /// A picture, by file path.
    Image(String),
}

impl ItemContent {
    #[cfg(test)]
    pub fn kind_name(&self) -> &'static str {
        match self {
            ItemContent::Diagram(_) => "diagram",
            ItemContent::Text(_) => "text",
            ItemContent::Image(_) => "image",
        }
    }
}

/// One tab of a project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectItem {
    /// Stable id; the dock layout refers to tabs by it.
    pub id: u64,
    pub name: String,
    pub content: ItemContent,
}

/// The tab arrangement: splits, tab groups and floating windows, holding item ids.
pub type Layout = egui_stack::DockState<u64>;

/// A project file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectFile {
    /// Format marker and version: 1 = diagrams only, 2 = items and layout. Files from before the
    /// rename say `architect_ai_project`.
    #[serde(alias = "architect_ai_project")]
    pub alien_artefact_project: u32,
    /// Format 1: the diagrams, one tab each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagrams: Vec<NamedDiagram>,
    /// Format 2: every tab.
    #[serde(default)]
    pub items: Vec<ProjectItem>,
    /// Format 1: index of the shown diagram. Format 2: id of the active diagram.
    #[serde(default)]
    pub active: u64,
    /// Format 2: where the tabs are, as RON text. (The layout holds floats that are infinite
    /// until first drawn, which JSON cannot store; RON can.)
    #[serde(default)]
    pub layout: Option<String>,
}

/// The layout as stored in a project file.
pub fn layout_to_text(layout: &Layout) -> Option<String> {
    ron::to_string(layout).ok()
}

/// A project as loaded.
pub struct Project {
    pub items: Vec<ProjectItem>,
    /// The diagram that was active, if known.
    pub active: Option<u64>,
    /// The tab arrangement, if the file had a usable one.
    pub layout: Option<Layout>,
}

/// Reads a project file of any version, or a single-diagram file from before projects existed
/// (it becomes a one-diagram project named `fallback_name`). Older fields are migrated, and a
/// layout that does not match the items is dropped.
pub fn load_project(text: &str, fallback_name: &str) -> Result<Project, String> {
    let mut project = match serde_json::from_str::<ProjectFile>(text) {
        Ok(p) if !p.items.is_empty() => Project {
            items: p.items,
            active: Some(p.active),
            // An unreadable layout is not worth refusing the project for: the tabs are regrouped.
            layout: p.layout.and_then(|text| ron::from_str::<Layout>(&text).ok()),
        },
        Ok(p) => {
            // Format 1: diagrams with positional ids.
            let items: Vec<ProjectItem> = p
                .diagrams
                .into_iter()
                .enumerate()
                .map(|(i, d)| ProjectItem { id: i as u64 + 1, name: d.name, content: ItemContent::Diagram(d.diagram) })
                .collect();
            Project { items, active: Some(p.active + 1), layout: None }
        }
        Err(project_err) => match serde_json::from_str::<Diagram>(text) {
            Ok(d) => Project {
                items: vec![ProjectItem { id: 1, name: fallback_name.to_owned(), content: ItemContent::Diagram(d) }],
                active: Some(1),
                layout: None,
            },
            // Report the project error unless the file clearly is a single diagram.
            Err(diagram_err) => {
                let is_project = text.contains("alien_artefact_project") || text.contains("architect_ai_project");
                return Err(if is_project { project_err } else { diagram_err }.to_string());
            }
        },
    };
    for item in &mut project.items {
        if let ItemContent::Diagram(d) = &mut item.content {
            d.migrate();
        }
    }
    // The layout must show exactly the items, each once.
    if let Some(layout) = &project.layout {
        let mut shown: Vec<u64> = layout.iter_all_tabs().map(|(_, id)| *id).collect();
        shown.sort_unstable();
        let mut ids: Vec<u64> = project.items.iter().map(|i| i.id).collect();
        ids.sort_unstable();
        if shown != ids {
            project.layout = None;
        }
    }
    Ok(project)
}

/// Subset of a diagram, used for copy/paste and duplicate.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Clip {
    /// Marker so arbitrary JSON in the clipboard isn't mistaken for a clip.
    #[serde(alias = "architect_ai_clip")]
    pub alien_artefact_clip: u32,
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
    #[cfg(test)]
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

    /// How an end is called in messages: the object's name and kind, or empty canvas.
    fn describe(&self, end: End) -> String {
        match end.node().and_then(|id| self.node(id)) {
            Some(n) => {
                let kind = match n.kind {
                    ShapeKind::Class => "a class",
                    ShapeKind::Trait => "a trait",
                    ShapeKind::File => "a file",
                    ShapeKind::Directory => "a directory",
                    ShapeKind::Enum => "an enum",
                    ShapeKind::External => "an external type",
                    ShapeKind::Package => "a package",
                    k if k.is_collection() => {
                        return format!("The {} of {}", k.name().to_lowercase(), self.display_text(n));
                    }
                    _ => "a shape",
                };
                format!("\"{}\" is {kind}", n.text)
            }
            None => "The end is on empty canvas".to_owned(),
        }
    }

    /// The pointer that gives a collection its element type, ignoring line `exclude`.
    pub fn element_pointer(&self, collection: Id, exclude: Option<Id>) -> Option<&Edge> {
        self.edges
            .iter()
            .find(|e| e.pointer.is_some() && e.from == End::Node(collection) && Some(e.id) != exclude)
    }

    /// Text shown inside a shape: its name, or for a collection its element type
    /// (`key -> value` for a dictionary).
    pub fn display_text(&self, n: &Node) -> String {
        if !n.kind.is_collection() {
            return n.title();
        }
        let or_q = |s: &str| if s.trim().is_empty() { "?".to_owned() } else { s.trim().to_owned() };
        let element = self
            .element_pointer(n.id, None)
            .and_then(|e| e.to.node())
            .and_then(|id| self.node(id))
            .map(|t| t.text.clone())
            .unwrap_or_else(|| or_q(&n.text));
        if n.kind == ShapeKind::Dictionary {
            format!("{} \u{2192} {}", or_q(&n.key), element)
        } else {
            element
        }
    }

    /// Explanation for the user when a line of this kind may not connect `from` to `to`
    /// (`None` if it may).
    #[cfg(test)]
    pub fn link_error(&self, kind: LinkKind, from: End, to: End) -> Option<String> {
        self.link_error_for(None, kind, from, to)
    }

    /// Like `link_error`, for existing line `edge` (which is not counted against itself).
    pub fn link_error_for(&self, edge: Option<Id>, kind: LinkKind, from: End, to: End) -> Option<String> {
        self.link_problem(edge, kind, from, to)
            .map(|(reason, rule)| format!("{reason}\n\n{rule}").trim_end().to_owned())
    }

    /// Whether a line of this kind may connect `from` to `to`.
    pub fn link_allowed(&self, kind: LinkKind, from: End, to: End) -> bool {
        self.link_allowed_for(None, kind, from, to)
    }

    /// Like `link_allowed`, for existing line `edge` (which is not counted against itself).
    pub fn link_allowed_for(&self, edge: Option<Id>, kind: LinkKind, from: End, to: End) -> bool {
        self.link_problem(edge, kind, from, to).is_none()
    }

    /// Why a line of this kind may not connect `from` to `to`: (what is wrong, the rule).
    /// `None` when it may. `edge` is the line itself when it already exists.
    fn link_problem(&self, edge: Option<Id>, kind: LinkKind, from: End, to: End) -> Option<(String, String)> {
        use ShapeKind::{Class, Enum, External, Package, Trait};
        let (fk, tk) = (self.node_kind(from), self.node_kind(to));
        let is = |k: Option<ShapeKind>, f: fn(ShapeKind) -> bool| k.is_some_and(f);
        let s = |x: &str| x.to_owned();
        match kind {
            LinkKind::Pointer(_) => {
                if is(fk, ShapeKind::is_storage) || is(tk, ShapeKind::is_storage) {
                    let end = if is(fk, ShapeKind::is_storage) { from } else { to };
                    return Some((
                        format!(
                            "{}. Files and directories are not values in the program's memory, so \
                             nothing can point to them and they cannot point to anything.",
                            self.describe(end)
                        ),
                        s("Use \"Related\" to show that an object reads or writes a file. A handle to an \
                           open file is a field of the object (e.g. std::fstream, std::fs::File)."),
                    ));
                }
                if fk == Some(Package) || tk == Some(Package) {
                    let end = if fk == Some(Package) { from } else { to };
                    return Some((
                        format!("{}. A pointer connects objects, not the packages that hold them.", self.describe(end)),
                        s("Draw the pointer between objects inside the packages. To show that one package \
                           depends on another, draw \"Uses\" between the packages."),
                    ));
                }
                if fk == Some(External) {
                    return Some((
                        format!("{}. Its members are not part of this program, so it cannot own pointers.", self.describe(from)),
                        s("Point from your own types to external ones, not the other way."),
                    ));
                }
                if let Some(c) = from.node().filter(|_| is(fk, ShapeKind::is_collection)) {
                    if from == to {
                        return Some((
                            format!("{} cannot contain itself.", self.describe(from)),
                            s("A collection's elements are of another type."),
                        ));
                    }
                    if let Some(other) = self.element_pointer(c, edge) {
                        let target = other
                            .to
                            .node()
                            .and_then(|id| self.node(id))
                            .map_or_else(|| s("another object"), |n| format!("\"{}\"", n.text));
                        return Some((
                            format!("{} already has an element pointer, to {target}.", self.describe(from)),
                            s("A pointer leaving a collection says what its elements are, so a collection has \
                               exactly one. For pairs or several types, use a dictionary or a class as the element."),
                        ));
                    }
                }
                None
            }
            LinkKind::Relation(RelationKind::Implements) => {
                let ok_from = matches!(fk, Some(Class) | Some(Enum));
                let ok_to = matches!(tk, Some(Trait) | Some(External));
                if ok_from && ok_to {
                    return None;
                }
                let reason = match (fk, tk) {
                    (None, _) => s("It has to start on the class that implements the trait."),
                    (Some(Trait), Some(Class)) => s("The direction is reversed: draw it from the class to the trait."),
                    _ if !ok_from => format!("{}, and only a class or an enum can implement a trait.", self.describe(from)),
                    (_, None) => s("It has to end on the trait being implemented."),
                    _ => format!("{}, and only a trait (or an external trait) can be implemented.", self.describe(to)),
                };
                Some((reason, s("\"Implements\" goes from a class or enum to the trait it implements.")))
            }
            LinkKind::Relation(RelationKind::Inherits) => {
                let ok = from != to && matches!(fk, Some(Class) | Some(Trait)) && fk == tk;
                if ok {
                    return None;
                }
                let reason = match (fk, tk) {
                    (None, _) => s("It has to start on the derived class."),
                    (_, None) => s("It has to end on the base class."),
                    _ if from == to => match fk {
                        Some(Trait) => format!("{}. A trait cannot extend itself.", self.describe(from)),
                        _ => format!("{}. A class cannot derive from itself.", self.describe(from)),
                    },
                    (Some(Class), Some(Trait)) => format!(
                        "{}. A class doesn't inherit from a trait; use \"Implements\" for that.",
                        self.describe(to)
                    ),
                    (Some(Trait), Some(Class)) => format!("{}, and a trait can't inherit from a class.", self.describe(to)),
                    _ => s("Only classes and traits can take part in inheritance."),
                };
                Some((
                    reason,
                    s("\"Inheritance\" goes from the derived class to its base class, or from a trait to \
                       the trait it extends."),
                ))
            }
            LinkKind::Relation(RelationKind::Related) => (from.node().is_some() && from == to).then(|| {
                (
                    format!("{}. An object can't be related to itself.", self.describe(from)),
                    s("\"Related\" connects two different objects."),
                )
            }),
            LinkKind::Relation(RelationKind::Uses) => {
                let rule = s("\"Uses\" goes from a type to a type it calls or depends on, or from a package to \
                              a package it depends on.");
                let user = |k: Option<ShapeKind>| is(k, ShapeKind::is_object) || k == Some(Package);
                let used = |k: Option<ShapeKind>| {
                    matches!(k, Some(Class) | Some(Trait) | Some(Enum) | Some(External) | Some(Package))
                };
                let reason = if fk.is_none() {
                    s("It has to start on the type (or package) that does the using.")
                } else if tk.is_none() {
                    s("It has to end on the type (or package) being used.")
                } else if from == to {
                    format!("{}. Using itself says nothing.", self.describe(from))
                } else if is(fk, ShapeKind::is_storage) || is(tk, ShapeKind::is_storage) {
                    s("Files and directories are not called. Use \"Related\" to show that a type reads or writes them.")
                } else if (fk == Some(Package)) != (tk == Some(Package)) {
                    s("A package uses packages and a type uses types; this line mixes the two.")
                } else if !user(fk) {
                    format!("{}, which has no code of its own to do the calling.", self.describe(from))
                } else if !used(tk) {
                    format!("{}, which is data rather than something to call.", self.describe(to))
                } else {
                    return None;
                };
                Some((reason, rule))
            }
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
        // Objects first, then the innermost (smallest) package around the point.
        self.nodes
            .iter()
            .rev()
            .find(|n| n.kind != ShapeKind::Package && n.rect().contains(p))
            .or_else(|| {
                self.nodes
                    .iter()
                    .filter(|n| n.kind == ShapeKind::Package && n.rect().contains(p))
                    .min_by(|a, b| a.rect().area().total_cmp(&b.rect().area()))
            })
            .map(|n| n.id)
    }

    /// What a click at `p` selects: an object, else a line within `tol`, else the package frame
    /// behind them. Lines come before packages because they are drawn on top of the frames.
    pub fn hit(&self, p: Pos2, tol: f32) -> Option<Id> {
        self.nodes
            .iter()
            .rev()
            .find(|n| n.kind != ShapeKind::Package && n.rect().contains(p))
            .map(|n| n.id)
            .or_else(|| self.edge_at(p, tol))
            .or_else(|| self.node_at(p))
    }

    /// The innermost package whose frame contains object `id` entirely.
    pub fn package_of(&self, id: Id) -> Option<Id> {
        let r = self.node(id)?.rect();
        self.nodes
            .iter()
            .filter(|p| p.kind == ShapeKind::Package && p.id != id && p.rect().contains_rect(r))
            .min_by(|a, b| a.rect().area().total_cmp(&b.rect().area()))
            .map(|p| p.id)
    }

    /// Every object inside package `id`'s frame (at any depth).
    pub fn package_contents(&self, id: Id) -> Vec<Id> {
        let Some(frame) = self.node(id).map(|p| p.rect()) else { return Vec::new() };
        self.nodes.iter().filter(|n| n.id != id && frame.contains_rect(n.rect())).map(|n| n.id).collect()
    }

    /// Package `id` and every package around it, innermost first.
    pub fn package_chain(&self, id: Id) -> Vec<Id> {
        let mut chain = Vec::new();
        let mut cur = self.package_of(id);
        while let Some(p) = cur {
            if chain.contains(&p) {
                break;
            }
            chain.push(p);
            cur = self.package_of(p);
        }
        chain
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
            alien_artefact_clip: 1,
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
        assert_eq!(d.link_error(IMPL, c, t), None);
        let msg = d.link_error(IMPL, c, c2).unwrap();
        assert!(msg.contains("is a class") && msg.contains("only a trait (or an external trait) can be implemented"), "{msg}");
        assert!(d.link_error(IMPL, t, c).unwrap().contains("reversed"));
        assert!(d.link_error(IMPL, c, free).unwrap().contains("end on the trait"));
        assert!(d.link_error(IMPL, free, t).unwrap().contains("start on the class"));

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

#[cfg(test)]
mod inheritance_tests {
    use super::*;

    const INHERITS: LinkKind = LinkKind::Relation(RelationKind::Inherits);

    #[test]
    fn inheritance_between_same_kind_objects_only() {
        let mut d = Diagram::default();
        let base = End::Node(d.add_node(ShapeKind::Class, pos2(0.0, 0.0)));
        let derived = End::Node(d.add_node(ShapeKind::Class, pos2(0.0, 300.0)));
        let t1 = End::Node(d.add_node(ShapeKind::Trait, pos2(300.0, 0.0)));
        let t2 = End::Node(d.add_node(ShapeKind::Trait, pos2(300.0, 300.0)));
        let free = End::point(pos2(900.0, 900.0));

        assert!(d.link_allowed(INHERITS, derived, base));
        assert!(d.link_allowed(INHERITS, t2, t1), "supertrait");
        assert!(!d.link_allowed(INHERITS, base, base), "itself");
        assert!(!d.link_allowed(INHERITS, derived, t1), "class -> trait is Implements");
        assert!(!d.link_allowed(INHERITS, t1, base), "trait -> class");
        assert!(!d.link_allowed(INHERITS, derived, free));

        assert!(d.link_error(INHERITS, derived, t1).unwrap().contains("use \"Implements\""));
        assert!(d.link_error(INHERITS, base, base).unwrap().contains("A class cannot derive from itself"));
        assert!(d.link_error(INHERITS, t1, t1).unwrap().contains("A trait cannot extend itself"));
        let related = LinkKind::Relation(RelationKind::Related);
        assert!(!d.link_allowed(related, base, base));
        assert!(d.link_error(related, base, base).unwrap().contains("related to itself"));
        assert!(d.link_allowed(related, base, derived));
        assert!(d.link_error(INHERITS, derived, free).unwrap().contains("end on the base class"));
        assert!(d.link_error(INHERITS, free, base).unwrap().contains("start on the derived class"));
        assert_eq!(d.link_error(INHERITS, derived, base), None);
    }

    #[test]
    fn inheritance_looks_like_uml_generalization() {
        assert_eq!(RelationKind::Inherits.look(), (LinePattern::Solid, Head::None, Head::HollowArrow));
        assert!(RelationKind::Inherits.needs_two_objects() && !RelationKind::Related.needs_two_objects());
    }
}

#[cfg(test)]
mod member_tests {
    use super::*;

    fn field(name: &str, ty: &str, v: Visibility) -> Member {
        Member { kind: MemberKind::Field, visibility: v, name: name.into(), ty: ty.into(), params: String::new(), is_static: false, is_async: false, repeat: 0 }
    }

    #[test]
    fn member_text_uses_uml_notation() {
        assert_eq!(field("count", "int", Visibility::Private).text(), "- count: int");
        assert_eq!(field("id", "", Visibility::Protected).text(), "# id");
        let m = Member {
            kind: MemberKind::Method,
            visibility: Visibility::Public,
            name: "draw".into(),
            ty: "bool".into(),
            params: "ctx: Context".into(),
            is_static: true,
            is_async: false,
            repeat: 0,
        };
        assert_eq!(m.text(), "+ draw(ctx: Context): bool");
        let mut no_ret = m.clone();
        no_ret.ty.clear();
        assert_eq!(no_ret.text(), "+ draw(ctx: Context)");
    }

    #[test]
    fn new_members_get_numbered_names_and_sensible_visibility() {
        let mut ms = vec![];
        ms.push(Member::new(MemberKind::Field, ShapeKind::Class, &ms));
        ms.push(Member::new(MemberKind::Field, ShapeKind::Class, &ms));
        ms.push(Member::new(MemberKind::Method, ShapeKind::Class, &ms));
        assert_eq!((ms[1].name.as_str(), ms[1].visibility), ("field2", Visibility::Private));
        assert_eq!((ms[2].name.as_str(), ms[2].visibility), ("method1", Visibility::Public));
        assert_eq!(Member::new(MemberKind::Field, ShapeKind::Trait, &[]).visibility, Visibility::Public);
    }

    #[test]
    fn class_layout_has_field_and_method_compartments() {
        let mut n = Node::new(1, ShapeKind::Class, pos2(0.0, 0.0));
        let empty = n.body_layout();
        assert!(empty.lines.is_empty() && empty.separators.is_empty());

        n.members.push(field("a", "int", Visibility::Private));
        n.members.push(Member::new(MemberKind::Method, ShapeKind::Class, &n.members));
        n.members.push(field("b", "int", Visibility::Private));
        let l = n.body_layout();
        // Fields first (in order), then the separator, then methods.
        let names: Vec<&str> = l.lines.iter().map(|(m, _)| m.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "method1"]);
        assert_eq!(l.separators.len(), 1);
        assert!(l.lines[1].1 < l.separators[0] && l.separators[0] < l.lines[2].1);
        assert!(l.height > l.lines[2].1);

        // Required size grows with members; measurement is supplied by the caller.
        let need = n.required_size(|s| s.len() as f32 * 7.0);
        assert!(need.y >= l.height && need.x > 0.0);
    }

    #[test]
    fn trait_layout_has_methods_only_and_old_files_load() {
        let mut t = Node::new(1, ShapeKind::Trait, pos2(0.0, 0.0));
        t.members.push(Member::new(MemberKind::Method, ShapeKind::Trait, &[]));
        let l = t.body_layout();
        assert_eq!(l.lines.len(), 1);
        assert!(l.separators.is_empty());

        let json = serde_json::to_string(&Node::new(2, ShapeKind::Class, pos2(0.0, 0.0))).unwrap();
        let old = json.replace(r#","members":[]"#, "");
        assert!(!old.contains("members"));
        let back: Node = serde_json::from_str(&old).unwrap();
        assert!(back.members.is_empty());
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;

    #[test]
    fn files_take_related_only() {
        let mut d = Diagram::default();
        let c = End::Node(d.add_node(ShapeKind::Class, pos2(0.0, 0.0)));
        let t = End::Node(d.add_node(ShapeKind::Trait, pos2(300.0, 0.0)));
        let f = End::Node(d.add_node(ShapeKind::File, pos2(0.0, 300.0)));
        let f2 = End::Node(d.add_node(ShapeKind::File, pos2(300.0, 300.0)));

        let related = LinkKind::Relation(RelationKind::Related);
        assert!(d.link_allowed(related, c, f) && d.link_allowed(related, f, f2));

        for k in PointerKind::ALL {
            let p = LinkKind::Pointer(k);
            assert!(!d.link_allowed(p, c, f) && !d.link_allowed(p, f, c) && !d.link_allowed(p, f, f));
            assert!(d.link_allowed(p, c, t));
        }
        let msg = d.link_error(LinkKind::Pointer(PointerKind::Unique), c, f).unwrap();
        assert!(msg.contains("\"file.txt\" is a file") && msg.contains("Related"), "{msg}");

        assert!(!d.link_allowed(LinkKind::Relation(RelationKind::Implements), f, t));
        assert!(!d.link_allowed(LinkKind::Relation(RelationKind::Inherits), f, f2));
        assert!(d.link_error(LinkKind::Relation(RelationKind::Inherits), f, f2).unwrap().contains("Only classes and traits"));
    }

    #[test]
    fn file_shape_is_a_named_non_type_with_folded_corner() {
        let n = Node::new(1, ShapeKind::File, pos2(0.0, 0.0));
        assert_eq!(n.text, "file.txt");
        assert!(n.kind.is_named() && !n.kind.is_object());
        assert_eq!(n.text_rect(), n.rect(), "name is centered in the whole shape");
        let pts = outline(ShapeKind::File, n.rect()).unwrap();
        assert_eq!(pts.len(), 5, "rectangle with one corner cut off");
        // Lines attach to the cut corner, not the bounding box corner.
        let corner = n.boundary_point(n.rect().right_top() + vec2(100.0, -100.0));
        assert!(n.rect().contains(corner) && corner.distance(n.rect().right_top()) > 1.0);
    }
}

#[cfg(test)]
mod collection_tests {
    use super::*;

    const UNIQUE: LinkKind = LinkKind::Pointer(PointerKind::Unique);

    #[test]
    fn element_type_comes_from_pointer_or_text() {
        let mut d = Diagram::default();
        let v = d.add_node(ShapeKind::Vector, pos2(0.0, 0.0));
        let tex = d.add_node(ShapeKind::Class, pos2(300.0, 0.0));
        d.node_mut(tex).unwrap().text = "Texture".into();
        assert_eq!(d.display_text(d.node(v).unwrap()), "T", "text until a pointer exists");

        d.add_link(End::Node(v), End::Node(tex), UNIQUE, false);
        assert_eq!(d.display_text(d.node(v).unwrap()), "Texture");

        let m = d.add_node(ShapeKind::Dictionary, pos2(0.0, 300.0));
        d.node_mut(m).unwrap().key = "String".into();
        d.add_link(End::Node(m), End::Node(tex), LinkKind::Pointer(PointerKind::Shared), false);
        assert_eq!(d.display_text(d.node(m).unwrap()), "String \u{2192} Texture");
    }

    #[test]
    fn collection_has_one_element_pointer_and_cannot_contain_itself() {
        let mut d = Diagram::default();
        let v = End::Node(d.add_node(ShapeKind::Vector, pos2(0.0, 0.0)));
        let a = End::Node(d.add_node(ShapeKind::Class, pos2(300.0, 0.0)));
        let b = End::Node(d.add_node(ShapeKind::Class, pos2(300.0, 300.0)));
        let owner = End::Node(d.add_node(ShapeKind::Class, pos2(-300.0, 0.0)));

        assert!(d.link_allowed(UNIQUE, v, a));
        let first = d.add_link(v, a, UNIQUE, false);
        assert!(!d.link_allowed(UNIQUE, v, b), "second element pointer");
        assert!(d.link_error(UNIQUE, v, b).unwrap().contains("already has an element pointer"));
        // The existing element pointer may be moved to another target.
        assert!(d.link_allowed_for(Some(first), UNIQUE, v, b));

        assert!(!d.link_allowed_for(Some(first), UNIQUE, v, v));
        assert!(d.link_error_for(Some(first), UNIQUE, v, v).unwrap().contains("cannot contain itself"));

        // Owners point to collections freely; collections may hold collections.
        assert!(d.link_allowed(UNIQUE, owner, v));
        let inner = End::Node(d.add_node(ShapeKind::Set, pos2(0.0, 300.0)));
        assert!(d.link_allowed(UNIQUE, inner, v));

        // No inheritance or implements with collections.
        assert!(!d.link_allowed(LinkKind::Relation(RelationKind::Inherits), v, inner));
    }

    #[test]
    fn collection_shape_and_serialization() {
        let n = Node::new(1, ShapeKind::Dictionary, pos2(0.0, 0.0));
        assert_eq!((n.text.as_str(), n.key.as_str()), ("T", "K"));
        assert!(n.kind.is_collection() && !n.kind.is_object() && n.kind.stereotype().is_some());
        let (front, t) = (n.collection_front(), n.text_rect());
        assert!(n.rect().contains_rect(front) && front.contains_rect(t) && t.top() > front.top());
        let back: Node = serde_json::from_str(&serde_json::to_string(&n).unwrap()).unwrap();
        assert_eq!(back, n);
    }
}

#[cfg(test)]
mod hit_tests {
    use super::*;

    #[test]
    fn lines_inside_a_package_are_clickable() {
        let mut d = Diagram::default();
        let pkg = d.add_node(ShapeKind::Package, pos2(0.0, 0.0)); // 420x300 around the origin
        let a = d.add_node(ShapeKind::Class, pos2(-100.0, -60.0));
        let b = d.add_node(ShapeKind::Class, pos2(100.0, 80.0));
        let e = d.add_link(End::Node(a), End::Node(b), LinkKind::Pointer(PointerKind::Unique), false);
        d.edge_mut(e).unwrap().style = EdgeStyle::Straight;
        let mid = polyline_midpoint(&d.edge_path(d.edge(e).unwrap()).unwrap());

        assert_eq!(d.hit(mid, 3.0), Some(e), "the line, not the package behind it");
        assert_eq!(d.hit(pos2(-100.0, -60.0), 3.0), Some(a), "objects first");
        assert_eq!(d.hit(pos2(180.0, -120.0), 3.0), Some(pkg), "empty frame area selects the package");
    }
}

#[cfg(test)]
mod project_tests {
    use super::*;

    fn diagram_with_class() -> Diagram {
        let mut a = Diagram::default();
        a.add_node(ShapeKind::Class, pos2(0.0, 0.0));
        a
    }

    #[test]
    fn format_1_projects_and_single_diagrams_load() {
        let a = diagram_with_class();
        let v1 = format!(
            r#"{{"alien_artefact_project":1,"diagrams":[{{"name":"Server","diagram":{}}},{{"name":"Client","diagram":{}}}],"active":1}}"#,
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&Diagram::default()).unwrap()
        );
        let p = load_project(&v1, "x").unwrap();
        assert_eq!(p.items.len(), 2);
        assert_eq!((p.items[0].id, p.items[0].name.as_str()), (1, "Server"));
        assert_eq!(p.items[0].content, ItemContent::Diagram(a.clone()));
        assert_eq!(p.active, Some(2), "format 1 stored the index; ids are index + 1");
        assert!(p.layout.is_none());

        let single = load_project(&serde_json::to_string(&a).unwrap(), "old").unwrap();
        assert_eq!((single.items.len(), single.items[0].name.as_str()), (1, "old"));

        assert!(load_project("{ not json", "x").is_err());
    }

    #[test]
    fn format_2_keeps_items_and_layout() {
        let items = vec![
            ProjectItem { id: 7, name: "D".into(), content: ItemContent::Diagram(diagram_with_class()) },
            ProjectItem { id: 9, name: "Notes".into(), content: ItemContent::Text("hello".into()) },
            ProjectItem { id: 12, name: "Pic".into(), content: ItemContent::Image("a.png".into()) },
        ];
        let file = ProjectFile {
            alien_artefact_project: 2,
            diagrams: Vec::new(),
            items: items.clone(),
            active: 7,
            layout: layout_to_text(&Layout::new(vec![7, 9, 12])),
        };
        let json = serde_json::to_string(&file).unwrap();
        assert!(!json.contains("\"diagrams\""), "format 1 field is not written");
        let p = load_project(&json, "x").unwrap();
        assert_eq!(p.items, items);
        assert_eq!(p.active, Some(7));
        assert!(p.layout.is_some());

        // A layout that lost an item is dropped (the app then puts every tab in one group).
        let broken = ProjectFile { layout: layout_to_text(&Layout::new(vec![7, 9])), ..file.clone() };
        let p = load_project(&serde_json::to_string(&broken).unwrap(), "x").unwrap();
        assert!(p.layout.is_none());

        // Unreadable layout text: the project still opens, without its layout.
        let garbage = ProjectFile { layout: Some("not ron".into()), ..file };
        let p = load_project(&serde_json::to_string(&broken).unwrap(), "x").unwrap();
        assert!(p.layout.is_none());
    }
}

#[cfg(test)]
mod rename_tests {
    use super::*;

    #[test]
    fn projects_saved_before_the_rename_still_open() {
        let old = r#"{"architect_ai_project":1,"diagrams":[{"name":"Old","diagram":{"nodes":[],"edges":[],"next_id":0}}],"active":0}"#;
        let project = load_project(old, "x").unwrap();
        assert_eq!(project.items[0].name, "Old");
        let clip: Clip = serde_json::from_str(r#"{"architect_ai_clip":1,"nodes":[],"edges":[]}"#).unwrap();
        assert_eq!(clip.alien_artefact_clip, 1);
    }
}


/// Rule O1/F1 while editing: an object's name may be emptied while it is being typed, but not left
/// empty. Puts back `original` (the name before editing), or the kind's default name.
pub fn keep_name(name: &mut String, original: &str, kind: ShapeKind) {
    if name.trim().is_empty() {
        *name = if original.trim().is_empty() { kind.default_name().to_owned() } else { original.to_owned() };
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[test]
    fn names_cannot_be_left_empty() {
        let mut name = "   ".to_owned();
        keep_name(&mut name, "Renderer", ShapeKind::Trait);
        assert_eq!(name, "Renderer", "the name before editing comes back");

        let mut name = String::new();
        keep_name(&mut name, "", ShapeKind::Enum);
        assert_eq!(name, "EnumName", "or the kind's default");

        let mut name = "Other".to_owned();
        keep_name(&mut name, "Renderer", ShapeKind::Class);
        assert_eq!(name, "Other", "a real name is kept");
    }
}
