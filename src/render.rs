//! Painting shapes and edges in screen space. Shared by the canvas and the shape palette.

use egui::{
    epaint::text::LayoutJob, pos2, vec2, Align, Color32, FontId, Painter, Pos2, Rect, Shape,
    Stroke, StrokeKind,
};

use crate::model::{
    badge_center, edge_geometry, header_height, package_tab_height, package_tab_width, stereotype_height, Node, outline,
    Head, HeadFill, LinePattern, ShapeKind,
};

// ------------------------------------------------------------------------------------------------
// Dark theme for the canvas. Diagrams keep their real colours (they are saved and exported as
// drawn in the light theme); on screen in the dark theme, light fills become dark surfaces with a
// hint of their hue, and dark lines and text become light. Strong colours are left alone.

static DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_dark(on: bool) {
    DARK.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_dark() -> bool {
    DARK.load(std::sync::atomic::Ordering::Relaxed)
}

/// Perceived lightness, 0 (black) to 1 (white).
fn lightness(c: Color32) -> f32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0
}

/// `a` moved towards `b` by `t` (0..1), keeping `a`'s alpha.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let [ar, ag, ab, aa] = a.to_srgba_unmultiplied();
    let [br, bg, bb, _] = b.to_srgba_unmultiplied();
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(f(ar, br), f(ag, bg), f(ab, bb), aa)
}

const DARK_SURFACE: Color32 = Color32::from_rgb(45, 46, 50);
const DARK_INK: Color32 = Color32::from_rgb(218, 220, 224);

/// A fill as shown in the current theme.
pub fn tone_fill(c: Color32) -> Color32 {
    if is_dark() && c.a() > 0 && lightness(c) > 0.6 { mix(DARK_SURFACE, c, 0.12) } else { c }
}

/// A package frame's fill: in the dark theme only a step above the canvas, so the objects inside
/// (`tone_fill`) stand out from it.
pub fn tone_frame(c: Color32) -> Color32 {
    if is_dark() && c.a() > 0 && lightness(c) > 0.6 { mix(Color32::from_rgb(36, 37, 41), c, 0.05) } else { c }
}

/// A line or text colour as shown in the current theme.
pub fn tone_ink(c: Color32) -> Color32 {
    if is_dark() && c.a() > 0 && lightness(c) < 0.35 { mix(DARK_INK, c, 0.15) } else { c }
}

/// The canvas behind the diagram.
pub fn canvas_color() -> Color32 {
    if is_dark() { Color32::from_rgb(30, 31, 34) } else { Color32::WHITE }
}

pub fn grid_color() -> Color32 {
    if is_dark() { Color32::from_rgb(42, 43, 47) } else { Color32::from_gray(235) }
}

/// Background of the small pointer badges.
fn badge_color() -> Color32 {
    if is_dark() { Color32::from_rgb(50, 52, 57) } else { Color32::from_gray(248) }
}

/// Per-object text a shape draws itself: the stereotype line and, for a package tab, the name.
#[derive(Clone, Copy, Default)]
pub struct Deco<'a> {
    pub stereotype: Option<&'a str>,
    pub name: &'a str,
}

/// `font_px` is the shape's font size in screen pixels (sizes the class name compartment).
pub fn paint_shape(
    painter: &Painter,
    kind: ShapeKind,
    r: Rect,
    fill: Color32,
    stroke: Stroke,
    zoom: f32,
    font_px: f32,
    deco: Deco,
) {
    // The stereotype is skipped when too small to read (e.g. palette icons).
    let stereotype = deco.stereotype.filter(|_| font_px * 0.85 >= 5.0);
    let paint_stereotype = |area: Rect| {
        if let Some(text) = stereotype {
            let band = Rect::from_min_size(area.min, vec2(area.width(), stereotype_height(font_px, area.height())));
            paint_text(painter, band, text, font_px * 0.85, stroke.color);
        }
    };
    match kind {
        ShapeKind::Class | ShapeKind::Trait | ShapeKind::Enum => {
            painter.rect(r, 0.0, fill, stroke, StrokeKind::Middle);
            let y = r.top() + header_height(deco.stereotype.is_some(), font_px, r.height());
            if y < r.bottom() {
                painter.line_segment([pos2(r.left(), y), pos2(r.right(), y)], stroke);
            }
            paint_stereotype(r);
        }
        ShapeKind::External => {
            // Dashed border: a type that is not ours.
            painter.rect_filled(r, 3.0, fill);
            let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(Shape::dashed_line(&corners, stroke, 6.0 * zoom.max(0.3), 4.0 * zoom.max(0.3)));
            paint_stereotype(r);
        }
        ShapeKind::Package => {
            // A frame with a name tab on its top-left corner.
            let tab_h = package_tab_height(font_px, r.height());
            let tab_w = package_tab_width(deco.name, font_px, r.width());
            let body = Rect::from_min_max(pos2(r.left(), r.top() + tab_h), r.max);
            let tab = Rect::from_min_size(r.min, vec2(tab_w, tab_h));
            painter.rect(body, 0.0, fill, stroke, StrokeKind::Middle);
            painter.rect(tab, 0.0, fill, stroke, StrokeKind::Middle);
        }
        ShapeKind::Directory => {
            // A folder: a body with a small tab on top.
            let tab_h = (r.height() * 0.18).min(10.0 * zoom);
            let body = Rect::from_min_max(pos2(r.left(), r.top() + tab_h), r.max);
            let tab = vec![
                pos2(r.left(), r.top()),
                pos2(r.left() + r.width() * 0.36, r.top()),
                pos2(r.left() + r.width() * 0.44, r.top() + tab_h),
                pos2(r.left(), r.top() + tab_h),
            ];
            painter.add(Shape::convex_polygon(tab, fill, stroke));
            painter.rect(body, 0.0, fill, stroke, StrokeKind::Middle);
        }
        ShapeKind::Rectangle | ShapeKind::Text => {
            painter.rect(r, 0.0, fill, stroke, StrokeKind::Middle);
        }
        ShapeKind::Rounded => {
            let radius = (10.0 * zoom).min(r.width().min(r.height()) / 2.0);
            painter.rect(r, radius, fill, stroke, StrokeKind::Middle);
        }
        ShapeKind::Ellipse => {
            painter.add(Shape::ellipse_filled(r.center(), r.size() / 2.0, fill));
            painter.add(Shape::ellipse_stroke(r.center(), r.size() / 2.0, stroke));
        }
        ShapeKind::Cylinder => {
            let rx = r.width() / 2.0;
            let ry = (15.0 * zoom).min(r.height() / 4.0);
            let cx = r.center().x;
            let top = pos2(cx, r.top() + ry);
            let bottom = pos2(cx, r.bottom() - ry);
            // Body fill: bottom cap + side rect + top cap.
            painter.add(Shape::ellipse_filled(bottom, vec2(rx, ry), fill));
            painter.rect_filled(Rect::from_min_max(pos2(r.left(), top.y), pos2(r.right(), bottom.y)), 0.0, fill);
            painter.add(Shape::ellipse_filled(top, vec2(rx, ry), fill));
            // Outline: sides + front half of the bottom cap + full top cap.
            let mut path = vec![pos2(r.left(), top.y)];
            for i in 0..=24 {
                let t = std::f32::consts::PI * (1.0 - i as f32 / 24.0);
                path.push(pos2(cx + rx * t.cos(), bottom.y + ry * t.sin()));
            }
            path.push(pos2(r.right(), top.y));
            painter.add(Shape::line(path, stroke));
            painter.add(Shape::ellipse_stroke(top, vec2(rx, ry), stroke));
        }
        k if k.is_collection() => {
            // Two stacked cards ("many"), stereotype at the top of the front card.
            let off = crate::model::collection_offset(r.width() / zoom, r.height() / zoom) * zoom;
            let back = Rect::from_min_max(pos2(r.left() + off, r.top()), pos2(r.right(), r.bottom() - off));
            let front = Rect::from_min_max(pos2(r.left(), r.top() + off), pos2(r.right() - off, r.bottom()));
            painter.rect(back, 2.0, fill, stroke, StrokeKind::Middle);
            painter.rect(front, 2.0, fill, stroke, StrokeKind::Middle);
            paint_stereotype(front);
        }
        ShapeKind::File => {
            if let Some(pts) = outline(kind, r) {
                painter.add(Shape::convex_polygon(pts, fill, stroke));
            }
            let f = crate::model::file_fold(r.width(), r.height());
            let fold = vec![pos2(r.right() - f, r.top()), pos2(r.right() - f, r.top() + f), pos2(r.right(), r.top() + f)];
            painter.add(Shape::line(fold, stroke));
        }
        _ => {
            if let Some(pts) = outline(kind, r) {
                painter.add(Shape::convex_polygon(pts, fill, stroke));
            }
        }
    }
}

/// Paints an object's members inside its screen rect `r` (compartment lines and member text).
pub fn paint_members(painter: &Painter, n: &Node, r: Rect, zoom: f32, stroke: Stroke) {
    let layout = n.body_layout();
    for y in &layout.separators {
        let y = r.top() + y * zoom;
        if y < r.bottom() {
            painter.line_segment([pos2(r.left(), y), pos2(r.right(), y)], stroke);
        }
    }
    let size = n.font_size * zoom;
    if size < 3.0 {
        return;
    }
    let color = tone_ink(Color32::from_rgba_unmultiplied(n.text_color[0], n.text_color[1], n.text_color[2], n.text_color[3]));
    let clipped = painter.with_clip_rect(r.shrink(1.0).intersect(painter.clip_rect()));
    for (m, y) in layout.lines {
        let format = egui::TextFormat {
            font_id: FontId::proportional(size),
            color,
            underline: if m.is_static { Stroke::new(1.0, color) } else { Stroke::NONE },
            ..Default::default()
        };
        let galley = clipped.layout_job(LayoutJob::single_section(m.text(), format));
        let pos = pos2(r.left() + layout.pad * zoom, r.top() + y * zoom - galley.size().y / 2.0);
        clipped.galley(pos, galley, color);
    }
}

/// Paints centered, wrapped text inside `r`.
pub fn paint_text(painter: &Painter, r: Rect, text: &str, size: f32, color: Color32) {
    if text.is_empty() || size < 2.0 {
        return;
    }
    let galley = layout_centered(painter, text, size, color, (r.width() - 4.0).max(10.0));
    let pos = pos2(r.center().x, r.center().y - galley.size().y / 2.0);
    painter.galley(pos, galley, color);
}

pub fn layout_centered(
    painter: &Painter,
    text: &str,
    size: f32,
    color: Color32,
    wrap: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), FontId::proportional(size), color, wrap);
    job.halign = Align::Center;
    painter.layout_job(job)
}

pub fn paint_edge(
    painter: &Painter,
    pts: &[Pos2],
    color: Color32,
    width: f32,
    pattern: LinePattern,
    start: Head,
    end: Head,
    zoom: f32,
) {
    let (line, heads) = edge_geometry(pts, head_len(width, zoom), start, end);
    let stroke = Stroke::new(width, color);
    // Dash lengths follow the line width but never shrink below a visible size when zoomed out.
    let (dashes, gaps) = pattern.dashes(width.max(zoom));
    if dashes.is_empty() {
        painter.add(Shape::line(line, stroke));
    } else {
        painter.extend(Shape::dashed_line_with_offset(&line, stroke, &dashes, &gaps, 0.0));
    }
    for h in heads {
        match h.fill {
            HeadFill::Solid => painter.add(Shape::convex_polygon(h.points, color, Stroke::NONE)),
            HeadFill::Hollow => painter.add(Shape::convex_polygon(h.points, canvas_color(), stroke)),
            HeadFill::Open => painter.add(Shape::line(h.points, stroke)),
        };
    }
}

/// Arrowhead length in screen pixels for a line of the given (screen) width.
pub fn head_len(width: f32, zoom: f32) -> f32 {
    10.0 * zoom + width
}

/// Small text tag (pointer kind) beside a line near its source end, on a background so it stays readable.
pub fn paint_badge(painter: &Painter, pts: &[Pos2], head_len: f32, text: &str, size: f32, color: Color32) {
    if size < 3.0 {
        return;
    }
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::monospace(size), color);
    let box_size = galley.size() + vec2(6.0, 2.0);
    let rect = Rect::from_center_size(badge_center(pts, head_len, box_size, true), box_size);
    painter.rect(rect, 3.0, badge_color(), Stroke::new(1.0, color.gamma_multiply(0.5)), StrokeKind::Inside);
    painter.galley(rect.center() - galley.size() / 2.0, galley, color);
}

#[cfg(test)]
mod theme_tests {
    use super::*;

    #[test]
    fn dark_theme_swaps_light_fills_and_dark_ink_only() {
        set_dark(true);
        let white = Color32::WHITE;
        let black = Color32::BLACK;
        let pastel = Color32::from_rgb(225, 236, 250);
        let red = Color32::from_rgb(214, 39, 40);
        assert!(lightness(tone_fill(white)) < 0.3, "white boxes become dark surfaces");
        assert!(lightness(tone_fill(pastel)) < 0.3, "pastel boxes too");
        let p = tone_fill(pastel).to_srgba_unmultiplied();
        assert!(p[2] > p[0], "keeping a hint of their hue");
        assert!(lightness(tone_ink(black)) > 0.7, "black lines and text become light");
        assert_eq!(tone_ink(red), red, "strong colours stay");
        assert_eq!(tone_fill(Color32::TRANSPARENT), Color32::TRANSPARENT);
        set_dark(false);
        assert_eq!(tone_fill(white), white);
        assert_eq!(tone_ink(black), black);
    }
}
