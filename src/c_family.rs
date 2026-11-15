//! C and C++ colouring with tree-sitter: the real grammars, so C++ literals (`0x8000'0000`,
//! `R"(...)"`, `1.5_km`), templates and macros are understood, and a mistake in the code only
//! affects the code around it. After an edit only the changed part of the file is parsed and
//! coloured again.

use std::ops::Range;
use std::sync::OnceLock;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};
use syntect::highlighting::{FontStyle, Highlighter, Theme};
use syntect::parsing::Scope;
use tree_sitter::{InputEdit, Language, Parser, Point, Query, QueryCursor, StreamingIterator, Tree};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    C,
    Cpp,
}

/// The grammar for a file format (extension). Headers are C++: the C++ grammar also reads C.
pub fn lang_of(format: &str) -> Option<Lang> {
    match format {
        "c" => Some(Lang::C),
        "h" | "hh" | "hpp" | "hxx" | "h++" | "cpp" | "cc" | "cxx" | "c++" | "inl" | "ipp" | "tpp" | "ino" => {
            Some(Lang::Cpp)
        }
        _ => None,
    }
}

struct Grammar {
    language: Language,
    query: Query,
}

fn grammar(lang: Lang) -> &'static Grammar {
    static C: OnceLock<Grammar> = OnceLock::new();
    static CPP: OnceLock<Grammar> = OnceLock::new();
    let make = |language: Language, source: String| {
        let mut query = Query::new(&language, &source).expect("the bundled highlight queries are valid");
        // Every identifier is a `variable` (not coloured), and that pattern comes before the
        // ones for function names and CONSTANTS, so it would hide them.
        query.disable_capture("variable");
        Grammar { language, query }
    };
    match lang {
        Lang::C => C.get_or_init(|| make(tree_sitter_c::LANGUAGE.into(), tree_sitter_c::HIGHLIGHT_QUERY.to_owned())),
        // The C++ query only adds to the C one; its patterns come first so they win.
        Lang::Cpp => CPP.get_or_init(|| {
            make(
                tree_sitter_cpp::LANGUAGE.into(),
                format!("{}\n{}", tree_sitter_cpp::HIGHLIGHT_QUERY, tree_sitter_c::HIGHLIGHT_QUERY),
            )
        }),
    }
}

/// The colour and italics for each capture of `query`, taken from the syntect `theme` the other
/// formats use, so C and C++ look like the rest. `None` keeps the plain text colour.
fn capture_styles(query: &Query, theme: &Theme) -> Vec<Option<(Color32, bool)>> {
    let highlighter = Highlighter::new(theme);
    query
        .capture_names()
        .iter()
        .map(|name| {
            let scope = match *name {
                "comment" => "comment",
                "string" => "string",
                "number" => "constant.numeric",
                "constant" => "constant.language",
                "keyword" => "keyword",
                "operator" => "keyword.operator",
                "type" => "entity.name.class",
                "function" => "entity.name.function",
                "function.special" => "support.function",
                "property" => "variable.other.member",
                "label" => "entity.name.tag",
                "variable.builtin" => "variable.language",
                _ => return None,
            };
            let style = highlighter.style_for_stack(&[Scope::new(scope).expect("valid scope")]);
            let fg = style.foreground;
            Some((Color32::from_rgb(fg.r, fg.g, fg.b), style.font_style.contains(FontStyle::ITALIC)))
        })
        .collect()
}

const NONE: u16 = u16::MAX;

/// One editor's parse of its text, kept between frames.
struct Parsed {
    key: (Lang, bool),
    parser: Parser,
    tree: Tree,
    text: String,
    /// The capture colouring each byte of `text` (`NONE` for plain text).
    paint: Vec<u16>,
    job: LayoutJob,
}

thread_local! {
    static PARSES: std::cell::RefCell<std::collections::HashMap<egui::Id, Parsed>> = Default::default();
}

/// The coloured text of `code` in the editor `editor`.
pub fn highlight(editor: egui::Id, code: &str, lang: Lang, dark: bool, theme: &Theme, plain: Color32, font: FontId) -> LayoutJob {
    let g = grammar(lang);
    PARSES.with_borrow_mut(|parses| {
        let key = (lang, dark);
        if let Some(p) = parses.get_mut(&editor).filter(|p| p.key == key) {
            if p.text == code && p.job.sections.first().is_none_or(|s| s.format.font_id == font) {
                return p.job.clone();
            }
            // Tell tree-sitter what changed, so it reuses the rest of the old tree.
            let (start, old_end, new_end) = changed_span(&p.text, code);
            let edit = InputEdit {
                start_byte: start,
                old_end_byte: old_end,
                new_end_byte: new_end,
                start_position: point_at(code, start),
                old_end_position: point_at(&p.text, old_end),
                new_end_position: point_at(code, new_end),
            };
            p.tree.edit(&edit);
            if let Some(tree) = p.parser.parse(code, Some(&p.tree)) {
                // Recolour the edited bytes and whatever the edit changed the meaning of.
                let mut dirty = start..new_end;
                for r in p.tree.changed_ranges(&tree) {
                    dirty = dirty.start.min(r.start_byte)..dirty.end.max(r.end_byte);
                }
                let mut paint = Vec::with_capacity(code.len());
                paint.extend_from_slice(&p.paint[..start]);
                paint.resize(new_end, NONE);
                paint.extend_from_slice(&p.paint[old_end..]);
                paint_range(&mut paint, &tree, code, g, dirty);
                p.tree = tree;
                p.paint = paint;
                p.text = code.to_owned();
                p.job = layout(code, &p.paint, &capture_styles(&g.query, theme), plain, font);
                return p.job.clone();
            }
        }
        // First time (or another language or theme): parse it all.
        let mut parser = Parser::new();
        parser.set_language(&g.language).expect("grammar matches the tree-sitter version");
        let tree = parser.parse(code, None).expect("no timeout or cancellation is set");
        let mut paint = vec![NONE; code.len()];
        paint_range(&mut paint, &tree, code, g, 0..code.len());
        let job = layout(code, &paint, &capture_styles(&g.query, theme), plain, font);
        parses.insert(editor, Parsed { key, parser, tree, text: code.to_owned(), paint, job: job.clone() });
        job
    })
}

/// The bytes that differ between `old` and `new`: (start, end in `old`, end in `new`), all on
/// character boundaries.
fn changed_span(old: &str, new: &str) -> (usize, usize, usize) {
    let mut start = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    while !new.is_char_boundary(start) || !old.is_char_boundary(start) {
        start -= 1;
    }
    let max = old.len().min(new.len()) - start;
    let mut suffix = old.bytes().rev().zip(new.bytes().rev()).take(max).take_while(|(a, b)| a == b).count();
    while !new.is_char_boundary(new.len() - suffix) || !old.is_char_boundary(old.len() - suffix) {
        suffix -= 1;
    }
    (start, old.len() - suffix, new.len() - suffix)
}

/// Row and column (in bytes) of byte `at`.
fn point_at(text: &str, at: usize) -> Point {
    let before = &text.as_bytes()[..at];
    let row = before.iter().filter(|&&b| b == b'\n').count();
    let column = at - before.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    Point { row, column }
}

/// Colours the bytes of `range` from the captures over them. Bigger nodes are painted first so
/// the nodes inside them win, and for the same node the first pattern in the query wins.
fn paint_range(paint: &mut [u16], tree: &Tree, code: &str, g: &Grammar, range: Range<usize>) {
    if range.is_empty() {
        return;
    }
    let mut captures: Vec<(Range<usize>, usize, u16)> = Vec::new();
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range.clone());
    let mut matches = cursor.matches(&g.query, tree.root_node(), code.as_bytes());
    while let Some(m) = matches.next() {
        for c in m.captures() {
            captures.push((c.node.byte_range(), m.pattern_index, c.index as u16));
        }
    }
    captures.sort_by_key(|(r, pattern, _)| (std::cmp::Reverse(r.len()), std::cmp::Reverse(*pattern)));
    paint[range.clone()].fill(NONE);
    for (r, _, capture) in captures {
        let from = r.start.max(range.start);
        let to = r.end.min(range.end);
        if from < to {
            paint[from..to].fill(capture);
        }
    }
}

fn layout(code: &str, paint: &[u16], styles: &[Option<(Color32, bool)>], plain: Color32, font: FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    let style = |i: u16| styles.get(i as usize).copied().flatten().unwrap_or((plain, false));
    let mut start = 0;
    while start < code.len() {
        let s = style(paint[start]);
        let mut end = start + 1;
        while end < code.len() && (!code.is_char_boundary(end) || style(paint[end]) == s) {
            end += 1;
        }
        let format = TextFormat { font_id: font.clone(), color: s.0, italics: s.1, ..Default::default() };
        job.append(&code[start..end], 0.0, format);
        start = end;
    }
    job.wrap.max_width = f32::INFINITY;
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> &'static Theme {
        &crate::code::syntect().ts.themes["base16-mocha.dark"]
    }

    fn colours(job: &LayoutJob) -> Vec<(String, Color32)> {
        job.sections.iter().map(|s| (job.text[s.byte_range.start.0..s.byte_range.end.0].to_owned(), s.format.color)).collect()
    }

    fn colour_of(job: &LayoutJob, piece: &str) -> Color32 {
        colours(job).into_iter().find(|(t, _)| t.contains(piece)).unwrap_or_else(|| panic!("no {piece:?}")).1
    }

    pub(super) const HEADER: &str = "/** @brief A cast. */\nconstexpr uint64_t u2p63 = 0x8000'0000'0000'0000ULL;\nauto s = R\"(a \" b)\";\n/* note */\nconstexpr uint64_t offsetCastUint(const int64_t a)\n{\n    return static_cast<uint64_t>(a) + u2p63; // done\n}\n";

    #[test]
    fn cpp_literals_do_not_break_the_colours_after_them() {
        let job = highlight(egui::Id::unique("hdr"), HEADER, Lang::Cpp, true, theme(), Color32::WHITE, FontId::monospace(12.0));
        assert_eq!(job.text, HEADER);
        let comment = colour_of(&job, "note");
        assert_eq!(colour_of(&job, "@brief"), comment);
        assert_eq!(colour_of(&job, "// done"), comment);
        assert_ne!(colour_of(&job, "constexpr"), comment);
        assert_ne!(colour_of(&job, "return"), comment);
        assert_ne!(colour_of(&job, "offsetCastUint"), Color32::WHITE, "function names are coloured");
        assert_ne!(colour_of(&job, "R\"(a"), colour_of(&job, "return"));
    }

    #[test]
    fn incremental_colours_match_colouring_from_scratch() {
        let edits = [
            HEADER.replace("note", "notes"),
            HEADER.replace("/* note */\n", ""),
            HEADER.replace("\n{\n", "\n/*{\n"), // opens a comment that runs to the next `*/`
            HEADER.replace("ULL;", "ULL;\nint added = 3;"),
            HEADER.replace("cast", "cåst"),
            format!("{HEADER}\"open string"),
        ];
        let font = FontId::monospace(12.0);
        for edited in edits {
            let id = egui::Id::unique(("inc", &edited));
            highlight(id, HEADER, Lang::Cpp, true, theme(), Color32::WHITE, font.clone());
            let incremental = highlight(id, &edited, Lang::Cpp, true, theme(), Color32::WHITE, font.clone());
            let fresh = highlight(egui::Id::unique(("fresh", &edited)), &edited, Lang::Cpp, true, theme(), Color32::WHITE, font.clone());
            assert_eq!(incremental.text, edited);
            assert_eq!(colours(&incremental), colours(&fresh), "for {edited:?}");
        }
    }

    #[test]
    fn c_files_use_the_c_grammar() {
        assert_eq!(lang_of("c"), Some(Lang::C));
        assert_eq!(lang_of("h"), Some(Lang::Cpp));
        assert_eq!(lang_of("rs"), None);
        let code = "#include <stdio.h>\nint main(void) { /* hi */ return 0; }\n";
        let job = highlight(egui::Id::unique("c"), code, Lang::C, true, theme(), Color32::WHITE, FontId::monospace(12.0));
        assert_ne!(colour_of(&job, "hi"), colour_of(&job, "return"));
    }
}

#[cfg(test)]
mod timing {
    use super::*;

    /// Run with `cargo test --release time_c_family -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn time_c_family() {
        let code = tests::HEADER.repeat(500);
        let theme = &crate::code::syntect().ts.themes["base16-mocha.dark"];
        let font = FontId::monospace(13.0);
        let id = egui::Id::unique("c timing");
        let run = |what: &str, code: &str| {
            let t = std::time::Instant::now();
            highlight(id, code, Lang::Cpp, true, theme, Color32::WHITE, font.clone());
            eprintln!("{what}: {:?}", t.elapsed());
        };
        run(&format!("{} lines, first colouring", code.lines().count()), &code);
        let mid = code[..code.len() / 2].rfind('\n').unwrap() + 1;
        let edited = format!("{}x{}", &code[..mid], &code[mid..]);
        run("after typing a character", &edited);
        run("unchanged", &edited);
        run("after opening a comment near the top", &edited.replacen("\n", "\n/*\n", 1));
    }
}
