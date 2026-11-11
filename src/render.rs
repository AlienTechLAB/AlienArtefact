//! Painting shapes and edges in screen space. Shared by the canvas and the shape palette.

use egui::{
    epaint::text::LayoutJob, pos2, vec2, Align, Color32, FontId, Painter, Pos2, Rect, Shape,
    Stroke, StrokeKind,
};

use crate::model::{badge_center, edge_geometry, header_height, stereotype_height, outline, Head, HeadFill, LinePattern, ShapeKind};

/// `font_px` is the shape's font size in screen pixels (sizes the class name compartment).
pub fn paint_shape(
    painter: &Painter,
    kind: ShapeKind,
    r: Rect,
    fill: Color32,
    stroke: Stroke,
    zoom: f32,
    font_px: f32,
) {
    match kind {
        ShapeKind::Class | ShapeKind::Trait => {
            painter.rect(r, 0.0, fill, stroke, StrokeKind::Middle);
            let y = r.top() + header_height(kind, font_px, r.height());
            if y < r.bottom() {
                painter.line_segment([pos2(r.left(), y), pos2(r.right(), y)], stroke);
            }
            // The stereotype is skipped when too small to read (e.g. palette icons).
            if let Some(stereotype) = kind.stereotype().filter(|_| font_px * 0.85 >= 5.0) {
                let band = Rect::from_min_size(r.min, vec2(r.width(), stereotype_height(font_px, r.height())));
                paint_text(painter, band, stereotype, font_px * 0.85, stroke.color);
            }
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
        _ => {
            if let Some(pts) = outline(kind, r) {
                painter.add(Shape::convex_polygon(pts, fill, stroke));
            }
        }
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
            HeadFill::Hollow => painter.add(Shape::convex_polygon(h.points, Color32::WHITE, stroke)),
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
    painter.rect(rect, 3.0, Color32::from_gray(248), Stroke::new(1.0, color.gamma_multiply(0.5)), StrokeKind::Inside);
    painter.galley(rect.center() - galley.size() / 2.0, galley, color);
}
