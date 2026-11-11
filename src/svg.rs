//! SVG export.

use std::fmt::Write;

use egui::{vec2, Pos2, Rect};

use crate::model::{arrow_geometry, outline, polyline_midpoint, Diagram, Rgba, ShapeKind};

fn hex(c: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn alpha(c: Rgba) -> f32 {
    c[3] as f32 / 255.0
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn points(pts: &[Pos2]) -> String {
    pts.iter().map(|p| format!("{:.1},{:.1}", p.x, p.y)).collect::<Vec<_>>().join(" ")
}

fn text_block(out: &mut String, center: Pos2, text: &str, size: f32, color: Rgba) {
    let lines: Vec<&str> = text.lines().collect();
    let lh = size * 1.2;
    for (i, line) in lines.iter().enumerate() {
        let y = center.y + (i as f32 - (lines.len() as f32 - 1.0) / 2.0) * lh;
        let _ = writeln!(
            out,
            r#"<text x="{:.1}" y="{:.1}" font-size="{size}" fill="{}" fill-opacity="{}" text-anchor="middle" dominant-baseline="middle">{}</text>"#,
            center.x,
            y,
            hex(color),
            alpha(color),
            esc(line)
        );
    }
}

pub fn export(d: &Diagram) -> String {
    let b = d.bounds().unwrap_or(Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0))).expand(20.0);
    let mut out = String::new();
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="{x:.1} {y:.1} {w:.1} {h:.1}" font-family="Helvetica, Arial, sans-serif">"#,
        x = b.min.x,
        y = b.min.y,
        w = b.width(),
        h = b.height()
    );
    let _ = writeln!(
        out,
        r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="white"/>"#,
        b.min.x,
        b.min.y,
        b.width(),
        b.height()
    );

    for n in &d.nodes {
        let r = n.rect();
        let paint = format!(
            r#"fill="{}" fill-opacity="{}" stroke="{}" stroke-opacity="{}" stroke-width="{}""#,
            hex(n.fill),
            alpha(n.fill),
            hex(n.stroke),
            alpha(n.stroke),
            n.stroke_width
        );
        match n.kind {
            ShapeKind::Rectangle | ShapeKind::Text | ShapeKind::Rounded => {
                let rx = if n.kind == ShapeKind::Rounded { 10.0f32.min(r.width().min(r.height()) / 2.0) } else { 0.0 };
                let _ = writeln!(
                    out,
                    r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{rx:.1}" {paint}/>"#,
                    r.min.x,
                    r.min.y,
                    r.width(),
                    r.height()
                );
            }
            ShapeKind::Ellipse => {
                let c = r.center();
                let _ = writeln!(
                    out,
                    r#"<ellipse cx="{:.1}" cy="{:.1}" rx="{:.1}" ry="{:.1}" {paint}/>"#,
                    c.x,
                    c.y,
                    r.width() / 2.0,
                    r.height() / 2.0
                );
            }
            ShapeKind::Cylinder => {
                let rx = r.width() / 2.0;
                let ry = 15.0f32.min(r.height() / 4.0);
                let (l, rr, ty, by) = (r.left(), r.right(), r.top() + ry, r.bottom() - ry);
                let _ = writeln!(
                    out,
                    r#"<path d="M{l:.1},{ty:.1} L{l:.1},{by:.1} A{rx:.1},{ry:.1} 0 0 0 {rr:.1},{by:.1} L{rr:.1},{ty:.1} Z" {paint}/>"#
                );
                let _ = writeln!(
                    out,
                    r#"<ellipse cx="{:.1}" cy="{ty:.1}" rx="{rx:.1}" ry="{ry:.1}" {paint}/>"#,
                    r.center().x
                );
            }
            _ => {
                if let Some(pts) = outline(n.kind, r) {
                    let _ = writeln!(out, r#"<polygon points="{}" {paint}/>"#, points(&pts));
                }
            }
        }
        text_block(&mut out, r.center(), &n.text, n.font_size, n.text_color);
    }

    for e in &d.edges {
        let Some(path) = d.edge_path(e) else { continue };
        let (line, heads) = arrow_geometry(&path, 10.0 + e.width, e.start_arrow, e.end_arrow);
        let dash = if e.dashed { r#" stroke-dasharray="6 4""# } else { "" };
        let _ = writeln!(
            out,
            r#"<polyline points="{}" fill="none" stroke="{}" stroke-opacity="{}" stroke-width="{}"{dash}/>"#,
            points(&line),
            hex(e.color),
            alpha(e.color),
            e.width
        );
        for h in heads {
            let _ = writeln!(out, r#"<polygon points="{}" fill="{}"/>"#, points(&h), hex(e.color));
        }
        if !e.label.is_empty() {
            let mid = polyline_midpoint(&path);
            let lines = e.label.lines().count().max(1) as f32;
            let longest = e.label.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
            let (w, h) = (longest * 6.5 + 6.0, lines * 13.2 + 2.0);
            let _ = writeln!(
                out,
                r#"<rect x="{:.1}" y="{:.1}" width="{w:.1}" height="{h:.1}" fill="white"/>"#,
                mid.x - w / 2.0,
                mid.y - h / 2.0
            );
            text_block(&mut out, mid, &e.label, 11.0, e.color);
        }
    }

    out.push_str("</svg>\n");
    out
}
