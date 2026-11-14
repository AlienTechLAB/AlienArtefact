//! A terminal tab: the system shell (PowerShell on Windows, `$SHELL` on Linux) running in a
//! pseudo-terminal, so programs see a real terminal (prompts, colours, Ctrl+C, history keys).

use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, Event, Key, Sense, Ui};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

/// Lines kept above the screen for scrolling back.
const SCROLLBACK: usize = 5000;

pub struct Terminal {
    /// The screen as the shell drew it (fed by the reader thread).
    parser: Arc<Mutex<vt100::Parser>>,
    /// Shared with the reader thread, which answers the shell's cursor position questions.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    /// Rows and columns.
    size: (u16, u16),
    /// The shell has ended.
    exited: Arc<AtomicBool>,
    /// Lines scrolled back from the bottom.
    scroll: usize,
}

impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminal").field("size", &self.size).finish()
    }
}

/// The shell to run, with its arguments.
fn shell() -> CommandBuilder {
    if cfg!(windows) {
        let mut cmd = CommandBuilder::new("powershell.exe");
        cmd.arg("-NoLogo");
        cmd
    } else {
        CommandBuilder::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into()))
    }
}

impl Terminal {
    /// Starts the shell in `cwd`. `ctx` is asked to repaint whenever the shell writes.
    pub fn start(cwd: Option<&Path>, ctx: egui::Context) -> Result<Self, String> {
        let (rows, cols) = (24u16, 80u16);
        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        let mut cmd = shell();
        cmd.env("TERM", "xterm-256color");
        if let Some(dir) = cwd.filter(|d| d.is_dir()) {
            cmd.cwd(dir);
        } else if let Ok(dir) = std::env::current_dir() {
            cmd.cwd(dir);
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer().map_err(|e| e.to_string())?));
        let nz = |n: u16| NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN);
        let parser = Arc::new(Mutex::new(vt100::Parser::new(nz(rows), nz(cols), SCROLLBACK)));
        let exited = Arc::new(AtomicBool::new(false));
        {
            let parser = parser.clone();
            let exited = exited.clone();
            let writer = writer.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let Ok(mut p) = parser.lock() else { break };
                            p.process(&buf[..n]);
                            // "Where is the cursor?" (ESC [ 6 n): the Windows console asks this
                            // when it starts and waits for the answer, as do some programs.
                            if contains(&buf[..n], b"[6n") {
                                let (row, col) = p.screen().cursor_position();
                                let answer = format!("[{};{}R", row + 1, col + 1);
                                drop(p);
                                if let Ok(mut w) = writer.lock() {
                                    let _ = w.write_all(answer.as_bytes());
                                    let _ = w.flush();
                                }
                            }
                            ctx.request_repaint();
                        }
                    }
                }
                exited.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            });
        }
        Ok(Terminal { parser, writer, master: pair.master, child, size: (rows, cols), exited, scroll: 0 })
    }

    fn send(&mut self, bytes: &[u8]) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// Fits the terminal to `rows` x `cols` characters.
    fn resize(&mut self, rows: u16, cols: u16) {
        if (rows, cols) == self.size || rows == 0 || cols == 0 {
            return;
        }
        self.size = (rows, cols);
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        if let (Ok(mut p), Some(r), Some(c)) = (self.parser.lock(), NonZeroU16::new(rows), NonZeroU16::new(cols)) {
            p.set_size(r, c);
        }
    }

    /// Draws the screen filling the space, and passes the keyboard to the shell while focused.
    pub fn ui(&mut self, ui: &mut Ui) {
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let (cw, rh) = ui.fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click());
        let rows = ((rect.height() - 8.0) / rh).floor().max(2.0) as u16;
        let cols = ((rect.width() - 12.0) / cw).floor().max(10.0) as u16;
        self.resize(rows, cols);

        let id = resp.id;
        if resp.clicked() {
            resp.request_focus();
        }
        let focused = resp.has_focus();
        if focused {
            // Tab, arrows and Esc go to the shell, not to moving the focus.
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                )
            });
            self.keyboard(ui);
        }
        if resp.hovered() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 {
                let lines = (dy / rh).round() as isize;
                self.scroll = (self.scroll as isize + lines).clamp(0, SCROLLBACK as isize) as usize;
            }
        }

        let visuals = ui.visuals().clone();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, visuals.extreme_bg_color);
        let Ok(mut parser) = self.parser.lock() else { return };
        parser.screen_mut().set_scrollback(self.scroll);
        self.scroll = parser.screen().scrollback();
        let screen = parser.screen();
        let default_fg = visuals.text_color();
        let origin = rect.min + egui::vec2(6.0, 4.0);
        for row in 0..rows {
            let job = row_job(screen, row, cols, &font, default_fg);
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            painter.galley(origin + egui::vec2(0.0, row as f32 * rh), galley, default_fg);
        }
        if focused && self.scroll == 0 && !screen.hide_cursor() {
            let (r, c) = screen.cursor_position();
            let at = origin + egui::vec2(c as f32 * cw, r as f32 * rh);
            painter.rect_filled(egui::Rect::from_min_size(at, egui::vec2(cw, rh)), 0.0, default_fg.gamma_multiply(0.6));
        }
        if self.exited.load(Ordering::Relaxed) {
            painter.text(
                rect.right_bottom() - egui::vec2(8.0, 6.0),
                egui::Align2::RIGHT_BOTTOM,
                "[the shell has ended; close the tab]",
                font,
                visuals.weak_text_color(),
            );
        }
    }

    /// Turns this frame's key presses and typed text into what a terminal sends.
    fn keyboard(&mut self, ui: &Ui) {
        let app_cursor = self.parser.lock().map(|p| p.screen().application_cursor()).unwrap_or(false);
        let events = ui.input(|i| i.events.clone());
        let mut out: Vec<u8> = Vec::new();
        for e in events {
            match e {
                Event::Text(t) => out.extend_from_slice(t.as_bytes()),
                Event::Paste(t) => out.extend_from_slice(t.replace("\r\n", "\r").replace('\n', "\r").as_bytes()),
                // Ctrl+C and Ctrl+X arrive as copy and cut: in a terminal they are control keys.
                Event::Copy => out.push(0x03),
                Event::Cut => out.push(0x18),
                Event::Key { key, pressed: true, modifiers, .. } => {
                    if let Some(bytes) = key_bytes(key, modifiers, app_cursor) {
                        out.extend_from_slice(&bytes);
                    }
                }
                _ => {}
            }
        }
        if !out.is_empty() {
            self.scroll = 0;
            self.send(&out);
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// The bytes a terminal sends for a key that is not plain text.
fn key_bytes(key: Key, m: egui::Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let arrow = |c: u8| if app_cursor { vec![0x1b, b'O', c] } else { vec![0x1b, b'[', c] };
    let s = |x: &str| Some(x.as_bytes().to_vec());
    match key {
        Key::Enter => s("\r"),
        Key::Backspace => Some(vec![0x7f]),
        Key::Tab if m.shift => s("\x1b[Z"),
        Key::Tab => s("\t"),
        Key::Escape => s("\x1b"),
        Key::ArrowUp => Some(arrow(b'A')),
        Key::ArrowDown => Some(arrow(b'B')),
        Key::ArrowRight => Some(arrow(b'C')),
        Key::ArrowLeft => Some(arrow(b'D')),
        Key::Home => s("\x1b[H"),
        Key::End => s("\x1b[F"),
        Key::Delete => s("\x1b[3~"),
        Key::Insert => s("\x1b[2~"),
        Key::PageUp => s("\x1b[5~"),
        Key::PageDown => s("\x1b[6~"),
        // Ctrl+letter: the control character (Ctrl+A = 1, ... Ctrl+Z = 26).
        _ if m.ctrl && !m.alt => {
            let name = key.name();
            let c = name.chars().next().filter(|c| name.len() == 1 && c.is_ascii_alphabetic())?;
            Some(vec![c.to_ascii_uppercase() as u8 - b'A' + 1])
        }
        _ => None,
    }
}

/// One screen row as coloured text: runs of cells with the same look become one section.
fn row_job(screen: &vt100::Screen, row: u16, cols: u16, font: &egui::FontId, default_fg: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut run = String::new();
    let mut run_format: Option<TextFormat> = None;
    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else { break };
        if cell.is_wide_continuation() {
            continue;
        }
        let (mut fg, mut bg) = (ansi(cell.fgcolor(), default_fg), ansi(cell.bgcolor(), Color32::TRANSPARENT));
        if cell.inverse() {
            std::mem::swap(&mut fg, &mut bg);
            if fg == Color32::TRANSPARENT {
                fg = Color32::BLACK;
            }
        }
        if cell.dim() {
            fg = fg.gamma_multiply(0.6);
        }
        let format = TextFormat {
            font_id: font.clone(),
            color: fg,
            background: bg,
            italics: cell.italic(),
            underline: if cell.underline() { egui::Stroke::new(1.0, fg) } else { egui::Stroke::NONE },
            ..Default::default()
        };
        if run_format.as_ref() != Some(&format) {
            if let Some(f) = run_format.take() {
                job.append(&run, 0.0, f);
                run.clear();
            }
            run_format = Some(format);
        }
        if cell.has_contents() { run.push_str(cell.contents()) } else { run.push(' ') }
    }
    if let Some(f) = run_format {
        job.append(run.trim_end(), 0.0, f);
    }
    job
}

/// A terminal colour: the 16 standard colours, the 256-colour cube and greys, or RGB.
fn ansi(c: vt100::Color, default: Color32) -> Color32 {
    const BASE: [(u8, u8, u8); 16] = [
        (12, 12, 12), (197, 15, 31), (19, 161, 14), (193, 156, 0),
        (0, 55, 218), (136, 23, 152), (58, 150, 221), (204, 204, 204),
        (118, 118, 118), (231, 72, 86), (22, 198, 12), (249, 241, 165),
        (59, 120, 255), (180, 0, 158), (97, 214, 214), (242, 242, 242),
    ];
    match c {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        vt100::Color::Idx(i) if i < 16 => {
            let (r, g, b) = BASE[i as usize];
            Color32::from_rgb(r, g, b)
        }
        vt100::Color::Idx(i) if i < 232 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        vt100::Color::Idx(i) => {
            let g = 8 + (i - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_become_terminal_bytes() {
        let none = egui::Modifiers::NONE;
        assert_eq!(key_bytes(Key::Enter, none, false), Some(b"\r".to_vec()));
        assert_eq!(key_bytes(Key::ArrowUp, none, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes(Key::ArrowUp, none, true), Some(b"\x1bOA".to_vec()), "application cursor mode");
        assert_eq!(key_bytes(Key::D, egui::Modifiers::CTRL, false), Some(vec![4]), "Ctrl+D");
        assert_eq!(key_bytes(Key::A, none, false), None, "plain letters arrive as text");
    }

    #[test]
    fn colours_of_the_palette_and_cube() {
        assert_eq!(ansi(vt100::Color::Idx(1), Color32::WHITE), Color32::from_rgb(197, 15, 31));
        assert_eq!(ansi(vt100::Color::Idx(196), Color32::WHITE), Color32::from_rgb(255, 0, 0));
        assert_eq!(ansi(vt100::Color::Idx(232), Color32::WHITE), Color32::from_rgb(8, 8, 8));
        assert_eq!(ansi(vt100::Color::Default, Color32::WHITE), Color32::WHITE);
    }

    #[test]
    fn a_shell_starts_and_answers() {
        let ctx = egui::Context::default();
        let mut t = Terminal::start(None, ctx).expect("a shell starts");
        t.send(b"echo alien-artefact-test\r");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let text = t.parser.lock().unwrap().screen().contents();
            if text.matches("alien-artefact-test").count() >= 2 {
                break; // the command line and its output
            }
            assert!(std::time::Instant::now() < deadline, "no answer from the shell:\n{text}");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}
