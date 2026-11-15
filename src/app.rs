//! Application shell: menus, shape palette, format panel, undo/redo and files.

use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::{vec2, Color32, Event, Key, Modifiers, Pos2, Rect, Sense, Stroke, Ui, Vec2, ViewportCommand};

use crate::canvas::{Drag, LinkError};
use crate::model::{
    Clip, Diagram, EdgeStyle, Head, Id, LinePattern, LinkKind, Member, MemberKind, Node, PointerKind,
    Project, ProjectFile, RelationKind, ShapeKind, Visibility,
};
use crate::tabs::{Content, NewKind, Tab};
use crate::render::{paint_edge, paint_shape, Deco};

const MAX_UNDO: usize = 200;

/// A plain rectangular button as wide as the space it is in, with its label on the left
/// (palette categories, the + menu of the tabs). `chosen` shows it selected.
pub fn rect_button(ui: &mut Ui, label: &str, chosen: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    // The app's own colours, not those of the menu or popup it may be in, so it looks the same
    // everywhere.
    let style = ui.ctx().global_style();
    let visuals = &style.visuals;
    let fill = if chosen {
        visuals.selection.bg_fill
    } else if resp.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        visuals.widgets.inactive.weak_bg_fill
    };
    let text = if chosen { visuals.selection.stroke.color } else { visuals.text_color() };
    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter().text(
        Pos2::new(rect.left() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::TextStyle::Button.resolve(ui.style()),
        text,
    );
    resp
}

/// egui's look for a theme: the dark themes recolour egui's dark visuals to match their canvas.
fn theme_visuals(theme: crate::render::Theme) -> egui::Visuals {
    use crate::render::Theme;
    let rgb = Color32::from_rgb;
    // panel, window, extreme, faint, inactive, inactive weak, hovered, active, frame stroke,
    // selection, selection text.
    let tint = match theme {
        Theme::Light => return egui::Visuals::light(),
        Theme::Graphite => return egui::Visuals::dark(),
        Theme::Neon => [
            rgb(11, 15, 33),
            rgb(15, 20, 42),
            rgb(6, 9, 22),
            rgb(16, 22, 46),
            rgb(24, 33, 68),
            rgb(20, 28, 58),
            rgb(34, 50, 100),
            rgb(40, 62, 128),
            rgb(30, 42, 84),
            rgb(28, 92, 170),
            rgb(170, 225, 255),
        ],
        Theme::Amethyst => [
            rgb(22, 13, 34),
            rgb(28, 18, 44),
            rgb(13, 7, 22),
            rgb(32, 20, 48),
            rgb(48, 30, 72),
            rgb(40, 25, 62),
            rgb(68, 42, 104),
            rgb(86, 52, 132),
            rgb(58, 36, 86),
            rgb(118, 56, 170),
            rgb(240, 214, 255),
        ],
    };
    let [panel, window, extreme, faint, inactive, inactive_weak, hovered, active, frame, sel, sel_text] = tint;
    let accent = theme.palette().map_or(rgb(0, 120, 215), |p| p.line);
    let mut v = egui::Visuals::dark();
    v.panel_fill = panel;
    v.window_fill = window;
    v.window_stroke = Stroke::new(1.0, frame);
    v.extreme_bg_color = extreme;
    v.faint_bg_color = faint;
    v.code_bg_color = faint;
    v.hyperlink_color = accent;
    v.selection.bg_fill = sel;
    v.selection.stroke = Stroke::new(1.0, sel_text);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = panel;
    w.noninteractive.weak_bg_fill = panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, frame);
    w.inactive.bg_fill = inactive;
    w.inactive.weak_bg_fill = inactive_weak;
    w.hovered.bg_fill = hovered;
    w.hovered.weak_bg_fill = hovered;
    w.hovered.bg_stroke = Stroke::new(1.0, accent.gamma_multiply(0.8));
    w.active.bg_fill = active;
    w.active.weak_bg_fill = active;
    w.active.bg_stroke = Stroke::new(1.0, accent);
    w.open.bg_fill = inactive;
    w.open.weak_bg_fill = inactive;
    v
}

/// The explorer icon: two overlapping pages with a folded corner, as in VS Code.
fn files_icon(painter: &egui::Painter, c: Pos2, stroke: Stroke) {
    let p = |x: f32, y: f32| Pos2::new(c.x + x, c.y + y);
    // The page behind: only its visible edges.
    painter.add(egui::Shape::line(vec![p(-8.0, -6.0), p(-8.0, 12.0), p(4.0, 12.0)], stroke));
    // The page in front, with a folded top-right corner.
    painter.add(egui::Shape::closed_line(
        vec![p(-4.0, -12.0), p(4.0, -12.0), p(9.0, -7.0), p(9.0, 8.0), p(-4.0, 8.0)],
        stroke,
    ));
    painter.add(egui::Shape::line(vec![p(4.0, -12.0), p(4.0, -7.0), p(9.0, -7.0)], stroke));
}

/// The search icon: a magnifying glass.
fn search_icon(painter: &egui::Painter, c: Pos2, stroke: Stroke) {
    let lens = Pos2::new(c.x + 2.0, c.y - 2.0);
    painter.circle_stroke(lens, 7.0, stroke);
    painter.line_segment([Pos2::new(lens.x - 5.0, lens.y + 5.0), Pos2::new(c.x - 9.0, c.y + 9.0)], Stroke::new(stroke.width + 0.8, stroke.color));
}

/// What a search option toggle shows.
#[derive(Clone, Copy)]
enum OptionIcon {
    /// "Aa": match case.
    Case,
    /// "ab" underlined: whole words.
    WholeWord,
}

/// A small on/off button drawn in `rect` (inside the search field), highlighted when `on`.
fn option_toggle(ui: &mut Ui, rect: Rect, id: &str, on: bool, tip: &str, icon: OptionIcon) -> egui::Response {
    let resp = ui.interact(rect, ui.scope_id().with(id), Sense::click()).on_hover_text(tip);
    let style = ui.ctx().global_style();
    let v = &style.visuals;
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, 3.0, v.selection.bg_fill.gamma_multiply(0.7));
        painter.rect_stroke(rect, 3.0, Stroke::new(1.0, v.selection.stroke.color), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        painter.rect_filled(rect, 3.0, v.widgets.hovered.weak_bg_fill);
    }
    let ink = if on { v.strong_text_color() } else { v.text_color() };
    let font = egui::FontId::proportional(12.0);
    match icon {
        OptionIcon::Case => {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Aa", font, ink);
        }
        OptionIcon::WholeWord => {
            let c = rect.center();
            painter.text(Pos2::new(c.x, c.y - 1.5), egui::Align2::CENTER_CENTER, "ab", font, ink);
            // A bracket under the letters: the word's ends.
            let (l, r, y) = (c.x - 6.5, c.x + 6.5, c.y + 6.0);
            painter.add(egui::Shape::line(
                vec![Pos2::new(l, y - 2.5), Pos2::new(l, y), Pos2::new(r, y), Pos2::new(r, y - 2.5)],
                Stroke::new(1.0, ink),
            ));
        }
    }
    resp
}

/// A search field with Match Case and Whole Word toggles inside its right end. Alt+C / Alt+W
/// toggle them while typing in it (as in VS Code). `name` makes its ids unique.
pub fn search_field(ui: &mut Ui, name: &str, value: &mut String, opts: &mut crate::search::SearchOptions, hint: &str) -> egui::Response {
    // Handled before the field is drawn, so the letter is not typed into it.
    let field_id = ui.make_persistent_id(name);
    if ui.memory(|m| m.has_focus(field_id)) {
        let (case, word) = ui.input_mut(|i| {
            let c = i.consume_key(Modifiers::ALT, Key::C);
            let w = i.consume_key(Modifiers::ALT, Key::W);
            if c || w {
                i.events.retain(|e| !matches!(e, Event::Text(_)));
            }
            (c, w)
        });
        opts.case_sensitive ^= case;
        opts.whole_word ^= word;
    }
    // Room on the right of the field for the two toggles.
    const TOGGLE: f32 = 22.0;
    let field = egui::TextEdit::singleline(value)
        .id(field_id)
        .hint_text(hint)
        .desired_width(ui.available_width())
        .margin(egui::Margin { left: 6, right: (TOGGLE * 2.0 + 8.0) as i8, top: 5, bottom: 5 });
    let resp = ui.add(field);
    let r = resp.rect;
    let word_rect = Rect::from_min_size(Pos2::new(r.right() - 3.0 - TOGGLE, r.top() + 3.0), vec2(TOGGLE, r.height() - 6.0));
    let case_rect = word_rect.translate(vec2(-(TOGGLE + 2.0), 0.0));
    if option_toggle(ui, case_rect, &format!("{name}_case"), opts.case_sensitive, "Match Case (Alt+C)", OptionIcon::Case).clicked() {
        opts.case_sensitive = !opts.case_sensitive;
    }
    if option_toggle(ui, word_rect, &format!("{name}_word"), opts.whole_word, "Match Whole Word (Alt+W)", OptionIcon::WholeWord).clicked() {
        opts.whole_word = !opts.whole_word;
    }
    resp
}

/// A line containing the searched text, under its file: the line number, then the line with
/// the matches (`ranges`, character ranges of `preview`) highlighted.
fn match_row(ui: &mut Ui, line: usize, preview: &str, ranges: &[(usize, usize)]) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(120.0), 22.0), Sense::click());
    let style = ui.ctx().global_style();
    let v = &style.visuals;
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let font = egui::TextStyle::Body.resolve(ui.style());
    let cy = rect.center().y;
    let small = egui::FontId::proportional(font.size - 1.5);
    let num = ui.painter().text(Pos2::new(rect.left() + 22.0, cy), egui::Align2::LEFT_CENTER, line.to_string(), small, v.weak_text_color());
    // The line, with the matches on the selection colour.
    let mut job = egui::text::LayoutJob::default();
    let plain = egui::TextFormat { font_id: font.clone(), color: v.text_color(), ..Default::default() };
    let marked = egui::TextFormat {
        font_id: font,
        color: v.strong_text_color(),
        background: v.selection.bg_fill.gamma_multiply(0.8),
        ..Default::default()
    };
    let chars: Vec<char> = preview.chars().collect();
    let mut at = 0;
    for &(a, b) in ranges {
        job.append(&chars[at..a].iter().collect::<String>(), 0.0, plain.clone());
        job.append(&chars[a..b].iter().collect::<String>(), 0.0, marked.clone());
        at = b;
    }
    job.append(&chars[at..].iter().collect::<String>(), 0.0, plain);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let pos = Pos2::new(num.right() + 8.0, cy - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, v.text_color());
    resp
}

/// A list of files found, each followed by its lines (search results, references): rows of 22
/// points, only those in view drawn. `counts` shows each file's number of matches. Returns the
/// file clicked, and where for a line: (line from 1, from, to).
fn hit_list(
    ui: &mut Ui,
    salt: &str,
    hits: &[crate::search::Hit],
    counts: bool,
    focused: Option<&std::path::Path>,
) -> Option<(PathBuf, Option<(usize, usize, usize)>)> {
    enum Row {
        File(usize),
        Line(usize, usize),
    }
    let mut rows = Vec::new();
    for (i, hit) in hits.iter().enumerate() {
        rows.push(Row::File(i));
        rows.extend((0..hit.lines.len()).map(|j| Row::Line(i, j)));
    }
    let mut open = None;
    egui::ScrollArea::both().id_salt(salt).auto_shrink(false).show_rows(ui, 22.0, rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in &rows[range] {
            match *row {
                Row::File(i) => {
                    let hit = &hits[i];
                    let selected = focused == Some(hit.path.as_path());
                    let count = if !counts {
                        None
                    } else if hit.binary_matches > 0 {
                        Some(format!("{} (binary)", hit.binary_matches))
                    } else {
                        Some(hit.match_count().to_string())
                    };
                    let r = result_row(ui, &hit.name, &hit.dir, count.as_deref(), selected);
                    if r.on_hover_text(hit.path.display().to_string()).clicked() {
                        open = Some((hit.path.clone(), None));
                    }
                }
                Row::Line(i, j) => {
                    let hit = &hits[i];
                    let m = &hit.lines[j];
                    if match_row(ui, m.line, &m.preview, &m.preview_ranges()).clicked() {
                        let (a, b) = m.cols.first().copied().unwrap_or((0, 0));
                        open = Some((hit.path.clone(), Some((m.line, a, b))));
                    }
                }
            }
        }
    });
    open
}

/// Places found by code navigation, as files with lines: the References panel, or the pop-up
/// list of Go to ... (at `pos`).
pub struct NavList {
    title: String,
    hits: Vec<crate::search::Hit>,
    pos: Option<Pos2>,
}

/// The identifier around character `col` of `line` (letters, digits, `_`), or "".
fn word_at(line: &str, col: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let is_word = |c: &char| c.is_alphanumeric() || *c == '_';
    let col = col.min(chars.len());
    let start = (0..col).rev().take_while(|&i| is_word(&chars[i])).last().unwrap_or(col);
    let end = (col..chars.len()).take_while(|&i| is_word(&chars[i])).last().map_or(col, |i| i + 1);
    chars[start..end].iter().collect()
}

/// The references icon: a list (three bullets and lines).
fn references_icon(painter: &egui::Painter, c: Pos2, stroke: Stroke) {
    for dy in [-7.0, 0.0, 7.0] {
        painter.circle_filled(Pos2::new(c.x - 8.0, c.y + dy), 1.6, stroke.color);
        painter.line_segment([Pos2::new(c.x - 4.0, c.y + dy), Pos2::new(c.x + 10.0, c.y + dy)], stroke);
    }
}

/// One file in the search results: its name, then (dimmer) the folder it is in, and at the right
/// end `count` (the number of matches) when given.
fn result_row(ui: &mut Ui, name: &str, dir: &str, count: Option<&str>, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(120.0), 22.0), Sense::click());
    let style = ui.ctx().global_style();
    let v = &style.visuals;
    if selected {
        ui.painter().rect_filled(rect, 0.0, v.selection.bg_fill.gamma_multiply(0.6));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let font = egui::TextStyle::Body.resolve(ui.style());
    let cy = rect.center().y;
    let name_rect = ui.painter().text(Pos2::new(rect.left() + 8.0, cy), egui::Align2::LEFT_CENTER, name, font.clone(), v.text_color());
    if !dir.is_empty() {
        let small = egui::FontId::proportional(font.size - 1.5);
        ui.painter().text(Pos2::new(name_rect.right() + 8.0, cy), egui::Align2::LEFT_CENTER, dir, small, v.weak_text_color());
    }
    if let Some(count) = count {
        let small = egui::FontId::proportional(font.size - 2.0);
        let galley = ui.painter().layout_no_wrap(count.to_owned(), small, v.strong_text_color());
        let badge = Rect::from_min_size(
            Pos2::new(rect.right() - 8.0 - galley.size().x - 10.0, cy - galley.size().y / 2.0 - 1.0),
            vec2(galley.size().x + 10.0, galley.size().y + 2.0),
        );
        ui.painter().rect_filled(badge, badge.height() / 2.0, v.widgets.inactive.bg_fill);
        ui.painter().galley(badge.min + vec2(5.0, 1.0), galley, v.strong_text_color());
    }
    resp
}

/// One row of the explorer tree: indented by `depth`, a chevron for folders (`expanded` is
/// `None` for files), the name; highlighted when `selected`. `strong` for the root row.
fn tree_row(ui: &mut Ui, depth: usize, name: &str, expanded: Option<bool>, selected: bool, strong: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(120.0), 22.0), Sense::click());
    let style = ui.ctx().global_style();
    let v = &style.visuals;
    if selected {
        ui.painter().rect_filled(rect, 0.0, v.selection.bg_fill.gamma_multiply(0.6));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let x = rect.left() + 6.0 + depth as f32 * 12.0;
    let cy = rect.center().y;
    let ink = if strong { v.strong_text_color() } else { v.text_color() };
    if let Some(open) = expanded {
        let pts = if open {
            vec![Pos2::new(x, cy - 2.0), Pos2::new(x + 4.0, cy + 2.0), Pos2::new(x + 8.0, cy - 2.0)]
        } else {
            vec![Pos2::new(x + 2.0, cy - 4.0), Pos2::new(x + 6.0, cy), Pos2::new(x + 2.0, cy + 4.0)]
        };
        ui.painter().add(egui::Shape::line(pts, Stroke::new(1.3, ink)));
    }
    let font = if strong { egui::FontId::proportional(12.5) } else { egui::TextStyle::Body.resolve(ui.style()) };
    ui.painter().text(Pos2::new(x + 14.0, cy), egui::Align2::LEFT_CENTER, name, font, ink);
    resp
}

/// The diagrams icon: a small flowchart (a box, a decision diamond, two circles and the boxes
/// below them), about 30 x 28 points around `c`.
fn flowchart_icon(painter: &egui::Painter, c: Pos2, stroke: Stroke) {
    let p = |x: f32, y: f32| Pos2::new(c.x + x, c.y + y);
    let line = |a: Pos2, b: Pos2| {
        painter.line_segment([a, b], stroke);
    };
    let none = Color32::TRANSPARENT;
    // Top box -> diamond -> bottom box.
    painter.rect_stroke(Rect::from_center_size(p(0.0, -11.0), vec2(11.0, 5.0)), 1.5, stroke, egui::StrokeKind::Middle);
    line(p(0.0, -8.5), p(0.0, -5.0));
    painter.add(egui::Shape::convex_polygon(vec![p(0.0, -5.0), p(5.0, 0.0), p(0.0, 5.0), p(-5.0, 0.0)], none, stroke));
    line(p(0.0, 5.0), p(0.0, 8.5));
    painter.rect_stroke(Rect::from_center_size(p(0.0, 11.0), vec2(11.0, 5.0)), 1.5, stroke, egui::StrokeKind::Middle);
    // Diamond -> circles left and right -> small boxes below them.
    for side in [-1.0, 1.0] {
        line(p(5.0 * side, 0.0), p(8.0 * side, 0.0));
        painter.circle_stroke(p(11.0 * side, 0.0), 3.0, stroke);
        line(p(11.0 * side, 3.0), p(11.0 * side, 7.5));
        painter.rect_stroke(Rect::from_center_size(p(11.0 * side, 10.5), vec2(5.0, 5.0)), 0.5, stroke, egui::StrokeKind::Middle);
    }
}

/// What the side panel shows, one icon each in the activity bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideView {
    /// The open folder as a tree.
    Explorer,
    /// Finding files in the open folder by name.
    Search,
    /// The results of Find All References (only while there are some).
    References,
    /// The diagram palette.
    Diagram,
}

/// The categories of the diagram palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteView {
    Objects,
    Collections,
    Pointers,
    Relations,
}

impl PaletteView {
    pub const ALL: [PaletteView; 4] =
        [PaletteView::Objects, PaletteView::Collections, PaletteView::Pointers, PaletteView::Relations];

    pub fn title(self) -> &'static str {
        match self {
            PaletteView::Objects => "Objects",
            PaletteView::Collections => "Collections",
            PaletteView::Pointers => "Pointers",
            PaletteView::Relations => "Relations",
        }
    }
}

pub struct App {
    pub diagram: Diagram,
    pub selection: BTreeSet<Id>,
    pub pan: Vec2,
    pub zoom: f32,
    pub canvas_rect: Rect,
    pub drag: Drag,
    pub editing: Option<Id>,
    pub edit_focus: bool,
    pub palette_drag: Option<ShapeKind>,
    pub show_grid: bool,
    pub snap: bool,
    pub snap_active: bool,

    pub undo: Vec<Diagram>,
    pub redo: Vec<Diagram>,
    /// Last committed state; differences from it become one undo step.
    pub stable: Diagram,
    /// The active diagram as last saved.
    pub saved: Diagram,
    /// The project file.
    pub path: Option<PathBuf>,
    /// The project's diagrams, one tab each. The active tab's state is in the fields above.
    pub tabs: Vec<Tab>,
    /// The tab of the diagram held in the fields above; `None` when the project has no diagram.
    pub active: Option<usize>,
    /// Tab names as last saved (adding, removing, renaming or reordering tabs is a change).
    pub saved_names: Vec<String>,
    /// The tab being renamed, and the name typed so far.
    pub renaming: Option<(usize, String)>,
    /// Tabs waiting to be closed, in order (one question at a time for unsaved ones).
    pub close_queue: Vec<u64>,
    /// The unsaved tab being asked about: its id, and the "apply to other unsaved tabs" box.
    pub close_ask: Option<(u64, bool)>,
    /// The answer given with "apply to other unsaved tabs" ticked, for the rest of the queue:
    /// `Some(true)` closes them without asking, `Some(false)` keeps them.
    pub close_answer_all: Option<bool>,
    /// Put the keyboard focus on "No" when the question appears.
    pub close_focus_no: bool,
    /// The Preferences window is open (the main window is blocked meanwhile).
    pub prefs_open: bool,
    pub rename_focus: bool,
    /// The last place selected in a text file's editor: which file, which lines and columns, and
    /// the text. Kept after the editor loses focus, so the selection can be referred to later.
    pub code_selection: Option<crate::code::CodeSelection>,
    /// Where each tab's button was drawn this frame (the rename field is drawn over it).
    pub tab_rects: std::collections::HashMap<u64, egui::Rect>,
    /// Where the tabs are: groups, splits and floating windows.
    pub dock: crate::model::Layout,
    pub next_item_id: u64,
    /// Diagram tabs drawn this frame and the last one, with their screen areas (for dropping a
    /// palette shape onto the diagram under the mouse).
    pub visible_diagrams: Vec<(u64, Rect)>,
    pub last_visible_diagrams: Vec<(u64, Rect)>,
    title: String,
    clipboard: Option<Clip>,
    paste_count: u32,
    pub status: String,
    allow_close: bool,
    /// Fit the view once the canvas size is known (after opening a file).
    pub fit_pending: bool,
    /// The Validate window is open (it re-checks the diagram every frame).
    pub validate_open: bool,
    /// Active pointer/relation tool: the next drag (or click) on the canvas draws this kind of line.
    pub link_tool: Option<LinkKind>,
    /// Incorrect line shown with an error window until the user clicks OK.
    pub link_error: Option<LinkError>,
    /// New pointers point to a const object.
    pub pointer_const: bool,
    /// What the side panel shows (explorer or diagram palette); `None` = panel hidden.
    pub side: Option<SideView>,
    /// The view to show again when the panel is reopened with Ctrl+B.
    pub last_side: SideView,
    /// The palette category shown in the diagram palette.
    pub palette_view: PaletteView,
    /// The folder open in the explorer.
    pub explorer: Option<crate::explorer::Explorer>,
    /// The search panel: finding files by name in the explorer's folder.
    pub search: crate::search::FileSearch,
    /// Put the keyboard focus on the search field when it is next drawn.
    pub search_focus: bool,
    /// The egui context, for terminals (their reader threads ask for repaints).
    pub egui_ctx: egui::Context,
    /// A text file (in the active diagram) whose contents should be opened in a tab.
    pub open_file_request: Option<Id>,
    /// An object's name as it was when its name field got the focus (restored if left empty).
    pub name_before_edit: String,
    /// The colour theme (Graphite by default); chosen in the View menu and Preferences.
    pub theme: crate::render::Theme,
    /// clangd, for code navigation in C/C++ files (started when first needed).
    pub lsp: Option<crate::lsp::Lsp>,
    /// A navigation asked for in an editor this frame: tab, what, at which character.
    pub nav_request: Option<(u64, crate::lsp::NavKind, usize)>,
    /// The file, symbol and mouse position of the last navigation asked for.
    nav_origin: Option<PathBuf>,
    nav_symbol: String,
    nav_pos: Option<Pos2>,
    /// Several places found by Go to ...: listed in a pop-up.
    nav_popup: Option<NavList>,
    /// The results of Find All References, in the References panel.
    references: Option<NavList>,
    /// How many lines one notch of the mouse wheel scrolls in a text editor (Preferences).
    pub wheel_lines: u32,
    /// The theme last applied to egui, so it is only changed when `theme` changes.
    applied_theme: Option<crate::render::Theme>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            diagram: Diagram::default(),
            selection: BTreeSet::new(),
            pan: vec2(40.0, 40.0),
            zoom: 1.0,
            canvas_rect: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
            drag: Drag::None,
            editing: None,
            edit_focus: false,
            palette_drag: None,
            show_grid: true,
            snap: true,
            snap_active: true,
            undo: Vec::new(),
            redo: Vec::new(),
            stable: Diagram::default(),
            saved: Diagram::default(),
            path: None,
            tabs: vec![Tab { id: 1, name: "Diagram 1".into(), content: Content::Diagram(None) }],
            active: Some(0),
            saved_names: vec!["Diagram 1".into()],
            renaming: None,
            close_queue: Vec::new(),
            close_ask: None,
            close_answer_all: None,
            close_focus_no: false,
            prefs_open: false,
            rename_focus: false,
            code_selection: None,
            tab_rects: Default::default(),
            dock: crate::model::Layout::new(vec![1]),
            next_item_id: 2,
            visible_diagrams: Vec::new(),
            last_visible_diagrams: Vec::new(),
            title: String::new(),
            clipboard: None,
            paste_count: 0,
            status: String::new(),
            allow_close: false,
            fit_pending: false,
            validate_open: false,
            link_tool: None,
            link_error: None,
            pointer_const: false,
            side: Some(SideView::Diagram),
            last_side: SideView::Diagram,
            palette_view: PaletteView::Objects,
            explorer: None,
            search: Default::default(),
            search_focus: false,
            egui_ctx: egui::Context::default(),
            open_file_request: None,
            name_before_edit: String::new(),
            theme: crate::render::Theme::default(),
            wheel_lines: 5,
            lsp: None,
            nav_request: None,
            nav_origin: None,
            nav_symbol: String::new(),
            nav_pos: None,
            nav_popup: None,
            references: None,
            applied_theme: None,
        }
    }
}

impl eframe::App for App {
    /// The mouse wheel over a text editor scrolls `wheel_lines` lines per notch (egui's own speed
    /// is about two lines). Touchpads are left as they are.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let rects = crate::code::editor_rects(ctx);
        if rects.is_empty() {
            return;
        }
        let per_line = ctx.options(|o| o.input_options.line_scroll_speed);
        let mut pos = ctx.input(|i| i.pointer.latest_pos());
        for ev in &mut raw_input.events {
            match ev {
                Event::PointerMoved(p) => pos = Some(*p),
                Event::MouseWheel { unit, delta, source, modifiers, .. } if !modifiers.command => {
                    let wheel = match unit {
                        egui::MouseWheelUnit::Line => true,
                        egui::MouseWheelUnit::Point => *source == egui::MouseWheelSource::Wheel,
                        egui::MouseWheelUnit::Page => false,
                    };
                    let over = pos.and_then(|p| rects.iter().find(|(r, _)| r.contains(p)));
                    if let (true, Some(&(_, row))) = (wheel, over) {
                        *delta *= self.wheel_lines as f32 * row / per_line;
                    }
                }
                _ => {}
            }
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::code::clear_editor_rects(&ctx);
        self.egui_ctx = ctx.clone();
        if self.applied_theme != Some(self.theme) {
            let egui_theme = if self.theme.is_dark() { egui::Theme::Dark } else { egui::Theme::Light };
            ctx.set_theme(egui_theme);
            ctx.set_visuals_of(egui_theme, theme_visuals(self.theme));
            crate::render::set_theme(self.theme);
            self.applied_theme = Some(self.theme);
        }
        self.handle_close(&ctx);
        // While Preferences is open the main window takes no shortcuts.
        if !self.prefs_open {
            self.handle_shortcuts(&ctx);
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        // VS Code-style: a strip of category icons, and the palette of the chosen category beside it.
        // No inner margin, so the highlight bar of the shown category sits on the window's edge.
        let strip = egui::Frame::side_top_panel(ui.style()).inner_margin(0.0);
        if self.references.is_none() {
            if self.side == Some(SideView::References) {
                self.side = Some(SideView::Explorer);
            }
            if self.last_side == SideView::References {
                self.last_side = SideView::Explorer;
            }
        }
        // Search needs an open folder: when it is closed, the explorer shows instead.
        if self.explorer.is_none() {
            if self.side == Some(SideView::Search) {
                self.side = Some(SideView::Explorer);
            }
            if self.last_side == SideView::Search {
                self.last_side = SideView::Explorer;
            }
        }
        egui::Panel::left("activity").resizable(false).exact_size(44.0).frame(strip).show(ui, |ui| self.activity_bar(ui));
        match self.side {
            Some(SideView::Explorer) => {
                egui::Panel::left("explorer").resizable(true).default_size(220.0).show(ui, |ui| self.explorer_panel(ui));
            }
            Some(SideView::References) => {
                egui::Panel::left("references").resizable(true).default_size(300.0).show(ui, |ui| self.references_panel(ui));
            }
            Some(SideView::Search) => {
                egui::Panel::left("search").resizable(true).default_size(260.0).show(ui, |ui| self.search_panel(ui));
            }
            Some(SideView::Diagram) => {
                let view = self.palette_view;
                egui::Panel::left("palette").resizable(true).default_size(130.0).show(ui, |ui| self.palette(ui, view));
            }
            None => {}
        }
        // The format panel is there only while something is selected: it edits the selection. It
        // is hidden while a tab that is not a diagram (a file's editor, an image) has focus.
        let has_selection = self
            .selection
            .iter()
            .any(|id| self.diagram.node(*id).is_some() || self.diagram.edge(*id).is_some());
        if has_selection && self.diagram_focused() {
            egui::Panel::right("format").resizable(true).default_size(320.0).size_range(220.0..=560.0).show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.format_panel(ui));
            });
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.workspace(ui));
        self.start_nav();
        self.poll_nav();
        self.nav_popup_window(&ctx);
        self.link_error_window(&ctx);
        self.validate_window(&ctx);
        if self.prefs_open {
            self.preferences_window(&ctx);
        }

        self.commit_undo(&ctx);
        self.update_title(&ctx);
    }
}

impl App {
    /// The Preferences window: a separate OS window. The main window is dimmed and takes no
    /// input until it is closed; clicking the main window brings Preferences back to the front.
    fn preferences_window(&mut self, ctx: &egui::Context) {
        let prefs = egui::ViewportId::from_hash_of("preferences");
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::unique("prefs_blocker"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let r = ui.allocate_rect(screen, Sense::click_and_drag());
                ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(120));
                if r.clicked() || r.drag_started() {
                    ctx.send_viewport_cmd_to(prefs, ViewportCommand::Focus);
                }
            });

        // Keep it in front: whenever the main window gets the focus, give it back.
        if ctx.input(|i| i.viewport().focused) == Some(true) {
            ctx.send_viewport_cmd_to(prefs, ViewportCommand::Focus);
        }
        let size = vec2(360.0, 340.0);
        let mut builder = egui::ViewportBuilder::default()
            .with_title("Preferences")
            .with_inner_size(size)
            .with_taskbar(false)
            .with_resizable(false)
            .with_minimize_button(false)
            .with_maximize_button(false);
        // Centred over the main window.
        if let Some(main) = ctx.input(|i| i.viewport().outer_rect) {
            builder = builder.with_position(main.center() - size / 2.0);
        }
        let mut close = false;
        ctx.show_viewport_immediate(prefs, builder, |ui, _class| {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.heading("Preferences");
                ui.add_space(8.0);
                ui.strong("Theme");
                ui.add_space(4.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                for t in crate::render::Theme::ALL {
                    if rect_button(ui, t.name(), self.theme == t).clicked() {
                        self.theme = t;
                    }
                }
                ui.add_space(12.0);
                ui.strong("Text editor");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Mouse wheel scrolls");
                    ui.add(egui::DragValue::new(&mut self.wheel_lines).range(1..=30));
                    ui.label("lines per notch");
                });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                    ui.allocate_ui(vec2(100.0, 26.0), |ui| {
                        if rect_button(ui, "Close", false).clicked() {
                            close = true;
                        }
                    });
                });
            });
            if ui.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
        if close {
            self.prefs_open = false;
            // The main window is active again.
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, ViewportCommand::Focus);
        }
    }

    // ---------------------------------------------------------------- history

    fn commit_undo(&mut self, ctx: &egui::Context) {
        // An incorrect line on screen is temporary; it must not become an undo step.
        if self.diagram == self.stable || self.link_error.is_some() {
            return;
        }
        // Wait until a drag or text edit is finished so it becomes a single undo step.
        if ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input() {
            return;
        }
        self.force_commit();
    }

    pub fn force_commit(&mut self) {
        if self.diagram == self.stable {
            return;
        }
        let prev = std::mem::replace(&mut self.stable, self.diagram.clone());
        self.undo.push(prev);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        self.force_commit();
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.diagram, prev));
            self.after_history_change();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.diagram, next));
            self.after_history_change();
        }
    }

    fn after_history_change(&mut self) {
        self.stable = self.diagram.clone();
        let d = &self.diagram;
        self.selection.retain(|id| d.node(*id).is_some() || d.edge(*id).is_some());
        self.editing = None;
        self.drag = Drag::None;
    }

    // ---------------------------------------------------------------- files

    fn dirty(&self) -> bool {
        self.project_dirty()
    }

    fn confirm_discard(&self) -> bool {
        !self.dirty()
            || rfd::MessageDialog::new()
                .set_title("Unsaved changes")
                .set_description("The project has unsaved changes. Discard them?")
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                == rfd::MessageDialogResult::Yes
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.confirm_discard() {
                self.allow_close = true;
            } else {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            }
        }
    }

    /// The app as started without a project: nothing open, the palette hidden.
    pub fn start_empty(&mut self) {
        self.reset_project(Project { items: Vec::new(), active: None, layout: None }, None);
        self.side = None;
    }

    fn new_file(&mut self) {
        if self.confirm_discard() {
            self.reset_project(Project { items: Vec::new(), active: None, layout: None }, None);
            self.fit_pending = false;
            self.status = "New project".into();
        }
    }

    fn open(&mut self) {
        if !self.confirm_discard() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new().add_filter("Project or diagram", &["json"]).pick_file() {
            self.open_path(path);
        }
    }

    pub fn open_path(&mut self, path: PathBuf) {
        // A single-diagram file (from before projects) opens as a one-tab project named after it.
        let fallback = path.file_stem().map_or_else(|| "Diagram 1".into(), |s| s.to_string_lossy().into_owned());
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| crate::model::load_project(&s, &fallback))
        {
            Ok(project) => {
                // The folder the project had open in the explorer.
                let folder = std::fs::read_to_string(&path).ok().and_then(|t| crate::model::project_folder(&t));
                self.explorer = folder.map(|f| crate::explorer::Explorer::new(f.into()));
                let count = project.items.len();
                self.status = format!("Opened {} ({count} tab{})", path.display(), if count == 1 { "" } else { "s" });
                self.reset_project(project, Some(path));
            }
            Err(e) => self.status = format!("Could not open {}: {e}", path.display()),
        }
    }

    /// Ctrl+S: changed files on disk are written, and the project saved. A project that was
    /// never saved and holds only files from disk is not asked for.
    /// Ctrl+S: writes the changed files on disk, then the project. A project without a file is
    /// only asked a file name for when it has unsaved work of its own (a diagram, a project text
    /// file or a picture), not for files on disk or terminals.
    fn save(&mut self) {
        let written = self.save_disk_files();
        let unsaved_project_work = self.tabs.iter().any(|t| {
            matches!(
                t.content,
                crate::tabs::Content::Diagram(_) | crate::tabs::Content::Text { .. } | crate::tabs::Content::Image { .. }
            ) && self.tab_dirty(t)
        });
        match self.path.clone() {
            Some(p) => self.write_to(p),
            None if unsaved_project_work => self.save_as(),
            None => {
                self.mark_project_saved();
                let s = if written == 1 { "" } else { "s" };
                self.status = format!("Saved {written} file{s}");
            }
        }
    }

    /// Asks for a folder and shows it in the explorer.
    fn open_folder(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.status = format!("Opened folder {}", dir.display());
            self.explorer = Some(crate::explorer::Explorer::new(dir));
            self.side = Some(SideView::Explorer);
        }
    }

    fn save_as(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Project", &["json"])
            .set_file_name("project.json")
            .save_file()
        {
            self.write_to(p);
        }
    }

    fn write_to(&mut self, path: PathBuf) {
        self.force_commit();
        self.save_disk_files();
        let project = ProjectFile {
            alien_artefact_project: 2,
            diagrams: Vec::new(),
            items: self.project_items(),
            active: self.active_id().unwrap_or(0),
            // Without the terminals, which are not saved.
            layout: crate::model::layout_to_text(&self.dock.filter_tabs(|id| {
                !self.tabs.iter().any(|t| t.id == *id && matches!(t.content, crate::tabs::Content::Terminal(_)))
            })),
            folder: self.explorer.as_ref().map(|e| e.root.display().to_string()),
        };
        let json = serde_json::to_string_pretty(&project).expect("project serializes");
        match std::fs::write(&path, json) {
            Ok(()) => {
                self.mark_project_saved();
                self.status = format!("Saved {}", path.display());
                self.path = Some(path);
            }
            Err(e) => self.status = format!("Could not save {}: {e}", path.display()),
        }
    }

    fn export_svg(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("SVG image", &["svg"])
            .set_file_name(format!("{}.svg", self.active_tab_name().unwrap_or("diagram")))
            .save_file()
        else {
            return;
        };
        self.status = match std::fs::write(&path, crate::svg::export(&self.diagram)) {
            Ok(()) => format!("Exported {}", path.display()),
            Err(e) => format!("Could not export {}: {e}", path.display()),
        };
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        let dirty = if self.dirty() { "*" } else { "" };
        let title = match self.active_tab_name() {
            Some(tab) => format!("{name}{dirty} \u{2014} {tab} - Alien Artefact"),
            None => format!("{name}{dirty} - Alien Artefact"),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    // ---------------------------------------------------------------- validation

    /// Lists every broken rule; clicking one selects what it is about and scrolls to it.
    fn validate_window(&mut self, ctx: &egui::Context) {
        if !self.validate_open {
            return;
        }
        let issues = crate::validate::validate(&self.diagram);
        let mut open = true;
        let mut pick: Option<Vec<Id>> = None;
        egui::Window::new(format!("Problems ({})", issues.len()))
            .id(egui::Id::unique("validate_window"))
            .open(&mut open)
            .default_width(460.0)
            .default_height(360.0)
            .show(ctx, |ui| {
                if issues.is_empty() {
                    ui.label("No problems found. The diagram follows every rule the editor can check.");
                    return;
                }
                ui.weak("Rule ids refer to docs/language-spec.md. Click a problem to show it.");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, issue) in issues.iter().enumerate() {
                        ui.push_id(i, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.monospace(format!("{:<4}", issue.rule));
                                if ui.link(&issue.message).clicked() {
                                    pick = Some(issue.items.clone());
                                }
                            });
                        });
                    }
                });
            });
        self.validate_open = open;
        if let Some(items) = pick {
            self.selection = items.iter().copied().filter(|id| self.diagram.node(*id).is_some() || self.diagram.edge(*id).is_some()).collect();
            // Scroll so the first item is in the middle of the canvas.
            let first = items.iter().find_map(|id| {
                self.diagram.node(*id).map(|n| n.rect().center()).or_else(|| {
                    self.diagram.edge(*id).and_then(|e| self.diagram.edge_path(e)).map(|p| crate::model::polyline_midpoint(&p))
                })
            });
            if let Some(c) = first {
                self.pan = self.canvas_rect.size() / 2.0 - c.to_vec2() * self.zoom;
            }
        }
    }

    // ---------------------------------------------------------------- incorrect relations

    /// Modal error window for an incorrect relation; OK (or Enter/Esc) undoes the relation.
    fn link_error_window(&mut self, ctx: &egui::Context) {
        let Some(err) = &self.link_error else { return };
        let mut ok = false;
        // Near the top, so the incorrect (red) line on the canvas stays visible.
        let id = egui::Id::unique("link_error");
        let area = egui::Modal::default_area(id).anchor(egui::Align2::CENTER_TOP, vec2(0.0, 120.0));
        egui::Modal::new(id).area(area).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading("Incorrect relation");
            ui.add_space(8.0);
            ui.label(&err.message);
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ok = ui.add(egui::Button::new("   OK   ")).clicked();
            });
        });
        if ok || ctx.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Escape)) {
            self.dismiss_link_error();
        }
    }

    fn dismiss_link_error(&mut self) {
        let Some(err) = self.link_error.take() else { return };
        match err.restore {
            Some((end, orig)) => {
                if let Some(e) = self.diagram.edge_mut(err.id) {
                    if end == 0 { e.from = orig } else { e.to = orig }
                }
            }
            None => {
                self.diagram.edges.retain(|e| e.id != err.id);
                self.selection.remove(&err.id);
            }
        }
    }

    // ---------------------------------------------------------------- edit actions

    pub fn delete_selected(&mut self) {
        self.diagram.remove(&self.selection);
        self.selection.clear();
        self.editing = None;
    }

    pub fn duplicate(&mut self) {
        let clip = self.diagram.copy_subset(&self.selection);
        if !clip.nodes.is_empty() {
            self.selection = self.diagram.paste(&clip, vec2(20.0, 20.0));
        }
    }

    pub fn copy(&mut self, ctx: &egui::Context) {
        let clip = self.diagram.copy_subset(&self.selection);
        if clip.nodes.is_empty() {
            return;
        }
        if let Ok(json) = serde_json::to_string(&clip) {
            ctx.copy_text(json);
        }
        self.clipboard = Some(clip);
        self.paste_count = 0;
    }

    fn paste_clip(&mut self, clip: Clip) {
        self.paste_count += 1;
        let off = 20.0 * self.paste_count as f32;
        self.selection = self.diagram.paste(&clip, vec2(off, off));
    }

    pub fn paste_internal(&mut self) {
        if let Some(clip) = self.clipboard.clone() {
            self.paste_clip(clip);
        }
    }

    /// Pastes objects and pointers copied from this app; other clipboard text is ignored.
    fn paste_text(&mut self, text: &str) {
        if let Ok(clip) = serde_json::from_str::<Clip>(text) {
            self.paste_clip(clip);
        }
    }

    fn nudge(&mut self, d: Vec2) {
        for id in &self.selection {
            if let Some(n) = self.diagram.node_mut(*id) {
                n.pos[0] += d.x;
                n.pos[1] += d.y;
            }
        }
    }

    fn zoom_by(&mut self, factor: f32) {
        self.zoom_at(self.canvas_rect.center(), factor);
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Search for files (with a folder open): works also while typing in an editor.
        if self.link_error.is_none()
            && self.explorer.is_some()
            && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::F)) {
            self.side = Some(SideView::Search);
            self.search_focus = true;
        }
        // Save: works also while typing in a text file.
        if self.link_error.is_none() {
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S)) {
                self.save_as();
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
                self.save();
            }
        }
        // Find in the focused text file: works also while typing in it.
        if self.link_error.is_none() && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F)) {
            let tab = self.focused_item().and_then(|id| self.index_of(id)).map(|i| &self.tabs[i]);
            match tab.map(|t| (&t.content, t.id)) {
                Some((crate::tabs::Content::DiskText { .. }, id)) => crate::code::open_find(ctx, ("disk", id)),
                Some((crate::tabs::Content::Text { .. }, id)) => crate::code::open_find(ctx, ("text", id)),
                Some((crate::tabs::Content::FileText { .. }, id)) => crate::code::open_find(ctx, ("file", id)),
                _ => {}
            }
        }
        if ctx.egui_wants_keyboard_input() || self.link_error.is_some() {
            return;
        }
        let key = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;

        if key(cmd_shift, Key::Z) || key(cmd, Key::Y) {
            self.redo();
        }
        if key(cmd, Key::Z) {
            self.undo();
        }
        if key(cmd, Key::O) {
            self.open();
        }
        if key(cmd, Key::N) {
            self.new_file();
        }
        if key(cmd, Key::B) {
            self.toggle_palette();
        }
        if key(cmd, Key::T) {
            self.new_tab();
        }
        if key(cmd, Key::W) {
            if let Some(id) = self.focused_item() {
                self.close_item(id);
            }
        }
        if key(cmd, Key::A) {
            self.select_all();
        }
        if key(cmd, Key::D) {
            self.duplicate();
        }
        if key(Modifiers::NONE, Key::Delete) || key(Modifiers::NONE, Key::Backspace) {
            self.delete_selected();
        }
        if key(Modifiers::NONE, Key::F2) {
            if let Some(&id) = self.selection.iter().next() {
                self.start_editing(id);
            }
        }
        if key(Modifiers::NONE, Key::Escape) {
            if self.link_tool.take().is_none() {
                self.selection.clear();
            }
        }
        for (k, d) in [
            (Key::ArrowLeft, vec2(-1.0, 0.0)),
            (Key::ArrowRight, vec2(1.0, 0.0)),
            (Key::ArrowUp, vec2(0.0, -1.0)),
            (Key::ArrowDown, vec2(0.0, 1.0)),
        ] {
            if key(Modifiers::SHIFT, k) {
                self.nudge(d * crate::canvas::GRID);
            } else if key(Modifiers::NONE, k) {
                self.nudge(d);
            }
        }

        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            match ev {
                Event::Copy => self.copy(ctx),
                Event::Cut => {
                    self.copy(ctx);
                    self.delete_selected();
                }
                Event::Paste(text) => self.paste_text(&text),
                _ => {}
            }
        }
    }

    // ---------------------------------------------------------------- panels

    fn menu_bar(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.add(egui::Button::new("New project").shortcut_text("Ctrl+N")).clicked() {
                    self.new_file();
                }
                if ui.add(egui::Button::new("New diagram").shortcut_text("Ctrl+T")).clicked() {
                    self.new_tab();
                }
                if ui.button("New text file").clicked() {
                    self.add_tab(NewKind::Text, None);
                }
                if ui.button("New terminal").clicked() {
                    self.add_tab(NewKind::Terminal, None);
                }
                if ui.button("Open image...").clicked() {
                    self.open_image();
                }
                if ui.add(egui::Button::new("Close tab").shortcut_text("Ctrl+W")).clicked() {
                    if let Some(id) = self.focused_item() {
                        self.close_item(id);
                    }
                }
                ui.separator();
                if ui.add(egui::Button::new("Open...").shortcut_text("Ctrl+O")).clicked() {
                    self.open();
                }
                if ui.button("Open Folder...").clicked() {
                    self.open_folder();
                }
                if ui.add_enabled(self.explorer.is_some(), egui::Button::new("Close Folder")).clicked() {
                    self.explorer = None;
                }
                if ui.add(egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                    self.save();
                }
                if ui.add(egui::Button::new("Save as...").shortcut_text("Ctrl+Shift+S")).clicked() {
                    self.save_as();
                }
                ui.separator();
                if ui.button("Export as SVG...").clicked() {
                    self.export_svg();
                }
                ui.separator();
                if ui.button("Preferences...").clicked() {
                    self.prefs_open = true;
                    // Nothing in the main window keeps the keyboard meanwhile.
                    ui.ctx().memory_mut(|m| m.stop_text_input());
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.add_enabled(!self.undo.is_empty() || self.diagram != self.stable, egui::Button::new("Undo").shortcut_text("Ctrl+Z")).clicked() {
                    self.undo();
                }
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Redo").shortcut_text("Ctrl+Y")).clicked() {
                    self.redo();
                }
                ui.separator();
                let has_sel = !self.selection.is_empty();
                if ui.add_enabled(has_sel, egui::Button::new("Cut").shortcut_text("Ctrl+X")).clicked() {
                    self.copy(ui.ctx());
                    self.delete_selected();
                }
                if ui.add_enabled(has_sel, egui::Button::new("Copy").shortcut_text("Ctrl+C")).clicked() {
                    self.copy(ui.ctx());
                }
                if ui.add_enabled(self.clipboard.is_some(), egui::Button::new("Paste").shortcut_text("Ctrl+V")).clicked() {
                    self.paste_internal();
                }
                if ui.add_enabled(has_sel, egui::Button::new("Duplicate").shortcut_text("Ctrl+D")).clicked() {
                    self.duplicate();
                }
                if ui.add_enabled(has_sel, egui::Button::new("Delete").shortcut_text("Del")).clicked() {
                    self.delete_selected();
                }
                ui.separator();
                if ui.add(egui::Button::new("Select all").shortcut_text("Ctrl+A")).clicked() {
                    self.select_all();
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Zoom in").clicked() {
                    self.zoom_by(1.25);
                }
                if ui.button("Zoom out").clicked() {
                    self.zoom_by(0.8);
                }
                if ui.button("Actual size").clicked() {
                    self.zoom_by(1.0 / self.zoom);
                }
                if ui.button("Fit").clicked() {
                    self.fit_view();
                }
                ui.separator();
                ui.checkbox(&mut self.show_grid, "Grid");
                ui.checkbox(&mut self.snap, "Snap to grid");
                ui.separator();
                ui.menu_button("Theme", |ui| {
                    for t in crate::render::Theme::ALL {
                        ui.radio_value(&mut self.theme, t, t.name());
                    }
                });
            });
        });
    }

    /// The strip at the top of a diagram tab (like a file's): its name, zoom and grid controls.
    /// Returns true when one of them was used.
    pub fn diagram_strip(&mut self, ui: &mut Ui, name: &str) -> bool {
        let mut used = false;
        ui.horizontal(|ui| {
            ui.strong(name);
            ui.separator();
            if ui.button(" - ").on_hover_text("Zoom out").clicked() {
                self.zoom_by(0.8);
                used = true;
            }
            if ui.button(format!("{:.0}%", self.zoom * 100.0)).on_hover_text("Reset zoom").clicked() {
                self.zoom_by(1.0 / self.zoom);
                used = true;
            }
            if ui.button(" + ").on_hover_text("Zoom in").clicked() {
                self.zoom_by(1.25);
                used = true;
            }
            if ui.button("Fit").on_hover_text("Fit the diagram in view").clicked() {
                self.fit_view();
                used = true;
            }
            ui.separator();
            used |= ui.checkbox(&mut self.show_grid, "Grid").changed();
            used |= ui.checkbox(&mut self.snap, "Snap").changed();
            ui.separator();
            // The diagram is checked against the language rules all the time; this shows the
            // result and opens the list of problems.
            let problems = crate::validate::validate(&self.diagram).len();
            let (text, color) = if problems == 0 {
                ("\u{2714} No problems".to_owned(), Color32::from_rgb(90, 200, 120))
            } else {
                let s = if problems == 1 { "" } else { "s" };
                (format!("\u{26a0} {problems} problem{s}"), crate::canvas::WARN)
            };
            let hint = if problems == 0 { "The diagram follows every rule the editor checks" } else { "Show the problems" };
            let r = ui.add(egui::Button::new(egui::RichText::new(text).color(color)).frame(false)).on_hover_text(hint);
            if r.clicked() {
                self.validate_open = !self.validate_open;
                used = true;
            }
        });
        ui.separator();
        used
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let selection = self.focused_file_selection();
        ui.horizontal(|ui| {
            if let Some(sel) = selection {
                // In a file's editor: where the cursor is and how much is selected.
                ui.label(sel.status()).on_hover_text(sel.reference());
                ui.separator();
            }
            let mut first = true;
            if self.active.is_some() {
                ui.label(format!(
                    "{} shapes, {} lines  |  zoom {:.0}%",
                    self.diagram.nodes.len(),
                    self.diagram.edges.len(),
                    self.zoom * 100.0
                ));
                first = false;
            }
            if !self.status.is_empty() {
                if !first {
                    ui.separator();
                }
                ui.label(&self.status);
            }
        });
    }

    /// Shows or hides the side panel (Ctrl+B, or a second click on the active icon).
    pub fn toggle_palette(&mut self) {
        match self.side.take() {
            Some(view) => self.last_side = view,
            None => self.side = Some(self.last_side),
        }
    }

    /// The icon strip (VS Code's activity bar): the explorer and the diagram palette. Clicking an
    /// icon shows its panel; clicking the shown one hides it.
    fn activity_bar(&mut self, ui: &mut Ui) {
        let accent = self.theme.palette().map_or(Color32::from_rgb(0, 120, 215), |p| p.line);
        ui.add_space(6.0);
        // Search is there only while a folder is open.
        for view in [SideView::Explorer, SideView::Search, SideView::References, SideView::Diagram] {
            if view == SideView::Search && self.explorer.is_none() {
                continue;
            }
            // References only while there are results.
            if view == SideView::References && self.references.is_none() {
                continue;
            }
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
            let shown = self.side == Some(view);
            let title = match view {
                SideView::Explorer => "Explorer",
                SideView::Search => "Search (Ctrl+Shift+F)",
                SideView::References => "References",
                SideView::Diagram => "Diagram shapes",
            };
            let resp = resp.on_hover_text(if shown { format!("{title} (click to hide, Ctrl+B)") } else { title.to_owned() });
            let painter = ui.painter();
            if resp.hovered() {
                painter.rect_filled(rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
            }
            if shown {
                painter.rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 0.0, accent);
            }
            let col = if shown || resp.hovered() { ui.visuals().strong_text_color() } else { ui.visuals().weak_text_color() };
            match view {
                SideView::Explorer => files_icon(painter, rect.center(), Stroke::new(1.4, col)),
                SideView::Search => search_icon(painter, rect.center(), Stroke::new(1.6, col)),
                SideView::References => references_icon(painter, rect.center(), Stroke::new(1.6, col)),
                SideView::Diagram => flowchart_icon(painter, rect.center(), Stroke::new(1.3, col)),
            }
            if resp.clicked() {
                if shown {
                    self.toggle_palette();
                } else {
                    self.side = Some(view);
                    self.search_focus = view == SideView::Search;
                }
            }
        }
    }

    /// A panel header: a title, optionally a refresh button right after it (`refresh` is its
    /// tooltip), and a chevron that hides the panel. Returns whether refresh was clicked.
    fn panel_header(&mut self, ui: &mut Ui, title: &str, refresh: Option<&str>) -> bool {
        let mut refresh_clicked = false;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.strong(title);
            if let Some(tip) = refresh {
                let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                let resp = resp.on_hover_text(tip);
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                let col = if resp.hovered() { ui.visuals().strong_text_color() } else { ui.visuals().text_color() };
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "\u{21bb}", egui::FontId::proportional(15.0), col);
                refresh_clicked = resp.clicked();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // A left-pointing chevron, drawn (no icon font needed).
                let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                let resp = resp.on_hover_text("Hide the panel (Ctrl+B)");
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                let c = rect.center();
                let col = if resp.hovered() { ui.visuals().strong_text_color() } else { ui.visuals().text_color() };
                ui.painter().add(egui::Shape::line(
                    vec![Pos2::new(c.x + 3.0, c.y - 5.0), Pos2::new(c.x - 2.0, c.y), Pos2::new(c.x + 3.0, c.y + 5.0)],
                    Stroke::new(1.6, col),
                ));
                if resp.clicked() {
                    self.toggle_palette();
                }
            });
        });
        refresh_clicked
    }

    /// The explorer: the open folder as a tree (folders first, click to expand), or a button to
    /// open one. Clicking a file opens it in a tab.
    fn explorer_panel(&mut self, ui: &mut Ui) {
        let refresh = self.panel_header(ui, "EXPLORER", self.explorer.is_some().then_some("Refresh: read the folder again"));
        ui.add_space(2.0);
        let Some(ex) = self.explorer.as_mut() else {
            ui.add_space(6.0);
            ui.label("You have not opened a folder yet.");
            ui.add_space(8.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            if rect_button(ui, "Open Folder", true).clicked() {
                self.open_folder();
            }
            return;
        };
        let rows = ex.rows();
        let root_name = ex.root_name().to_uppercase();
        let root = ex.root.clone();
        let root_open = ex.expanded.contains(&root);
        // The file of the focused tab is highlighted.
        let focused_path = self.focused_disk_path();
        let mut toggle: Option<PathBuf> = None;
        let mut open: Option<PathBuf> = None;
        ui.horizontal(|ui| {
            if tree_row(ui, 0, &root_name, Some(root_open), false, true).clicked() {
                toggle = Some(root.clone());
            }
        });
        if root_open {
            egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (depth, e) in &rows {
                    let expanded = e.is_dir.then(|| self.explorer.as_ref().is_some_and(|x| x.expanded.contains(&e.path)));
                    let selected = focused_path.as_deref() == Some(e.path.as_path());
                    let r = tree_row(ui, depth + 1, &e.name, expanded, selected, false).on_hover_text(e.path.display().to_string());
                    if r.clicked() {
                        if e.is_dir {
                            toggle = Some(e.path.clone());
                        } else {
                            open = Some(e.path.clone());
                        }
                    }
                }
            });
        }
        if let Some(ex) = self.explorer.as_mut() {
            if let Some(dir) = toggle {
                ex.toggle(&dir);
            }
            if refresh {
                ex.refresh();
                self.search.rerun();
            }
        }
        if let Some(path) = open {
            self.open_disk_file(&path);
        }
    }

    /// The file on disk shown in the focused tab, if it shows one.
    fn focused_disk_path(&mut self) -> Option<PathBuf> {
        self.focused_item().and_then(|id| self.index_of(id)).and_then(|i| match &self.tabs[i].content {
            crate::tabs::Content::DiskText { path, .. } => Some(path.clone()),
            crate::tabs::Content::Image { path, .. } => Some(PathBuf::from(path)),
            _ => None,
        })
    }

    /// The search panel (only while a folder is open): finds files in the explorer's folder by
    /// name, and text in them. Two fields, each with Match Case and Whole Word toggles inside it
    /// (as in VS Code): file name patterns, and the text to find. Without text, the files found
    /// are listed; with text, the lines containing it, under their files. Clicking a file opens
    /// it, clicking a line opens the file there. Pattern rules are in `search.rs`.
    fn search_panel(&mut self, ui: &mut Ui) {
        self.panel_header(ui, "SEARCH", None);
        ui.add_space(2.0);
        // Not shown without a folder (see `ui`).
        let Some(root) = self.explorer.as_ref().map(|e| e.root.clone()) else { return };

        let files = search_field(ui, "search_files", &mut self.search.query, &mut self.search.opts, "Files, e.g. *.cpp, *.h (empty: all)");
        if std::mem::take(&mut self.search_focus) {
            files.request_focus();
        }
        ui.add_space(3.0);
        search_field(ui, "search_text", &mut self.search.text, &mut self.search.text_opts, "Text in the files");

        let ctx = ui.ctx().clone();
        self.search.update(Some(&root), move || ctx.request_repaint());

        ui.add_space(4.0);
        let n = self.search.results.len();
        let text = self.search.searching_text();
        let what = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let found = if text {
            let matches: usize = self.search.results.iter().map(|h| h.match_count()).sum();
            format!("{} in {}", what(matches, "match", "matches"), what(n, "file", "files"))
        } else {
            what(n, "file", "files")
        };
        let asked = text || !self.search.query.trim().is_empty();
        ui.horizontal(|ui| {
            if !asked {
                // Nothing searched for yet: no status line.
            } else if self.search.running() {
                ui.spinner();
                ui.weak(format!("Searching... {found}"));
            } else if n == 0 {
                ui.weak(if text { "No results." } else { "No files found." });
            } else if self.search.truncated {
                ui.weak(format!("{found} (more: refine the search)"));
            } else {
                ui.weak(found);
            }
            if asked {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("\u{21bb}").on_hover_text("Search again").clicked() {
                        self.search.rerun();
                    }
                });
            }
        });
        ui.add_space(2.0);

        let focused_path = self.focused_disk_path();
        let results = std::mem::take(&mut self.search.results);
        let open = hit_list(ui, "search_results", &results, text, focused_path.as_deref());
        self.search.results = results;
        if let Some((path, at)) = open {
            self.open_at(&path, at);
        }
    }

    /// Opens a file on disk in a tab and, with `at` = (line from 1, from, to), selects those
    /// characters of that line.
    fn open_at(&mut self, path: &std::path::Path, at: Option<(usize, usize, usize)>) {
        self.open_disk_file(path);
        if let Some((line, a, b)) = at {
            let tab = self.tabs.iter().find(|t| matches!(&t.content, crate::tabs::Content::DiskText { path: p, .. } if p == path));
            if let Some(id) = tab.map(|t| t.id) {
                crate::code::request_goto(&self.egui_ctx, ("disk", id), line, a, b);
            }
        }
    }

    // ------------------------------------------------------------- code navigation

    /// Carries out the navigation asked for in a C/C++ editor this frame: asks clangd (started
    /// when first needed, for the explorer's folder), or for Switch Header/Source without clangd
    /// looks for the file by name.
    fn start_nav(&mut self) {
        use crate::lsp::NavKind;
        let Some((tab, kind, at)) = self.nav_request.take() else { return };
        let Some(i) = self.index_of(tab) else { return };
        let crate::tabs::Content::DiskText { path, text, .. } = &self.tabs[i].content else { return };
        let (path, text) = (path.clone(), text.clone());
        let before: String = text.chars().take(at).collect();
        let line = before.matches('\n').count();
        let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count());
        let line_text = text.split('\n').nth(line).unwrap_or("");
        self.nav_symbol = word_at(line_text, col);
        self.nav_pos = self.egui_ctx.input(|i| i.pointer.latest_pos());
        self.nav_origin = Some(path.clone());
        let root = self.explorer.as_ref().map_or_else(|| path.parent().unwrap_or(&path).to_path_buf(), |e| e.root.clone());
        if self.lsp.as_mut().is_some_and(|l| l.exited() || l.root != root) {
            self.lsp = None;
        }
        if self.lsp.is_none() {
            let ctx = self.egui_ctx.clone();
            match crate::lsp::Lsp::start(&root, move || ctx.request_repaint()) {
                Ok(l) => self.lsp = Some(l),
                Err(e) => {
                    if kind == NavKind::SwitchHeaderSource {
                        self.switch_by_name();
                    } else {
                        self.status = format!("{}: {e}", kind.title());
                    }
                    return;
                }
            }
        }
        if let Some(lsp) = self.lsp.as_mut() {
            lsp.request(kind, &path, &text, line, crate::lsp::utf16_col(line_text, col));
            self.status = format!("{}: asking clangd...", kind.title());
        }
    }

    /// Switch Header/Source by file name (without clangd, or when it does not know).
    fn switch_by_name(&mut self) {
        let Some(origin) = self.nav_origin.clone() else { return };
        let root = self.explorer.as_ref().map(|e| e.root.clone());
        match crate::lsp::counterpart(&origin, root.as_deref()) {
            Some(other) => self.open_at(&other, None),
            None => {
                let name = origin.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.status = format!("No matching header or source file for {name}");
            }
        }
    }

    /// Takes clangd's answers: one place is opened at once; several are listed (in a pop-up, or
    /// for Find All References in the References panel).
    fn poll_nav(&mut self) {
        use crate::lsp::NavKind;
        let Some(lsp) = self.lsp.as_mut() else { return };
        for r in lsp.poll() {
            let symbol = if self.nav_symbol.is_empty() { String::new() } else { format!(" for '{}'", self.nav_symbol) };
            if r.kind == NavKind::SwitchHeaderSource {
                match r.locations.first() {
                    Some(loc) => {
                        let path = loc.path.clone();
                        self.open_at(&path, None);
                        self.status.clear();
                    }
                    None => self.switch_by_name(),
                }
                continue;
            }
            if r.locations.is_empty() {
                self.status = format!("No {} found{symbol} (clangd may still be indexing the project)", r.kind.noun());
                continue;
            }
            let hits = self.locations_to_hits(&r.locations);
            let count: usize = hits.iter().map(|h| h.lines.len()).sum();
            let title = format!("{}{symbol}", r.kind.title());
            self.status = format!("{title}: {count} found");
            match r.kind {
                NavKind::FindAllReferences => {
                    self.references = Some(NavList { title, hits, pos: None });
                    self.side = Some(SideView::References);
                }
                _ if count == 1 => {
                    let (path, m) = (hits[0].path.clone(), &hits[0].lines[0]);
                    let (a, b) = m.cols[0];
                    self.open_at(&path, Some((m.line, a, b)));
                }
                _ => self.nav_popup = Some(NavList { title, hits, pos: self.nav_pos }),
            }
        }
    }

    /// clangd's places as files with lines (like search results), the text read from the open
    /// tab or the disk.
    fn locations_to_hits(&self, locations: &[crate::lsp::Location]) -> Vec<crate::search::Hit> {
        let root = self.explorer.as_ref().map(|e| e.root.clone());
        let mut hits: Vec<crate::search::Hit> = Vec::new();
        let mut texts: std::collections::HashMap<PathBuf, String> = Default::default();
        for loc in locations {
            let text = texts.entry(loc.path.clone()).or_insert_with(|| {
                let open = self.tabs.iter().find_map(|t| match &t.content {
                    crate::tabs::Content::DiskText { path, text, .. }
                        if std::fs::canonicalize(path).ok().as_deref() == Some(loc.path.as_path()) || *path == loc.path =>
                    {
                        Some(text.clone())
                    }
                    _ => None,
                });
                open.unwrap_or_else(|| std::fs::read(&loc.path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default())
            });
            let line_text = text.split('\n').nth(loc.line).unwrap_or("").trim_end_matches('\r');
            let a = crate::lsp::char_col(line_text, loc.start_utf16);
            let b = crate::lsp::char_col(line_text, loc.end_utf16).max(a);
            let m = crate::search::LineMatch::new(loc.line + 1, line_text, vec![(a, b)]);
            match hits.iter_mut().find(|h| h.path == loc.path) {
                Some(h) => {
                    if !h.lines.iter().any(|l| l.line == m.line && l.cols == m.cols) {
                        h.lines.push(m);
                    }
                }
                None => {
                    let name = loc.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let parent = loc.path.parent().unwrap_or(&loc.path);
                    let dir = match root.as_deref().and_then(|r| parent.strip_prefix(r).ok()) {
                        Some(rel) => rel.to_string_lossy().replace('\\', "/"),
                        None => parent.display().to_string(),
                    };
                    hits.push(crate::search::Hit { path: loc.path.clone(), name, dir, lines: vec![m], binary_matches: 0 });
                }
            }
        }
        for h in &mut hits {
            h.lines.sort_by_key(|l| (l.line, l.cols.first().copied()));
        }
        hits.sort_by_cached_key(|h| (h.dir.to_lowercase(), h.name.to_lowercase()));
        hits
    }

    /// The places found for Go to ... when there are several: a list near where it was asked.
    /// Clicking one opens it; clicking elsewhere or Esc closes the list.
    fn nav_popup_window(&mut self, ctx: &egui::Context) {
        let Some(list) = self.nav_popup.take() else { return };
        let pos = list.pos.unwrap_or_else(|| ctx.content_rect().center() - vec2(260.0, 160.0));
        let focused = self.focused_disk_path();
        let mut open = None;
        let area = egui::Area::new(egui::Id::unique("nav_popup")).order(egui::Order::Foreground).fixed_pos(pos).constrain(true).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(520.0);
                ui.strong(&list.title);
                ui.add_space(4.0);
                let rows: usize = list.hits.iter().map(|h| 1 + h.lines.len()).sum();
                ui.allocate_ui(vec2(520.0, (rows as f32 * 22.0).min(330.0)), |ui| {
                    open = hit_list(ui, "nav_popup_list", &list.hits, true, focused.as_deref());
                });
            });
        });
        let close = open.is_some() || area.response.clicked_elsewhere() || ctx.input(|i| i.key_pressed(Key::Escape));
        if let Some((path, at)) = open {
            self.open_at(&path, at);
        }
        if !close {
            self.nav_popup = Some(list);
        }
    }

    /// The References panel: the results of Find All References, by file.
    fn references_panel(&mut self, ui: &mut Ui) {
        self.panel_header(ui, "REFERENCES", None);
        ui.add_space(2.0);
        let Some(list) = self.references.take() else {
            ui.weak("Right-click a symbol in a C/C++ file and choose Find All References (Shift+Alt+F12).");
            return;
        };
        let count: usize = list.hits.iter().map(|h| h.lines.len()).sum();
        let files = list.hits.len();
        let mut clear = false;
        ui.horizontal(|ui| {
            ui.label(&list.title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clear = ui.small_button("\u{d7}").on_hover_text("Clear the results").clicked();
            });
        });
        let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        ui.weak(format!("{} in {}", plural(count, "result", "results"), plural(files, "file", "files")));
        ui.add_space(2.0);
        let focused = self.focused_disk_path();
        let open = hit_list(ui, "references_list", &list.hits, true, focused.as_deref());
        if !clear {
            self.references = Some(list);
        } else if self.side == Some(SideView::References) {
            self.side = Some(SideView::Explorer);
        }
        if let Some((path, at)) = open {
            self.open_at(&path, at);
        }
    }

    /// The diagram palette: a header with a hide button, a tab per category (Objects, Collections,
    /// Pointers, Relations) and the shapes or lines of the chosen one. Clicking a shape adds it
    /// to the active diagram (a new diagram if there is none).
    fn palette(&mut self, ui: &mut Ui, view: PaletteView) {
        self.panel_header(ui, "DIAGRAM", None);
        ui.add_space(2.0);
        // The category tabs: plain rectangles, the full width of the panel.
        ui.spacing_mut().item_spacing.y = 2.0;
        for v in PaletteView::ALL {
            if rect_button(ui, v.title(), v == view).clicked() {
                self.palette_view = v;
            }
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            match view {
                PaletteView::Objects => self.palette_tiles(ui, &ShapeKind::OBJECTS),
                PaletteView::Collections => self.palette_tiles(ui, &ShapeKind::COLLECTIONS),
                PaletteView::Pointers => self.pointer_tiles(ui),
                PaletteView::Relations => self.relation_tiles(ui),
            }
            if self.link_tool.is_some() {
                ui.add_space(4.0);
                ui.weak("Drag from one object to another. Esc cancels.");
            }
        });
    }

    /// One palette tile showing a line style with a caption; returns true when clicked.
    fn link_tile(&mut self, ui: &mut Ui, kind: LinkKind, caption: &str, hint: &str) {
        let (rect, resp) = ui.allocate_exact_size(vec2(52.0, 44.0), Sense::click());
        let resp = resp.on_hover_text(format!("{} ({hint})", kind.name()));
        let active = self.link_tool == Some(kind);
        if active {
            ui.painter().rect(rect, 4.0, ui.visuals().selection.bg_fill, ui.visuals().selection.stroke, egui::StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
        }
        let col = ui.visuals().text_color();
        let y = rect.top() + 14.0;
        let pts = [Pos2::new(rect.left() + 6.0, y), Pos2::new(rect.right() - 6.0, y)];
        let (pattern, start, end) = match kind {
            LinkKind::Pointer(k) => k.look(),
            LinkKind::Relation(k) => k.look(),
        };
        paint_edge(ui.painter(), &pts, col, 1.2, pattern, start, end, 0.6);
        ui.painter().text(
            Pos2::new(rect.center().x, rect.bottom() - 12.0),
            egui::Align2::CENTER_CENTER,
            caption,
            egui::FontId::monospace(10.0),
            col,
        );
        if resp.clicked() {
            if active {
                self.link_tool = None;
            } else {
                // Lines are drawn on the active diagram: make sure there is one, in view.
                self.ensure_diagram();
                self.link_tool = Some(kind);
            }
        }
    }

    fn relation_tiles(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            self.link_tile(ui, LinkKind::Relation(RelationKind::Related), "related", "click, then drag between two objects");
            self.link_tile(ui, LinkKind::Relation(RelationKind::Implements), "impl", "click, then drag from a class to a trait");
            self.link_tile(
                ui,
                LinkKind::Relation(RelationKind::Inherits),
                "inherit",
                "click, then drag from the derived class to the base class",
            );
            self.link_tile(
                ui,
                LinkKind::Relation(RelationKind::Uses),
                "uses",
                "click, then drag from a type to a type it calls (or between packages: a dependency)",
            );
        });
    }

    fn pointer_tiles(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            for kind in PointerKind::ALL {
                let short = match kind {
                    PointerKind::Raw => "T*",
                    PointerKind::Reference => "T&",
                    PointerKind::Unique => "unique",
                    PointerKind::Shared => "shared",
                    PointerKind::Weak => "weak",
                };
                self.link_tile(ui, LinkKind::Pointer(kind), short, "click, then drag from the owner to the pointed-to object");
            }
        });
        ui.checkbox(&mut self.pointer_const, "Points to const")
            .on_hover_text("New pointers point to a const object");
    }

    fn palette_tiles(&mut self, ui: &mut Ui, kinds: &[ShapeKind]) {
        let tile = vec2(52.0, 44.0);
        ui.horizontal_wrapped(|ui| {
            for &kind in kinds {
                let (rect, resp) = ui.allocate_exact_size(tile, Sense::click_and_drag());
                let resp = resp.on_hover_text(format!("{} (click or drag onto the canvas)", kind.name()));
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
                }
                // Icon in the top part, a short caption below (class and trait icons look alike when small).
                let size = kind.default_size();
                let icon_area = Rect::from_min_max(rect.min + vec2(7.0, 5.0), rect.right_bottom() - vec2(7.0, 17.0));
                let scale = (icon_area.width() / size.x).min(icon_area.height() / size.y);
                let icon = Rect::from_center_size(icon_area.center(), size * scale);
                let stroke_col = ui.visuals().text_color();
                let deco = Deco { stereotype: kind.stereotype(), name: "" };
                let fill = crate::render::tone_fill(Color32::WHITE);
                paint_shape(ui.painter(), kind, icon, fill, Stroke::new(1.2, stroke_col), scale, 12.0 * scale, deco);
                let caption = match kind {
                    ShapeKind::Trait => "trait",
                    ShapeKind::Enum => "enum",
                    ShapeKind::External => "extern",
                    ShapeKind::Package => "package",
                    ShapeKind::Directory => "dir",
                    ShapeKind::File => "text file",
                    ShapeKind::BinaryFile => "bin file",
                    ShapeKind::Set => "set",
                    ShapeKind::Vector => "vector",
                    ShapeKind::Dictionary => "dict",
                    ShapeKind::Tree => "tree",
                    ShapeKind::Queue => "queue",
                    _ => "class",
                };
                ui.painter().text(
                    egui::pos2(rect.center().x, rect.bottom() - 9.0),
                    egui::Align2::CENTER_CENTER,
                    caption,
                    egui::FontId::monospace(10.0),
                    stroke_col,
                );

                if resp.clicked() {
                    self.add_shape_from_palette(kind);
                }
                if resp.drag_started() {
                    self.palette_drag = Some(kind);
                }
                if resp.drag_stopped() {
                    if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
                        // Onto whichever visible diagram is under the mouse.
                        if let Some(i) = self.diagram_tab_at(p) {
                            self.switch_tab(i);
                            let w = self.to_world(p);
                            self.add_node_at(kind, w);
                        }
                    }
                    self.palette_drag = None;
                }
            }
        });
    }

    fn format_panel(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        let node_ids: Vec<Id> = self.selection.iter().copied().filter(|id| self.diagram.node(*id).is_some()).collect();
        let edge_ids: Vec<Id> = self.selection.iter().copied().filter(|id| self.diagram.edge(*id).is_some()).collect();

        if node_ids.is_empty() && edge_ids.is_empty() {
            return;
        }

        if !node_ids.is_empty() {
            self.node_format(ui, &node_ids);
        }
        if !edge_ids.is_empty() {
            if !node_ids.is_empty() {
                ui.add_space(8.0);
            }
            self.edge_format(ui, &edge_ids);
        }

        ui.add_space(8.0);
        section(ui, "Arrange", "fmt.arrange", |ui| {
            ui.horizontal(|ui| {
                if ui.button("To front").clicked() {
                    self.diagram.reorder(&self.selection, true);
                }
                if ui.button("To back").clicked() {
                    self.diagram.reorder(&self.selection, false);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Duplicate").clicked() {
                    self.duplicate();
                }
                if ui.button("Delete").clicked() {
                    self.delete_selected();
                }
            });
        });
    }

    fn node_format(&mut self, ui: &mut Ui, ids: &[Id]) {
        let before = self.diagram.node(ids[0]).unwrap().clone();
        let mut n = before.clone();
        let single_class = ids.len() == 1 && n.kind.is_named();
        let single_collection = ids.len() == 1 && n.kind.is_collection();
        // Element type set by the collection's pointer, if it has one.
        let element_from = self
            .diagram
            .element_pointer(n.id, None)
            .filter(|_| single_collection)
            .and_then(|e| e.to.node())
            .and_then(|id| self.diagram.node(id))
            .map(|t| t.text.clone());

        if single_collection {
            section(ui, "Collection", "fmt.object", |ui| {
                egui::Grid::new("collection_props").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    // Chosen when the collection is created; it cannot be changed.
                    ui.label("Kind");
                    ui.label(n.kind.name());
                    ui.end_row();
                    if n.kind == ShapeKind::Dictionary {
                        ui.label("Key");
                        ui.add(egui::TextEdit::singleline(&mut n.key).hint_text("e.g. String").desired_width(f32::INFINITY));
                        ui.end_row();
                    }
                    ui.label(if n.kind == ShapeKind::Dictionary { "Value" } else { "Element" });
                    match &element_from {
                        Some(target) => {
                            ui.label(format!("{target}  (from its pointer)"));
                        }
                        None => {
                            ui.add(egui::TextEdit::singleline(&mut n.text).hint_text("e.g. i32").desired_width(f32::INFINITY));
                        }
                    }
                    ui.end_row();
                });
                ui.add_space(4.0);
                ui.weak("Draw a pointer from the collection to an object to make that object the element type.");
            });
        } else if single_class {
            let title = match n.kind {
                ShapeKind::Trait => "Trait",
                ShapeKind::File => "Text file",
                ShapeKind::BinaryFile => "Binary file",
                ShapeKind::Directory => "Directory",
                ShapeKind::Enum => "Enum",
                ShapeKind::External => "External type",
                ShapeKind::Package => "Package",
                _ => "Class",
            };
            section(ui, title, "fmt.object", |ui| {
                egui::Grid::new("class_props").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label("Name");
                    let hint = match n.kind {
                        ShapeKind::External => "e.g. tokio::sync::Notify",
                        ShapeKind::File | ShapeKind::BinaryFile => "e.g. config/settings.toml",
                        ShapeKind::Directory => "e.g. data/<Table>/",
                        _ => "",
                    };
                    let r = ui.add(egui::TextEdit::singleline(&mut n.text).hint_text(hint).desired_width(f32::INFINITY));
                    // A name may be emptied while typing, but not left empty.
                    if r.gained_focus() {
                        self.name_before_edit = n.text.clone();
                    }
                    if r.lost_focus() {
                        crate::model::keep_name(&mut n.text, &self.name_before_edit, n.kind);
                    }
                    ui.end_row();
                    if n.kind.is_object() {
                        ui.label("Type params").on_hover_text("Generic parameters with bounds, e.g. R: Row");
                        ui.add(egui::TextEdit::singleline(&mut n.type_params).hint_text("none").desired_width(f32::INFINITY));
                        ui.end_row();
                    }
                    if matches!(n.kind, ShapeKind::Class | ShapeKind::Enum) {
                        ui.label("Scope").on_hover_text("How many values of this type a running program has");
                        egui::ComboBox::from_id_salt("scope").selected_text(n.scope.name()).show_ui(ui, |ui| {
                            for s in crate::model::Scope::ALL {
                                ui.selectable_value(&mut n.scope, s, s.name());
                            }
                        });
                        ui.end_row();
                    }
                    if n.kind == ShapeKind::Class {
                        ui.label("Task").on_hover_text("An active object: runs on its own (a thread, timer or async task)");
                        ui.horizontal(|ui| {
                            let mut is_task = n.task.is_some();
                            if ui.checkbox(&mut is_task, "").changed() {
                                n.task = is_task.then(String::new);
                            }
                            if let Some(period) = &mut n.task {
                                ui.add(egui::TextEdit::singleline(period).hint_text("every 1 s").desired_width(f32::INFINITY));
                            }
                        });
                        ui.end_row();
                    }
                    if n.kind == ShapeKind::File {
                        ui.label("Contents");
                        if ui.button("Edit contents").on_hover_text("Or double-click the file").clicked() {
                            self.open_file_request = Some(n.id);
                        }
                        ui.end_row();
                    }
                    if n.kind == ShapeKind::BinaryFile {
                        ui.label("Type");
                        ui.label("binary (chosen when created)");
                        ui.end_row();
                    }
                    if n.kind == ShapeKind::Enum {
                        ui.label("Flags").on_hover_text("Variants are bit flags that combine");
                        ui.checkbox(&mut n.flags, "");
                        ui.end_row();
                    }
                    if n.kind.is_object() {
                        ui.label("Generated").on_hover_text("Source produced by a tool, not written by hand");
                        ui.checkbox(&mut n.generated, "");
                        ui.end_row();
                    }
                });
            });
            if n.kind.is_object() {
                members_ui(ui, &mut n);
            }
        }

        let style_title = if single_class || single_collection {
            "Style".to_owned()
        } else if ids.len() == 1 {
            "Object".to_owned()
        } else {
            format!("{} objects", ids.len())
        };
        section(ui, style_title, "fmt.style", |ui| {
            egui::Grid::new("node_style").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Fill");
                ui.color_edit_button_srgba_unmultiplied(&mut n.fill);
                ui.end_row();
                ui.label("Line");
                ui.color_edit_button_srgba_unmultiplied(&mut n.stroke);
                ui.end_row();
                ui.label("Line width");
                ui.add(egui::DragValue::new(&mut n.stroke_width).range(0.0..=20.0).speed(0.1).suffix(" pt"));
                ui.end_row();
                ui.label("Font color");
                ui.color_edit_button_srgba_unmultiplied(&mut n.text_color);
                ui.end_row();
                ui.label("Font size");
                ui.add(egui::DragValue::new(&mut n.font_size).range(4.0..=96.0).speed(0.2).suffix(" pt"));
                ui.end_row();
            });

            if ids.len() == 1 {
                if !single_class && !single_collection {
                    ui.add_space(6.0);
                    ui.label("Text");
                    ui.add(egui::TextEdit::multiline(&mut n.text).desired_rows(2).desired_width(f32::INFINITY));
                }
                ui.add_space(6.0);
                egui::Grid::new("node_geom").num_columns(4).spacing([6.0, 6.0]).show(ui, |ui| {
                    ui.label("X");
                    ui.add(egui::DragValue::new(&mut n.pos[0]));
                    ui.label("Y");
                    ui.add(egui::DragValue::new(&mut n.pos[1]));
                    ui.end_row();
                    ui.label("W");
                    ui.add(egui::DragValue::new(&mut n.size[0]).range(10.0..=10000.0));
                    ui.label("H");
                    ui.add(egui::DragValue::new(&mut n.size[1]).range(10.0..=10000.0));
                    ui.end_row();
                });
            }
        });

        if n == before {
            return;
        }
        if ids.len() == 1 {
            // Grow a collection so its element type stays readable.
            if n.kind.is_collection() && (n.text != before.text || n.key != before.key || n.kind != before.kind) {
                let mut probe = n.clone();
                probe.size = [1.0, 1.0];
                let text = {
                    let mut d = self.diagram.clone();
                    *d.node_mut(n.id).unwrap() = probe;
                    d.display_text(d.node(n.id).unwrap())
                };
                let font = egui::FontId::proportional(n.font_size);
                let w = ui.painter().layout_no_wrap(text, font, Color32::BLACK).size().x + n.font_size * 3.0;
                n.size[0] = n.size[0].max(w);
            }
            // Grow the box so the name and all members stay visible.
            let header_changed = n.type_params != before.type_params
                || n.scope != before.scope
                || n.task != before.task
                || n.generated != before.generated
                || n.flags != before.flags;
            // Files keep their page shape: a long name wraps inside instead of widening them.
            if n.kind.is_named()
                && !n.kind.is_storage()
                && (n.members != before.members
                    || n.text != before.text
                    || n.font_size != before.font_size
                    || header_changed)
            {
                let painter = ui.painter();
                let font = egui::FontId::proportional(n.font_size);
                let need = n.required_size(|s| painter.layout_no_wrap(s.to_owned(), font.clone(), Color32::BLACK).size().x);
                n.size = [n.size[0].max(need.x), n.size[1].max(need.y)];
            }
            *self.diagram.node_mut(ids[0]).unwrap() = n;
            return;
        }
        // Apply only the fields that changed to every selected shape.
        for id in ids {
            let t = self.diagram.node_mut(*id).unwrap();
            if n.kind != before.kind {
                t.kind = n.kind;
            }
            if n.fill != before.fill {
                t.fill = n.fill;
            }
            if n.stroke != before.stroke {
                t.stroke = n.stroke;
            }
            if n.stroke_width != before.stroke_width {
                t.stroke_width = n.stroke_width;
            }
            if n.text_color != before.text_color {
                t.text_color = n.text_color;
            }
            if n.font_size != before.font_size {
                t.font_size = n.font_size;
            }
        }
    }

    fn edge_format(&mut self, ui: &mut Ui, ids: &[Id]) {
        let before = self.diagram.edge(ids[0]).unwrap().clone();
        let mut e = before.clone();
        let heading = match (ids.len(), e.link()) {
            (1, Some(LinkKind::Pointer(_))) => "Pointer".to_owned(),
            (1, Some(LinkKind::Relation(_))) => "Relation".to_owned(),
            (1, None) => "Line".to_owned(),
            (n, _) => format!("{n} lines"),
        };
        // A relation is only offered when every selected line fits its rule (e.g. class -> trait).
        let relation_ok: Vec<bool> = RelationKind::ALL
            .iter()
            .map(|k| {
                ids.iter().all(|id| {
                    let l = self.diagram.edge(*id).unwrap();
                    self.diagram.link_allowed(LinkKind::Relation(*k), l.from, l.to)
                })
            })
            .collect();
        // Pointers cannot touch files; one check covers every pointer kind.
        let pointer_ok = ids.iter().all(|id| {
            let l = self.diagram.edge(*id).unwrap();
            self.diagram.link_allowed_for(Some(*id), LinkKind::Pointer(PointerKind::Raw), l.from, l.to)
        });
        section(ui, heading, "fmt.line", |ui| {
            egui::Grid::new("edge_style").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Type");
                let mut link = e.link();
                egui::ComboBox::from_id_salt("edge_link")
                    .selected_text(link.map_or("Plain connector", |k| k.name()))
                    .show_ui(ui, |ui| {
                        for k in PointerKind::ALL {
                            ui.add_enabled_ui(pointer_ok, |ui| {
                                ui.selectable_value(&mut link, Some(LinkKind::Pointer(k)), k.name())
                                    .on_disabled_hover_text(
                                        "Not allowed here: a file at either end, or a collection that already has its element pointer",
                                    );
                            });
                        }
                        ui.separator();
                        for (k, ok) in RelationKind::ALL.into_iter().zip(&relation_ok) {
                            ui.add_enabled_ui(*ok, |ui| {
                                ui.selectable_value(&mut link, Some(LinkKind::Relation(k)), k.name())
                                    .on_disabled_hover_text(k.rule());
                            });
                        }
                    });
                if link != e.link() {
                    if let Some(k) = link {
                        e.set_link(k);
                    }
                }
                ui.end_row();
                if let Some(kind) = e.pointer {
                    ui.label("Target");
                    ui.checkbox(&mut e.const_target, "const");
                    ui.end_row();
                    ui.label("Lock").on_hover_text("What guards the pointed-to value: Arc<Mutex<T>>, Arc<RwLock<T>>");
                    egui::ComboBox::from_id_salt("edge_lock").selected_text(e.lock.name()).show_ui(ui, |ui| {
                        for l in crate::model::Lock::ALL {
                            ui.selectable_value(&mut e.lock, l, l.name());
                        }
                    });
                    ui.end_row();
                    if kind.can_be_required() {
                        ui.label("Never null").on_hover_text("Always set (shown as [1]); otherwise it may be empty");
                        ui.checkbox(&mut e.required, "");
                        ui.end_row();
                    }
                } else if e.relation.is_some() {
                } else {
                    ui.label("Pattern");
                    egui::ComboBox::from_id_salt("edge_pattern").selected_text(e.pattern.name()).show_ui(ui, |ui| {
                        for p in LinePattern::ALL {
                            ui.selectable_value(&mut e.pattern, p, p.name());
                        }
                    });
                    ui.end_row();
                    for (label, head, salt) in [("Start", &mut e.start_head, "start_head"), ("End", &mut e.end_head, "end_head")] {
                        ui.label(label);
                        egui::ComboBox::from_id_salt(salt).selected_text(head.name()).show_ui(ui, |ui| {
                            for h in Head::ALL {
                                ui.selectable_value(head, h, h.name());
                            }
                        });
                        ui.end_row();
                    }
                }
                ui.label("Routing");
                egui::ComboBox::from_id_salt("edge_style")
                    .selected_text(match e.style {
                        EdgeStyle::Orthogonal => "Orthogonal",
                        EdgeStyle::Straight => "Straight",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut e.style, EdgeStyle::Orthogonal, "Orthogonal");
                        ui.selectable_value(&mut e.style, EdgeStyle::Straight, "Straight");
                    });
                ui.end_row();
                ui.label("Color");
                ui.color_edit_button_srgba_unmultiplied(&mut e.color);
                ui.end_row();
                ui.label("Width");
                ui.add(egui::DragValue::new(&mut e.width).range(0.5..=20.0).speed(0.1).suffix(" pt"));
                ui.end_row();
            });
            if ids.len() == 1 {
                ui.add_space(6.0);
                ui.label(if e.pointer.is_some() { "Label (e.g. member name)" } else { "Label" });
                ui.add(egui::TextEdit::singleline(&mut e.label).desired_width(f32::INFINITY));
            }
        });

        if e == before {
            return;
        }
        if ids.len() == 1 {
            *self.diagram.edge_mut(ids[0]).unwrap() = e;
            return;
        }
        for id in ids {
            let t = self.diagram.edge_mut(*id).unwrap();
            if e.style != before.style {
                t.style = e.style;
            }
            if e.color != before.color {
                t.color = e.color;
            }
            if e.width != before.width {
                t.width = e.width;
            }
            if e.pattern != before.pattern {
                t.pattern = e.pattern;
            }
            if e.start_head != before.start_head {
                t.start_head = e.start_head;
            }
            if e.end_head != before.end_head {
                t.end_head = e.end_head;
            }
            if e.pointer != before.pointer || e.relation != before.relation {
                t.pointer = e.pointer;
                t.relation = e.relation;
            }
            if e.const_target != before.const_target {
                t.const_target = e.const_target;
            }
            if e.lock != before.lock {
                t.lock = e.lock;
            }
            if e.required != before.required {
                t.required = e.required;
            }
        }
    }
}

/// A titled panel section that folds away when its title is clicked. Open by default; whether it
/// is open is remembered (per `id`) for the rest of the session.
fn section<R>(ui: &mut Ui, title: impl Into<String>, id: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    egui::CollapsingHeader::new(egui::RichText::new(title.into()).strong())
        .id_salt(id)
        .default_open(true)
        .show(ui, add_contents)
        .body_returned
}

/// Fields and Methods sections of the format panel for one class or trait.
fn members_ui(ui: &mut Ui, n: &mut Node) {
    match n.kind {
        ShapeKind::Class => member_section(ui, n, MemberKind::Field, "Fields"),
        ShapeKind::Enum => member_section(ui, n, MemberKind::Variant, "Variants"),
        _ => {}
    }
    member_section(ui, n, MemberKind::Method, "Methods");
}

enum RowAction {
    Up,
    Down,
    Remove,
}

fn member_section(ui: &mut Ui, n: &mut Node, kind: MemberKind, title: &str) {
    let count = n.members.iter().filter(|m| m.kind == kind).count();
    section(ui, format!("{title} ({count})"), &format!("fmt.members.{title}"), |ui| member_rows(ui, n, kind));
}

fn member_rows(ui: &mut Ui, n: &mut Node, kind: MemberKind) {
    let owner = n.kind;
    let in_trait = owner == ShapeKind::Trait;
    ui.horizontal(|ui| {
        let label = match kind {
            MemberKind::Field => "+ Add field",
            MemberKind::Method => "+ Add method",
            MemberKind::Variant => "+ Add variant",
        };
        if ui.small_button(label).clicked() {
            let m = Member::new(kind, owner, &n.members);
            n.members.push(m);
        }
    });

    let rows: Vec<usize> = (0..n.members.len()).filter(|&i| n.members[i].kind == kind).collect();
    if rows.is_empty() {
        ui.weak(match kind {
            MemberKind::Field => "No fields.",
            MemberKind::Method => "No methods.",
            MemberKind::Variant => "No variants.",
        });
        return;
    }
    let mut action: Option<(usize, RowAction)> = None;
    for (pos, &i) in rows.iter().enumerate() {
        let m = &mut n.members[i];
        ui.push_id(i, |ui| {
            egui::Frame::group(ui.style()).inner_margin(6.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if m.kind == MemberKind::Variant {
                        // Variants have no visibility of their own.
                        m.visibility = Visibility::Public;
                    } else if in_trait {
                        // Trait members are part of the interface: always public.
                        m.visibility = Visibility::Public;
                        ui.label("+").on_hover_text("Trait members are always public");
                    } else {
                        egui::ComboBox::from_id_salt("vis")
                            .width(36.0)
                            .selected_text(m.visibility.symbol())
                            .show_ui(ui, |ui| {
                                for v in Visibility::ALL {
                                    ui.selectable_value(&mut m.visibility, v, format!("{}  {}", v.symbol(), v.name()));
                                }
                            })
                            .response
                            .on_hover_text(m.visibility.name());
                    }
                    // Right-to-left so the buttons get their real width and the name field fills
                    // exactly what is left (fixed width estimates make the panel creep wider).
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("x").on_hover_text("Remove").clicked() {
                            action = Some((pos, RowAction::Remove));
                        }
                        if ui
                            .add_enabled(pos + 1 < rows.len(), egui::Button::new("v").small())
                            .on_hover_text("Move down")
                            .clicked()
                        {
                            action = Some((pos, RowAction::Down));
                        }
                        if ui.add_enabled(pos > 0, egui::Button::new("^").small()).on_hover_text("Move up").clicked() {
                            action = Some((pos, RowAction::Up));
                        }
                        ui.add(egui::TextEdit::singleline(&mut m.name).hint_text("name").desired_width(f32::INFINITY));
                    });
                });
                if m.kind == MemberKind::Method {
                    ui.horizontal(|ui| {
                        ui.label("params");
                        ui.add(
                            egui::TextEdit::singleline(&mut m.params)
                                .hint_text("a: int, b: &str")
                                .desired_width(f32::INFINITY),
                        );
                    });
                }
                ui.horizontal(|ui| {
                    ui.label(match m.kind {
                        MemberKind::Method => "returns",
                        MemberKind::Field => "type",
                        MemberKind::Variant => "data",
                    });
                    let hint = match m.kind {
                        MemberKind::Method => "none",
                        MemberKind::Field => "int",
                        MemberKind::Variant => "none, or e.g. LoginRequest",
                    };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if m.kind != MemberKind::Variant {
                            ui.checkbox(&mut m.is_static, "static")
                                .on_hover_text("Belongs to the type, not to each value");
                        }
                        ui.add(egui::TextEdit::singleline(&mut m.ty).hint_text(hint).desired_width(f32::INFINITY));
                    });
                });
                {
                    ui.horizontal(|ui| {
                        if m.kind == MemberKind::Method {
                            ui.checkbox(&mut m.is_async, "async").on_hover_text("Runs asynchronously (async fn)");
                        }
                        ui.label("repeat").on_hover_text(
                            "This line stands for this many members of the same shape, e.g. 74 generated \
                             table fields. 0 or 1 = just this one.",
                        );
                        ui.add(egui::DragValue::new(&mut m.repeat).range(0..=100_000));
                    });
                }
            });
        });
    }
    match action {
        Some((pos, RowAction::Up)) => n.members.swap(rows[pos], rows[pos - 1]),
        Some((pos, RowAction::Down)) => n.members.swap(rows[pos], rows[pos + 1]),
        Some((pos, RowAction::Remove)) => {
            n.members.remove(rows[pos]);
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::End;
    use egui::pos2;

    fn app_with_classes() -> (App, Id, Id, Id) {
        let mut app = App::default();
        let c1 = app.diagram.add_node(ShapeKind::Class, pos2(0.0, 0.0));
        let c2 = app.diagram.add_node(ShapeKind::Class, pos2(300.0, 0.0));
        let t = app.diagram.add_node(ShapeKind::Trait, pos2(0.0, 300.0));
        (app, c1, c2, t)
    }

    #[test]
    fn ok_removes_an_incorrect_new_relation() {
        let (mut app, c1, c2, _) = app_with_classes();
        let implements = LinkKind::Relation(RelationKind::Implements);
        let id = app.diagram.add_link(End::Node(c1), End::Node(c2), implements, false);
        let message = app.diagram.link_error(implements, End::Node(c1), End::Node(c2)).unwrap();
        app.link_error = Some(LinkError { id, restore: None, message });

        app.dismiss_link_error();
        assert!(app.diagram.edge(id).is_none());
        assert!(app.link_error.is_none());
    }

    #[test]
    fn ok_restores_a_moved_end_of_a_correct_relation() {
        let (mut app, c1, c2, t) = app_with_classes();
        let implements = LinkKind::Relation(RelationKind::Implements);
        let id = app.diagram.add_link(End::Node(c1), End::Node(t), implements, false);
        // The user dragged the trait end onto another class.
        app.diagram.edge_mut(id).unwrap().to = End::Node(c2);
        app.link_error = Some(LinkError { id, restore: Some((1, End::Node(t))), message: String::new() });

        app.dismiss_link_error();
        assert_eq!(app.diagram.edge(id).unwrap().to, End::Node(t));
    }
}
