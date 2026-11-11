//! SVG export.

use std::fmt::Write;

use egui::{vec2, Pos2, Rect};

use crate::model::{badge_center, edge_geometry, label_center, outline, stereotype_height, Diagram, HeadFill, Rgba, ShapeKind};

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

    // Packages first: they are frames behind the objects they hold.
    let (packages, objects): (Vec<&crate::model::Node>, Vec<&crate::model::Node>) =
        d.nodes.iter().partition(|n| n.kind == ShapeKind::Package);
    for n in packages.into_iter().chain(objects) {
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
            ShapeKind::External => {
                let _ = writeln!(
                    out,
                    r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="3" stroke-dasharray="6 4" {paint}/>"#,
                    r.min.x,
                    r.min.y,
                    r.width(),
                    r.height()
                );
            }
            ShapeKind::Package => {
                let tab = n.package_tab();
                for b in [Rect::from_min_max(egui::pos2(r.left(), tab.bottom()), r.max), tab] {
                    let _ = writeln!(
                        out,
                        r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" {paint}/>"#,
                        b.min.x,
                        b.min.y,
                        b.width(),
                        b.height()
                    );
                }
            }
            ShapeKind::Directory => {
                let tab_h = (r.height() * 0.18).min(10.0);
                let tab = [
                    egui::pos2(r.left(), r.top()),
                    egui::pos2(r.left() + r.width() * 0.36, r.top()),
                    egui::pos2(r.left() + r.width() * 0.44, r.top() + tab_h),
                    egui::pos2(r.left(), r.top() + tab_h),
                ];
                let _ = writeln!(out, r#"<polygon points="{}" {paint}/>"#, points(&tab));
                let _ = writeln!(
                    out,
                    r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" {paint}/>"#,
                    r.min.x,
                    r.min.y + tab_h,
                    r.width(),
                    r.height() - tab_h
                );
            }
            ShapeKind::Class | ShapeKind::Trait | ShapeKind::Enum => {
                let _ = writeln!(
                    out,
                    r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" {paint}/>"#,
                    r.min.x,
                    r.min.y,
                    r.width(),
                    r.height()
                );
                let y = n.text_rect().bottom();
                if y < r.bottom() {
                    let _ = writeln!(
                        out,
                        r#"<line x1="{:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke="{}" stroke-opacity="{}" stroke-width="{}"/>"#,
                        r.left(),
                        r.right(),
                        hex(n.stroke),
                        alpha(n.stroke),
                        n.stroke_width
                    );
                }
            }
            k if k.is_collection() => {
                let off = crate::model::collection_offset(r.width(), r.height());
                for (x, y) in [(r.left() + off, r.top()), (r.left(), r.top() + off)] {
                    let _ = writeln!(
                        out,
                        r#"<rect x="{x:.1}" y="{y:.1}" width="{:.1}" height="{:.1}" rx="2" {paint}/>"#,
                        r.width() - off,
                        r.height() - off
                    );
                }
                if let Some(stereotype) = n.stereotype_text() {
                    let stereotype = stereotype.as_str();
                    let front = n.collection_front();
                    let band = stereotype_height(n.font_size, front.height());
                    let center = egui::pos2(front.center().x, front.top() + band / 2.0);
                    text_block(&mut out, center, stereotype, n.font_size * 0.85, n.stroke);
                }
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
                if n.kind == ShapeKind::File {
                    let f = crate::model::file_fold(r.width(), r.height());
                    let fold = [
                        egui::pos2(r.right() - f, r.top()),
                        egui::pos2(r.right() - f, r.top() + f),
                        egui::pos2(r.right(), r.top() + f),
                    ];
                    let _ = writeln!(out, r#"<polyline points="{}" fill="none" {paint}/>"#, points(&fold));
                }
            }
        }
        if let Some(stereotype) = n.stereotype_text().filter(|_| n.kind.is_object() || n.kind == ShapeKind::External) {
            let stereotype = stereotype.as_str();
            let band = stereotype_height(n.font_size, r.height());
            let center = egui::pos2(r.center().x, r.top() + band / 2.0);
            text_block(&mut out, center, stereotype, n.font_size * 0.85, n.stroke);
        }
        text_block(&mut out, n.text_rect().center(), &d.display_text(n), n.font_size, n.text_color);
        if n.kind.is_object() {
            let layout = n.body_layout();
            for y in &layout.separators {
                let y = r.top() + y;
                if y < r.bottom() {
                    let _ = writeln!(
                        out,
                        r#"<line x1="{:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke="{}" stroke-opacity="{}" stroke-width="{}"/>"#,
                        r.left(),
                        r.right(),
                        hex(n.stroke),
                        alpha(n.stroke),
                        n.stroke_width
                    );
                }
            }
            for (m, y) in layout.lines {
                let underline = if m.is_static { r#" text-decoration="underline""# } else { "" };
                let _ = writeln!(
                    out,
                    r#"<text x="{:.1}" y="{:.1}" font-size="{}" fill="{}" fill-opacity="{}" dominant-baseline="middle"{underline}>{}</text>"#,
                    r.left() + layout.pad,
                    r.top() + y,
                    n.font_size,
                    hex(n.text_color),
                    alpha(n.text_color),
                    esc(&m.text())
                );
            }
        }
    }

    for e in &d.edges {
        let Some(path) = d.edge_path(e) else { continue };
        let (pattern, start, end) = e.look();
        let len = 10.0 + e.width;
        let (line, heads) = edge_geometry(&path, len, start, end);
        let (dashes, gaps) = pattern.dashes(e.width);
        let dash = if dashes.is_empty() {
            String::new()
        } else {
            let arr: Vec<String> = dashes.iter().zip(&gaps).flat_map(|(d, g)| [d.to_string(), g.to_string()]).collect();
            format!(r#" stroke-dasharray="{}""#, arr.join(" "))
        };
        let _ = writeln!(
            out,
            r#"<polyline points="{}" fill="none" stroke="{}" stroke-opacity="{}" stroke-width="{}"{dash}/>"#,
            points(&line),
            hex(e.color),
            alpha(e.color),
            e.width
        );
        let stroke = format!(r#"stroke="{}" stroke-width="{}""#, hex(e.color), e.width);
        for h in heads {
            let pts = points(&h.points);
            let _ = match h.fill {
                HeadFill::Solid => writeln!(out, r#"<polygon points="{pts}" fill="{}"/>"#, hex(e.color)),
                HeadFill::Hollow => writeln!(out, r#"<polygon points="{pts}" fill="white" {stroke}/>"#),
                HeadFill::Open => writeln!(out, r#"<polyline points="{pts}" fill="none" {stroke}/>"#),
            };
        }
        if let Some(badge) = e.badge() {
            let (w, h) = (badge.chars().count() as f32 * 6.0 + 6.0, 13.0);
            let at = badge_center(&path, len, egui::vec2(w, h), true);
            let _ = writeln!(
                out,
                r##"<rect x="{:.1}" y="{:.1}" width="{w:.1}" height="{h:.1}" rx="3" fill="#f8f8f8" stroke="{}" stroke-opacity="0.5"/>"##,
                at.x - w / 2.0,
                at.y - h / 2.0,
                hex(e.color)
            );
            let _ = writeln!(
                out,
                r#"<text x="{:.1}" y="{:.1}" font-size="10" font-family="monospace" fill="{}" text-anchor="middle" dominant-baseline="middle">{}</text>"#,
                at.x,
                at.y,
                hex(e.color),
                esc(&badge)
            );
        }
        if !e.label.is_empty() {
            let lines = e.label.lines().count().max(1) as f32;
            let longest = e.label.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
            let (w, h) = (longest * 6.5 + 6.0, lines * 13.2 + 2.0);
            let mid = label_center(e.pointer.is_some(), &path, len, egui::vec2(w, h));
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
