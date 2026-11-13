//! The code editor used for text files: monospace text with line numbers and syntax colours
//! chosen by the file's format (its extension).

use std::sync::OnceLock;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, TextBuffer, TextStyle, Ui};
use egui_extras::syntax_highlighting::{highlight_with, CodeTheme, SyntectSettings};

/// Syntax definitions and themes, loaded once (the highlighter caches by their address).
fn syntect() -> &'static SyntectSettings {
    static SETTINGS: OnceLock<SyntectSettings> = OnceLock::new();
    SETTINGS.get_or_init(SyntectSettings::default)
}

/// Colours for the highlighter written here (formats syntect does not know), after VS Code.
struct Palette {
    text: Color32,
    key: Color32,
    section: Color32,
    string: Color32,
    number: Color32,
    keyword: Color32,
    comment: Color32,
    punct: Color32,
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            text: Color32::from_rgb(212, 212, 212),
            key: Color32::from_rgb(156, 220, 254),
            section: Color32::from_rgb(78, 201, 176),
            string: Color32::from_rgb(206, 145, 120),
            number: Color32::from_rgb(181, 206, 168),
            keyword: Color32::from_rgb(86, 156, 214),
            comment: Color32::from_rgb(106, 153, 85),
            punct: Color32::from_rgb(160, 160, 160),
        }
    } else {
        Palette {
            text: Color32::from_rgb(30, 30, 30),
            key: Color32::from_rgb(0, 16, 128),
            section: Color32::from_rgb(38, 127, 153),
            string: Color32::from_rgb(163, 21, 21),
            number: Color32::from_rgb(9, 134, 88),
            keyword: Color32::from_rgb(0, 0, 255),
            comment: Color32::from_rgb(0, 128, 0),
            punct: Color32::from_rgb(110, 110, 110),
        }
    }
}

/// The highlighted text of a file in `format`.
pub fn highlight(ui: &Ui, code: &str, format: &str) -> LayoutJob {
    let font = TextStyle::Monospace.resolve(ui.style());
    match format {
        "toml" | "ini" => highlight_keys(code, font, &palette(ui.visuals().dark_mode)),
        "txt" | "csv" | "log" => {
            let p = palette(ui.visuals().dark_mode);
            LayoutJob::simple(code.to_owned(), font, p.text, f32::INFINITY)
        }
        _ => {
            let theme = CodeTheme::from_style(ui.style());
            highlight_with(ui.ctx(), ui.style(), &theme, code, format, syntect())
        }
    }
}

/// TOML and INI: `[sections]`, `key = value` lines, strings, numbers, booleans, comments.
fn highlight_keys(code: &str, font: FontId, p: &Palette) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut put = |s: &str, c: Color32| {
        if !s.is_empty() {
            job.append(s, 0.0, TextFormat::simple(font.clone(), c));
        }
    };
    for line in code.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let end = &line[body.len()..];
        let indent = &body[..body.len() - body.trim_start().len()];
        let rest = body.trim_start();
        put(indent, p.text);
        if rest.starts_with('#') || rest.starts_with(';') {
            put(rest, p.comment);
        } else if rest.starts_with('[') {
            put(rest, p.section);
        } else if let Some(eq) = rest.find('=').filter(|&i| !rest[..i].contains(['"', '\''])) {
            put(&rest[..eq], p.key);
            put("=", p.punct);
            value(&rest[eq + 1..], p, &mut put);
        } else {
            value(rest, p, &mut put);
        }
        put(end, p.text);
    }
    job.wrap.max_width = f32::INFINITY;
    job
}

/// A value: strings, numbers, keywords and comments, the rest as plain text.
fn value(s: &str, p: &Palette, put: &mut impl FnMut(&str, Color32)) {
    let b = s.as_bytes();
    let mut i = 0;
    while i < s.len() {
        let c = b[i];
        let start = i;
        if c == b'#' {
            put(&s[i..], p.comment);
            return;
        } else if c == b'"' || c == b'\'' {
            i += 1;
            while i < s.len() && b[i] != c {
                i += if b[i] == b'\\' && c == b'"' { 2 } else { 1 };
            }
            i = (i + 1).min(s.len());
            put(&s[start..i], p.string);
        } else if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'+' || c == b'.' {
            while i < s.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'-' | b'+' | b'.' | b':')) {
                i += 1;
            }
            let word = &s[start..i];
            let color = if matches!(word, "true" | "false" | "inf" | "nan" | "yes" | "no" | "on" | "off") {
                p.keyword
            } else if word.starts_with(|ch: char| ch.is_ascii_digit() || ch == '-' || ch == '+' || ch == '.')
                && word.chars().any(|ch| ch.is_ascii_digit())
            {
                p.number
            } else {
                p.text
            };
            put(word, color);
        } else {
            // One character (may be multi-byte).
            let len = s[i..].chars().next().map_or(1, char::len_utf8);
            i += len;
            let color = if c.is_ascii_punctuation() { p.punct } else { p.text };
            put(&s[start..i], color);
        }
    }
}

/// A code editor filling the space: line numbers on the left, highlighted text on the right.
pub fn code_editor(ui: &mut Ui, id_salt: impl std::hash::Hash + std::fmt::Debug, text: &mut String, format: &str) {
    let lines = text.split('\n').count().max(1);
    let digits = lines.to_string().len().max(3);
    let font = TextStyle::Monospace.resolve(ui.style());
    let gutter_color = ui.visuals().weak_text_color();
    let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, _wrap: f32| {
        let job = highlight(ui, buf.as_str(), format);
        ui.fonts_mut(|f| f.layout_job(job))
    };
    egui::ScrollArea::both().id_salt(id_salt).auto_shrink(false).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            let mut numbers: String =
                (1..=lines).map(|n| format!("{n:>digits$}")).collect::<Vec<_>>().join("\n");
            ui.add(
                egui::TextEdit::multiline(&mut numbers)
                    .font(font.clone())
                    .text_color(gutter_color)
                    .interactive(false)
                    .frame(egui::Frame::NONE)
                    .desired_width(ui.fonts_mut(|f| f.glyph_width(&font, '0')) * digits as f32 + 4.0)
                    .desired_rows(lines),
            );
            ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(text)
                    .code_editor()
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY)
                    .desired_rows(lines)
                    .layouter(&mut layouter),
            );
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colours(code: &str) -> Vec<(String, Color32)> {
        let job = highlight_keys(code, FontId::monospace(12.0), &palette(true));
        job.sections.iter().map(|s| (job.text[s.byte_range.start.0..s.byte_range.end.0].to_owned(), s.format.color)).collect()
    }

    #[test]
    fn toml_keys_sections_values_and_comments_are_coloured() {
        let p = palette(true);
        let c = colours("# config\n[server]\nport = 8080\nname = \"mech\" # main\ndebug = true\n");
        let find = |t: &str| c.iter().find(|(s, _)| s.trim() == t).map(|(_, c)| *c);
        assert_eq!(find("# config"), Some(p.comment));
        assert_eq!(find("[server]"), Some(p.section));
        assert_eq!(find("port"), Some(p.key));
        assert_eq!(find("8080"), Some(p.number));
        assert_eq!(find("\"mech\""), Some(p.string));
        assert_eq!(find("# main"), Some(p.comment));
        assert_eq!(find("true"), Some(p.keyword));
        // Nothing is lost or reordered.
        let all: String = c.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(all, "# config\n[server]\nport = 8080\nname = \"mech\" # main\ndebug = true\n");
    }

    #[test]
    fn syntect_knows_the_other_code_formats() {
        let ps = &syntect().ps;
        for f in ["json", "c", "h", "cpp", "rs", "py", "html", "xml", "md", "yaml"] {
            assert!(ps.find_syntax_by_extension(f).is_some(), "no syntax for {f}");
        }
    }
}
