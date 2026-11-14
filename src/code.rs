//! The code editor used for text files: monospace text with line numbers and syntax colours
//! chosen by the file's format (its extension).

use std::sync::OnceLock;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, TextBuffer, TextStyle, Ui};
use egui_extras::syntax_highlighting::SyntectSettings;
use syntect::highlighting::{FontStyle, HighlightIterator, HighlightState, Highlighter};
use syntect::parsing::{ParseState, ScopeStack};

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

/// The highlighted text of a file in `format`. `editor` identifies the editor showing it: its
/// colours are remembered line by line, so after an edit only the changed lines (and those whose
/// colours it changes, e.g. after opening a comment) are coloured again, as in VS Code.
pub fn highlight(ui: &Ui, editor: egui::Id, code: &str, format: &str) -> LayoutJob {
    let font = TextStyle::Monospace.resolve(ui.style());
    let dark = ui.visuals().dark_mode;
    let plain = |code: &str| LayoutJob::simple(code.to_owned(), font.clone(), palette(dark).text, f32::INFINITY);
    match format {
        "toml" | "ini" => highlight_keys(code, font, &palette(dark)),
        "txt" | "csv" | "log" => plain(code),
        _ => highlight_lines(editor, code, format, dark, font.clone()).unwrap_or_else(|| plain(code)),
    }
}

/// The colours of one line, and the colouring state before and after it.
#[derive(Clone)]
struct LineColours {
    text: String,
    start: (ParseState, HighlightState),
    end: (ParseState, HighlightState),
    /// Coloured pieces: byte range in the line, colour, italic.
    pieces: Vec<(std::ops::Range<usize>, Color32, bool)>,
}

/// One editor's remembered colours (for one format and theme).
#[derive(Default)]
struct ColourCache {
    key: (String, bool),
    lines: Vec<LineColours>,
}

thread_local! {
    static COLOUR_CACHES: std::cell::RefCell<std::collections::HashMap<egui::Id, ColourCache>> =
        Default::default();
}

/// Colours `code` with syntect, reusing the remembered colours of unchanged lines: the lines
/// before the first change, and the lines after the last change once the colouring state there is
/// the same as before. `None` when syntect does not know `format`.
fn highlight_lines(editor: egui::Id, code: &str, format: &str, dark: bool, font: FontId) -> Option<LayoutJob> {
    let set = syntect();
    let syntax = set.ps.find_syntax_by_extension(format).or_else(|| set.ps.find_syntax_by_name(format))?;
    let theme = &set.ts.themes[if dark { "base16-mocha.dark" } else { "Solarized (light)" }];
    let highlighter = Highlighter::new(theme);
    let new: Vec<&str> = syntect::util::LinesWithEndings::from(code).collect();

    COLOUR_CACHES.with_borrow_mut(|caches| {
        let cache = caches.entry(editor).or_default();
        let key = (format.to_owned(), dark);
        if cache.key != key {
            *cache = ColourCache { key, lines: Vec::new() };
        }
        let old = std::mem::take(&mut cache.lines);
        // Unchanged lines at the start and (shifted by inserted or removed lines) at the end.
        let prefix = old.iter().zip(&new).take_while(|(o, n)| o.text == **n).count();
        let max_suffix = old.len().min(new.len()) - prefix;
        let suffix = old.iter().rev().zip(new.iter().rev()).take(max_suffix).take_while(|(o, n)| o.text == **n).count();

        // Lines are moved out of the old list when reused, not copied.
        let mut old: Vec<Option<LineColours>> = old.into_iter().map(Some).collect();
        let shift = old.len() as isize - new.len() as isize;
        let mut lines: Vec<LineColours> = Vec::with_capacity(new.len());
        lines.extend(old[..prefix].iter_mut().map(|o| o.take().expect("each old line is used once")));
        let mut state = match lines.last() {
            Some(l) => l.end.clone(),
            None => (ParseState::new(syntax), HighlightState::new(&highlighter, ScopeStack::new())),
        };
        let mut i = prefix;
        while i < new.len() {
            if i >= new.len() - suffix {
                let o = (i as isize + shift) as usize;
                if old[o].as_ref().is_some_and(|o| o.start == state) {
                    // Back in step with the old colouring: the rest is unchanged.
                    for j in i..new.len() {
                        lines.push(old[(j as isize + shift) as usize].take().expect("each old line is used once"));
                    }
                    break;
                }
            }
            let line = new[i];
            let start = state.clone();
            let ops = state.0.parse_line(line, &set.ps).ok()?;
            let mut pieces = Vec::new();
            let mut at = 0;
            for (style, piece) in HighlightIterator::new(&mut state.1, &ops, line, &highlighter) {
                let fg = style.foreground;
                let italic = style.font_style.contains(FontStyle::ITALIC);
                pieces.push((at..at + piece.len(), Color32::from_rgb(fg.r, fg.g, fg.b), italic));
                at += piece.len();
            }
            lines.push(LineColours { text: line.to_owned(), start, end: state.clone(), pieces });
            i += 1;
        }

        let mut job = LayoutJob::default();
        for l in &lines {
            for (range, color, italics) in &l.pieces {
                let format = TextFormat { font_id: font.clone(), color: *color, italics: *italics, ..Default::default() };
                job.append(&l.text[range.clone()], 0.0, format);
            }
        }
        job.wrap.max_width = f32::INFINITY;
        cache.lines = lines;
        Some(job)
    })
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

/// A position in a text: line and column, both counted from 1 (columns in characters).
pub type LineCol = (usize, usize);

/// A selected part of a text file, from `start` up to (not including) `end`, as in VS Code.
/// The app keeps the last one so it can be referred to later (e.g. when asking an AI about it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeSelection {
    /// Where the text is: a text file tab (`node` is `None`), or the diagram tab holding a file
    /// object and that object.
    pub item: u64,
    pub node: Option<u64>,
    /// The file's name when the selection was made.
    pub file: String,
    pub start: LineCol,
    pub end: LineCol,
    /// The selected text (empty when only the cursor is placed).
    pub text: String,
}

impl CodeSelection {
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// How many lines the selection covers (0 when nothing is selected). A selection ending at
    /// the very start of a line does not count that line.
    pub fn line_count(&self) -> usize {
        if self.is_empty() {
            return 0;
        }
        let last = if self.end.1 == 1 && self.end.0 > self.start.0 { self.end.0 - 1 } else { self.end.0 };
        last - self.start.0 + 1
    }

    /// A short reference to the selected part: `file:line:col-line:col`, or `file:line:col` for
    /// a cursor.
    pub fn reference(&self) -> String {
        let (l1, c1) = self.start;
        let (l2, c2) = self.end;
        if self.is_empty() {
            format!("{}:{l1}:{c1}", self.file)
        } else {
            format!("{}:{l1}:{c1}-{l2}:{c2}", self.file)
        }
    }

    /// What the status bar says about it.
    pub fn status(&self) -> String {
        let (line, col) = self.end;
        match self.line_count() {
            0 => format!("Ln {line}, Col {col}"),
            n => {
                let chars = self.text.chars().count();
                let lines = if n == 1 { "1 line".to_owned() } else { format!("{n} lines") };
                format!("Ln {line}, Col {col}  ({lines}, {chars} characters selected)")
            }
        }
    }
}

/// The line and column of character `index` (counted from 0) in `text`.
pub fn line_col(text: &str, index: usize) -> LineCol {
    let mut line = 1;
    let mut col = 1;
    for c in text.chars().take(index) {
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// The selection between character indices `a` and `b` (either order) of `text`.
pub fn selection_of(text: &str, a: usize, b: usize, file: &str, item: u64, node: Option<u64>) -> CodeSelection {
    let (from, to) = (a.min(b), a.max(b));
    CodeSelection {
        item,
        node,
        file: file.to_owned(),
        start: line_col(text, from),
        end: line_col(text, to),
        text: text.chars().skip(from).take(to - from).collect(),
    }
}

/// A code editor filling the space: line numbers on the left, highlighted text on the right.
/// Returns the selection (character indices, start and end) while the editor has focus.
pub fn code_editor(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    text: &mut String,
    format: &str,
) -> Option<(usize, usize)> {
    let mut selection = None;
    let lines = text.split('\n').count().max(1);
    let digits = lines.to_string().len().max(3);
    let font = TextStyle::Monospace.resolve(ui.style());
    let gutter_color = ui.visuals().weak_text_color();
    let editor = egui::Id::unique(&id_salt);
    let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, _wrap: f32| {
        let job = highlight(ui, editor, buf.as_str(), format);
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
            let out = egui::TextEdit::multiline(text)
                .code_editor()
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY)
                .min_size(ui.available_size())
                .desired_rows(lines)
                .layouter(&mut layouter)
                .show(ui);
            if out.response.has_focus() {
                if let Some(r) = out.cursor_range {
                    let [a, b] = r.sorted_cursors();
                    selection = Some((a.index.0, b.index.0));
                }
            }
        });
    });
    selection
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
    fn selections_know_their_lines_and_columns() {
        let text = "fn main() {\n    let x = 1;\n}\n";
        let cursor = selection_of(text, 16, 16, "main.rs", 1, Some(2));
        assert_eq!((cursor.start, cursor.line_count()), ((2, 5), 0));
        assert_eq!(cursor.reference(), "main.rs:2:5");
        assert_eq!(cursor.status(), "Ln 2, Col 5");

        // "let x" on line 2, selected backwards.
        let s = selection_of(text, 21, 16, "main.rs", 1, Some(2));
        assert_eq!((s.start, s.end, s.text.as_str()), ((2, 5), (2, 10), "let x"));
        assert_eq!((s.line_count(), s.reference()), (1, "main.rs:2:5-2:10".to_owned()));

        // Whole lines 1-2: ending at the start of line 3 does not count line 3.
        let s = selection_of(text, 0, 27, "main.rs", 1, Some(2));
        assert_eq!((s.end, s.line_count()), ((3, 1), 2));
        assert_eq!(s.status(), "Ln 3, Col 1  (2 lines, 27 characters selected)");
    }

    #[test]
    fn incremental_colours_match_colouring_from_scratch() {
        let font = FontId::monospace(12.0);
        let colours = |job: &LayoutJob| {
            job.sections.iter().map(|s| (job.text[s.byte_range.start.0..s.byte_range.end.0].to_owned(), s.format.color)).collect::<Vec<_>>()
        };
        let base = "fn main() {\n    let x = 1; // one\n    let s = \"two\";\n}\n\nfn other() {}\n";
        let edits = [
            base.replace("let x", "let xy"),                 // typing in a line
            base.replace("fn main() {\n", "fn main() {\n    let added = 3;\n"), // a new line
            base.replace("\n\nfn other", "\n/*\nfn other"), // a comment that colours the lines after it
            base.replace("    let s = \"two\";\n", ""),       // a removed line
        ];
        for edited in edits {
            let id = egui::Id::unique(("inc", edited.len()));
            highlight_lines(id, base, "rs", true, font.clone()).unwrap();
            let incremental = highlight_lines(id, &edited, "rs", true, font.clone()).unwrap();
            let fresh = highlight_lines(egui::Id::unique(("fresh", edited.len())), &edited, "rs", true, font.clone()).unwrap();
            assert_eq!(incremental.text, edited);
            assert_eq!(colours(&incremental), colours(&fresh), "for {edited:?}");
        }
    }

    #[test]
    fn syntect_knows_the_other_code_formats() {
        let ps = &syntect().ps;
        for f in ["json", "c", "h", "cpp", "rs", "py", "html", "xml", "md", "yaml"] {
            assert!(ps.find_syntax_by_extension(f).is_some(), "no syntax for {f}");
        }
    }
}

#[cfg(test)]
mod timing {
    /// Run with `cargo test time_highlighting -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn time_highlighting() {
        use std::time::Instant;
        let code = std::fs::read_to_string("src/app.rs").unwrap();
        let id = egui::Id::unique("timing");
        let font = egui::FontId::monospace(13.0);
        let font2 = font.clone();
        let t = Instant::now();
        let first = super::highlight_lines(id, &code, "rs", true, font.clone()).unwrap();
        eprintln!("{} lines, first colouring: {:?}", code.lines().count(), t.elapsed());
        // One character typed in the middle of the file.
        let mid = code[..code.len() / 2].rfind('\n').unwrap() + 1;
        let edited = format!("{}x{}", &code[..mid], &code[mid..]);
        let t = Instant::now();
        super::highlight_lines(id, &edited, "rs", true, font.clone()).unwrap();
        eprintln!("after typing a character: {:?}", t.elapsed());
        let opened = code.replacen("\n", "\n/*\n", 1);
        let t = Instant::now();
        super::highlight_lines(id, &opened, "rs", true, font).unwrap();
        super::highlight_lines(id, &code, "rs", true, font2.clone()).unwrap();
        eprintln!("after opening a comment near the top: {:?}", t.elapsed());
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| {
            let job = super::highlight_lines(id, &code, "rs", true, font2.clone()).unwrap();
            let t = Instant::now();
            let _ = ui.fonts_mut(|f| f.layout_job(job));
            eprintln!("text layout, first: {:?}", t.elapsed());
            let job = super::highlight_lines(id, &edited, "rs", true, font2.clone()).unwrap();
            let t = Instant::now();
            let _ = ui.fonts_mut(|f| f.layout_job(job));
            eprintln!("text layout after a keystroke: {:?}", t.elapsed());
        });
        assert!(!first.sections.is_empty());
    }
}
