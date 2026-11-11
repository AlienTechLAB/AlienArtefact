//! Painting shapes and edges in screen space. Shared by the canvas and the shape palette.

use egui::{
    epaint::text::LayoutJob, pos2, vec2, Align, Color32, FontId, Painter, Pos2, Rect, Shape,
    Stroke, StrokeKind,
};

use crate::model::{arrow_geometry, outline, ShapeKind};

pub fn paint_shape(painter: &Painter, kind: ShapeKind, r: Rect, fill: Color32, stroke: Stroke, zoom: f32) {
    match kind {
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
    dashed: bool,
    start_arrow: bool,
    end_arrow: bool,
    zoom: f32,
) {
    let (line, heads) = arrow_geometry(pts, 10.0 * zoom + width, start_arrow, end_arrow);
    let stroke = Stroke::new(width, color);
    if dashed {
        painter.extend(Shape::dashed_line(&line, stroke, 6.0 * zoom, 4.0 * zoom));
    } else {
        painter.add(Shape::line(line, stroke));
    }
    for h in heads {
        painter.add(Shape::convex_polygon(h.to_vec(), color, Stroke::NONE));
    }
}
