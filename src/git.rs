//! The Git tab: source control for the explorer's folder, as in VS Code's Source Control view.
//!
//! Changes: the branch, ahead/behind its upstream, fetch / pull / push; the staged and unstaged
//! files (stage, unstage, discard, one or all), a commit message with Commit (and Amend), and the
//! diff of the selected file. History: the last commits; selecting one shows its patch.
//!
//! Everything runs the `git` program on a worker thread, one command after the other, so the
//! window never waits for it. Git is told never to ask for a password on a terminal
//! (`GIT_TERMINAL_PROMPT=0`): a push that needs one fails with git's message instead of hanging.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use egui::{Color32, RichText, Ui};

/// A file in `git status`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Relative to the repository's root, with `/`.
    pub path: String,
    /// Status in the index (staged) and in the working tree: `.` unchanged, `M` modified, `A`
    /// added, `D` deleted, `R` renamed, `C` copied, `U` unmerged, `?` untracked.
    pub index: char,
    pub worktree: char,
    /// The old name of a renamed or copied file.
    pub orig: Option<String>,
}

impl Entry {
    pub fn staged(&self) -> bool {
        !matches!(self.index, '.' | '?')
    }
    pub fn unstaged(&self) -> bool {
        self.worktree != '.'
    }
    pub fn conflicted(&self) -> bool {
        self.index == 'U' || self.worktree == 'U' || (self.index == 'A' && self.worktree == 'A') || (self.index == 'D' && self.worktree == 'D')
    }
}

/// `git status` of the repository.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: i64,
    pub behind: i64,
    /// No commit yet.
    pub initial: bool,
    pub entries: Vec<Entry>,
}

/// Parses `git status --porcelain=v2 --branch -z`.
pub fn parse_status(out: &str) -> Status {
    let mut st = Status::default();
    let mut fields = out.split('\0').peekable();
    while let Some(rec) = fields.next() {
        if let Some(h) = rec.strip_prefix("# ") {
            if let Some(v) = h.strip_prefix("branch.head ") {
                st.branch = if v == "(detached)" { "(detached HEAD)".into() } else { v.into() };
            } else if let Some(v) = h.strip_prefix("branch.upstream ") {
                st.upstream = Some(v.into());
            } else if let Some(v) = h.strip_prefix("branch.ab ") {
                let mut it = v.split(' ');
                st.ahead = it.next().and_then(|a| a.trim_start_matches('+').parse().ok()).unwrap_or(0);
                st.behind = it.next().and_then(|b| b.trim_start_matches('-').parse().ok()).unwrap_or(0);
            } else if h == "branch.oid (initial)" {
                st.initial = true;
            }
            continue;
        }
        let xy = |s: &str| {
            let mut c = s.chars();
            (c.next().unwrap_or('.'), c.next().unwrap_or('.'))
        };
        if let Some(r) = rec.strip_prefix("1 ") {
            // XY sub mH mI mW hH hI path
            let parts: Vec<&str> = r.splitn(8, ' ').collect();
            if parts.len() == 8 {
                let (index, worktree) = xy(parts[0]);
                st.entries.push(Entry { path: parts[7].into(), index, worktree, orig: None });
            }
        } else if let Some(r) = rec.strip_prefix("2 ") {
            // XY sub mH mI mW hH hI Xscore path, then the original path as the next field.
            let parts: Vec<&str> = r.splitn(9, ' ').collect();
            if parts.len() == 9 {
                let (index, worktree) = xy(parts[0]);
                let orig = fields.next().map(str::to_owned);
                st.entries.push(Entry { path: parts[8].into(), index, worktree, orig });
            }
        } else if let Some(r) = rec.strip_prefix("u ") {
            // XY sub m1 m2 m3 mW h1 h2 h3 path
            let parts: Vec<&str> = r.splitn(10, ' ').collect();
            if parts.len() == 10 {
                let (index, worktree) = xy(parts[0]);
                st.entries.push(Entry { path: parts[9].into(), index, worktree, orig: None });
            }
        } else if let Some(p) = rec.strip_prefix("? ") {
            st.entries.push(Entry { path: p.into(), index: '?', worktree: '?', orig: None });
        }
    }
    st.entries.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    st
}

/// A commit in the history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub author: String,
    pub when: String,
    pub refs: String,
    pub subject: String,
}

const LOG_FORMAT: &str = "--format=%H%x1f%h%x1f%an%x1f%ar%x1f%D%x1f%s%x1e";

fn parse_log(out: &str) -> Vec<Commit> {
    out.split('\x1e')
        .filter_map(|rec| {
            let f: Vec<&str> = rec.trim_start_matches('\n').split('\x1f').collect();
            (f.len() == 6).then(|| Commit {
                hash: f[0].into(),
                short: f[1].into(),
                author: f[2].into(),
                when: f[3].into(),
                refs: f[4].into(),
                subject: f[5].into(),
            })
        })
        .collect()
}

/// Runs git in `dir`. Its output, or its error message.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    git_input(dir, args, None)
}

fn git_input(dir: &Path, args: &[&str], input: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args).env("GIT_TERMINAL_PROMPT", "0").stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound { "git is not installed".to_owned() } else { format!("cannot run git: {e}") }
    })?;
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        use std::io::Write;
        let _ = stdin.write_all(text.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(if err.is_empty() { stdout.trim().to_owned() } else { err })
    }
}

/// At most this many lines of a diff are shown.
const MAX_DIFF_LINES: usize = 20_000;

fn lines_of(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text.lines().take(MAX_DIFF_LINES).map(str::to_owned).collect();
    if text.lines().nth(MAX_DIFF_LINES).is_some() {
        lines.push(format!("... (only the first {MAX_DIFF_LINES} lines are shown)"));
    }
    lines
}

/// A new file's contents shown as a diff (every line added).
fn untracked_diff(root: &Path, path: &str) -> Vec<String> {
    match std::fs::read(root.join(path)) {
        Err(e) => vec![format!("Cannot read {path}: {e}")],
        Ok(b) if b.contains(&0) => vec![format!("new binary file {path}")],
        Ok(b) => {
            let text = String::from_utf8_lossy(&b);
            let mut lines = vec![format!("new file {path}")];
            lines.extend(text.lines().take(MAX_DIFF_LINES).map(|l| format!("+{l}")));
            lines
        }
    }
}

/// What the diff pane shows.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Shown {
    File { path: String, staged: bool },
    Commit(String),
}

/// What the worker thread sends back.
enum Reply {
    Root(Result<PathBuf, String>),
    Status(Result<Status, String>),
    Branches(Vec<String>),
    Log(Result<Vec<Commit>, String>),
    Diff(Shown, Vec<String>),
    /// A command's result; then the status is read again.
    Ran { what: String, result: Result<String, String> },
}

type Job = Box<dyn FnOnce() -> Reply + Send>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Changes,
    History,
}

/// What the Git tab asks the app to do.
pub enum Action {
    /// Open this file in an editor tab.
    Open(PathBuf),
}

/// The Git tab.
pub struct GitTool {
    /// The folder it was opened for, and its repository's root once known.
    dir: PathBuf,
    root: Option<PathBuf>,
    /// Why there is no repository (not one, or git missing); `None` while unknown or found.
    no_repo: Option<String>,
    status: Status,
    branches: Vec<String>,
    log: Vec<Commit>,
    view: View,
    shown: Option<Shown>,
    diff: Vec<String>,
    message: String,
    amend: bool,
    /// The last command's result: success, text.
    output: Option<(bool, String)>,
    jobs: Sender<Job>,
    replies: Receiver<Reply>,
    /// Commands sent and not answered yet.
    running: usize,
    last_status: Instant,
}

impl GitTool {
    pub fn new(dir: PathBuf, ctx: egui::Context) -> GitTool {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (reply_tx, replies) = mpsc::channel();
        let repaint = ctx;
        std::thread::spawn(move || {
            for job in job_rx {
                if reply_tx.send(job()).is_err() {
                    break;
                }
                repaint.request_repaint();
            }
        });
        let mut tool = GitTool {
            dir,
            root: None,
            no_repo: None,
            status: Status::default(),
            branches: Vec::new(),
            log: Vec::new(),
            view: View::Changes,
            shown: None,
            diff: Vec::new(),
            message: String::new(),
            amend: false,
            output: None,
            jobs,
            replies,
            running: 0,
            last_status: Instant::now(),
        };
        tool.find_root();
        tool
    }

    fn send(&mut self, job: impl FnOnce() -> Reply + Send + 'static) {
        if self.jobs.send(Box::new(job)).is_ok() {
            self.running += 1;
        }
    }

    fn find_root(&mut self) {
        let dir = self.dir.clone();
        self.send(move || Reply::Root(git(&dir, &["rev-parse", "--show-toplevel"]).map(|s| PathBuf::from(s.trim()))));
    }

    /// Reads the status, branches and history again.
    pub fn refresh(&mut self) {
        let Some(root) = self.root.clone() else {
            self.find_root();
            return;
        };
        self.last_status = Instant::now();
        let r = root.clone();
        self.send(move || Reply::Status(git(&r, &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all"]).map(|s| parse_status(&s))));
        let r = root.clone();
        self.send(move || {
            let out = git(&r, &["branch", "--format=%(refname:short)"]).unwrap_or_default();
            Reply::Branches(out.lines().map(str::to_owned).collect())
        });
        let r = root;
        self.send(move || Reply::Log(git(&r, &["log", "-n", "300", LOG_FORMAT]).map(|s| parse_log(&s))));
        if let Some(shown) = self.shown.clone() {
            self.show(shown);
        }
    }

    /// Runs a git command that changes something, then refreshes.
    fn run(&mut self, what: &str, args: Vec<String>, input: Option<String>) {
        let Some(root) = self.root.clone() else { return };
        let what = what.to_owned();
        self.send(move || {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            Reply::Ran { what, result: git_input(&root, &args, input.as_deref()) }
        });
    }

    fn show(&mut self, shown: Shown) {
        let Some(root) = self.root.clone() else { return };
        self.shown = Some(shown.clone());
        let untracked = matches!(&shown, Shown::File { path, .. } if self.status.entries.iter().any(|e| e.path == *path && e.index == '?'));
        self.send(move || {
            let lines = match &shown {
                Shown::File { path, .. } if untracked => untracked_diff(&root, path),
                Shown::File { path, staged } => {
                    let mut args = vec!["diff", "--no-color"];
                    if *staged {
                        args.push("--cached");
                    }
                    args.extend(["--", path.as_str()]);
                    match git(&root, &args) {
                        Ok(d) if d.trim().is_empty() => vec!["(no changes)".to_owned()],
                        Ok(d) => lines_of(&d),
                        Err(e) => vec![e],
                    }
                }
                Shown::Commit(hash) => match git(&root, &["show", "--no-color", "--stat", "--patch", "--format=commit %H%nAuthor: %an <%ae>%nDate:   %ad%n%n%w(0,4,4)%B", hash]) {
                    Ok(d) => lines_of(&d),
                    Err(e) => vec![e],
                },
            };
            Reply::Diff(shown, lines)
        });
    }

    fn take_replies(&mut self) {
        while let Ok(reply) = self.replies.try_recv() {
            self.running = self.running.saturating_sub(1);
            match reply {
                Reply::Root(Ok(root)) => {
                    self.root = Some(root);
                    self.no_repo = None;
                    self.refresh();
                }
                Reply::Root(Err(e)) => {
                    self.root = None;
                    self.no_repo = Some(if e.contains("not a git repository") { "This folder is not in a git repository.".to_owned() } else { e });
                }
                Reply::Status(Ok(st)) => {
                    // The selected file is no longer changed (e.g. after staging all of it).
                    if let Some(Shown::File { path, staged }) = &self.shown {
                        let still = st.entries.iter().any(|e| e.path == *path && if *staged { e.staged() } else { e.unstaged() });
                        if !still {
                            let other = st.entries.iter().find(|e| e.path == *path).map(|e| Shown::File { path: e.path.clone(), staged: e.staged() });
                            match other {
                                Some(s) => self.show(s),
                                None => {
                                    self.shown = None;
                                    self.diff.clear();
                                }
                            }
                        }
                    }
                    self.status = st;
                }
                Reply::Status(Err(e)) => self.output = Some((false, e)),
                Reply::Branches(b) => self.branches = b,
                Reply::Log(Ok(log)) => self.log = log,
                Reply::Log(Err(_)) => self.log.clear(),
                Reply::Diff(shown, lines) => {
                    if self.shown.as_ref() == Some(&shown) {
                        self.diff = lines;
                    }
                }
                Reply::Ran { what, result } => {
                    self.output = Some(match result {
                        Ok(out) => {
                            let out = out.trim();
                            (true, if out.is_empty() { format!("{what}: done") } else { format!("{what}: {out}") })
                        }
                        Err(e) => (false, format!("{what} failed: {e}")),
                    });
                    self.refresh();
                }
            }
        }
    }

    /// Draws the tab. Returns what the app should do (open a file).
    pub fn ui(&mut self, ui: &mut Ui) -> Option<Action> {
        self.take_replies();
        // Notice changes made elsewhere (in the editor, a terminal): read the status again now
        // and then while the tab is shown.
        if self.root.is_some() && self.running == 0 && self.last_status.elapsed() > Duration::from_secs(3) {
            self.refresh();
        }
        if self.running > 0 {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        } else {
            ui.ctx().request_repaint_after(Duration::from_secs(3));
        }
        let Some(root) = self.root.clone() else {
            ui.add_space(12.0);
            match &self.no_repo {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Looking for the git repository...");
                    });
                }
                Some(why) => {
                    ui.label(why.as_str());
                    ui.weak(self.dir.display().to_string());
                    ui.add_space(8.0);
                    if ui.button("Initialize Repository").clicked() {
                        let dir = self.dir.clone();
                        self.no_repo = None;
                        self.send(move || match git(&dir, &["init"]) {
                            Ok(_) => Reply::Root(git(&dir, &["rev-parse", "--show-toplevel"]).map(|s| PathBuf::from(s.trim()))),
                            Err(e) => Reply::Root(Err(e)),
                        });
                    }
                }
            }
            return None;
        };
        self.top_bar(ui);
        if let Some((ok, text)) = &self.output {
            let color = if *ok { ui.visuals().weak_text_color() } else { ui.visuals().error_fg_color };
            let mut close = false;
            ui.horizontal(|ui| {
                close = ui.small_button("\u{d7}").on_hover_text("Hide").clicked();
                let first = text.lines().next().unwrap_or("");
                ui.add(egui::Label::new(RichText::new(first).color(color)).truncate()).on_hover_text(text.as_str());
            });
            if close {
                self.output = None;
            }
        }
        ui.separator();
        let mut action = None;
        egui::Panel::left("git_left").resizable(true).default_size(340.0).size_range(220.0..=700.0).show(ui, |ui| match self.view {
            View::Changes => action = self.changes_list(ui, &root),
            View::History => self.history_list(ui),
        });
        egui::CentralPanel::default().show(ui, |ui| self.diff_view(ui));
        action
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("\u{2387}");
            let current = self.status.branch.clone();
            let mut switch_to = None;
            egui::ComboBox::from_id_salt("git_branch").selected_text(&current).width(180.0).show_ui(ui, |ui| {
                for b in &self.branches {
                    if ui.selectable_label(*b == current, b).clicked() && *b != current {
                        switch_to = Some(b.clone());
                    }
                }
            });
            if let Some(b) = switch_to {
                self.run(&format!("Switch to {b}"), vec!["switch".into(), b], None);
            }
            match &self.status.upstream {
                Some(up) => {
                    ui.label(format!("\u{2191}{} \u{2193}{}", self.status.ahead, self.status.behind))
                        .on_hover_text(format!("{} commit(s) to push, {} to pull ({up})", self.status.ahead, self.status.behind));
                }
                None => {
                    ui.weak("no upstream");
                }
            }
            ui.separator();
            if ui.button("Fetch").on_hover_text("git fetch --all --prune").clicked() {
                self.run("Fetch", vec!["fetch".into(), "--all".into(), "--prune".into()], None);
            }
            if ui.button("Pull").on_hover_text("git pull").clicked() {
                self.run("Pull", vec!["pull".into()], None);
            }
            let push = if self.status.upstream.is_none() && !self.status.initial {
                vec!["push".into(), "-u".into(), "origin".into(), "HEAD".into()]
            } else {
                vec!["push".into()]
            };
            let tip = if push.len() > 1 { "git push -u origin HEAD (sets the upstream)" } else { "git push" };
            if ui.button("Push").on_hover_text(tip).clicked() {
                self.run("Push", push, None);
            }
            if ui.button("\u{21bb}").on_hover_text("Refresh").clicked() {
                self.refresh();
            }
            if self.running > 0 {
                ui.spinner();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.selectable_value(&mut self.view, View::History, "History");
                ui.selectable_value(&mut self.view, View::Changes, "Changes");
            });
        });
    }

    /// The commit box and the staged and unstaged files.
    fn changes_list(&mut self, ui: &mut Ui, root: &Path) -> Option<Action> {
        let mut action = None;
        let staged_count = self.status.entries.iter().filter(|e| e.staged()).count();
        let resp = ui.add(
            egui::TextEdit::multiline(&mut self.message)
                .hint_text("Message (Ctrl+Enter to commit)")
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        let ctrl_enter = resp.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
        let can_commit = (staged_count > 0 || self.amend) && (!self.message.trim().is_empty() || self.amend);
        let mut commit = false;
        ui.horizontal(|ui| {
            let tip = if staged_count == 0 && !self.amend {
                "Stage changes to commit them (+)"
            } else if self.message.trim().is_empty() && !self.amend {
                "Write a commit message"
            } else {
                "Commit the staged changes (Ctrl+Enter)"
            };
            let label = if self.amend { "\u{2714} Commit (Amend)" } else { "\u{2714} Commit" };
            commit = ui.add_enabled(can_commit, egui::Button::new(label)).on_hover_text(tip).on_disabled_hover_text(tip).clicked();
            ui.checkbox(&mut self.amend, "Amend").on_hover_text("Replace the last commit (with the staged changes)");
        });
        if (commit || (ctrl_enter && can_commit)) && self.root.is_some() {
            let mut args = vec!["commit".to_owned()];
            if self.amend {
                args.push("--amend".into());
            }
            let input = if self.message.trim().is_empty() {
                // Amend keeping the message.
                args.push("--no-edit".into());
                None
            } else {
                args.extend(["-F".into(), "-".into()]);
                Some(self.message.clone())
            };
            self.run("Commit", args, input);
            self.message.clear();
            self.amend = false;
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let entries = self.status.entries.clone();
            let conflicts: Vec<&Entry> = entries.iter().filter(|e| e.conflicted()).collect();
            let staged: Vec<&Entry> = entries.iter().filter(|e| e.staged() && !e.conflicted()).collect();
            let changed: Vec<&Entry> = entries.iter().filter(|e| e.unstaged() && !e.conflicted()).collect();
            if !conflicts.is_empty() {
                self.section(ui, "Merge Changes", &conflicts, false, root, &mut action);
            }
            self.section(ui, "Staged Changes", &staged, true, root, &mut action);
            self.section(ui, "Changes", &changed, false, root, &mut action);
            if entries.is_empty() {
                ui.add_space(8.0);
                ui.weak("No changes.");
            }
        });
        action
    }

    /// A list of files with a header ("Changes  [stage all]") and per-file buttons.
    fn section(&mut self, ui: &mut Ui, title: &str, entries: &[&Entry], staged: bool, root: &Path, action: &mut Option<Action>) {
        if entries.is_empty() && !staged {
            return;
        }
        ui.horizontal(|ui| {
            ui.strong(format!("{title}  {}", entries.len()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if entries.is_empty() {
                    return;
                }
                if staged {
                    if ui.small_button("\u{2212}").on_hover_text("Unstage all").clicked() {
                        self.unstage_all();
                    }
                } else {
                    if ui.small_button("+").on_hover_text("Stage all").clicked() {
                        let paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
                        self.stage(paths);
                    }
                    if ui.small_button("\u{21b6}").on_hover_text("Discard all these changes").clicked() {
                        let all: Vec<Entry> = entries.iter().map(|e| (*e).clone()).collect();
                        self.discard(&all, root);
                    }
                }
            });
        });
        for e in entries {
            let selected = self.shown == Some(Shown::File { path: e.path.clone(), staged });
            let letter = if staged { e.index } else { e.worktree };
            let (letter, color) = match letter {
                'M' => ('M', Color32::from_rgb(214, 170, 60)),
                'A' => ('A', Color32::from_rgb(87, 171, 90)),
                '?' => ('U', Color32::from_rgb(87, 171, 90)),
                'D' => ('D', Color32::from_rgb(220, 90, 80)),
                'R' => ('R', Color32::from_rgb(90, 150, 220)),
                'C' => ('C', Color32::from_rgb(90, 150, 220)),
                'U' => ('!', Color32::from_rgb(220, 90, 80)),
                c => (c, ui.visuals().text_color()),
            };
            let (dir, name) = match e.path.rsplit_once('/') {
                Some((d, n)) => (d, n),
                None => ("", e.path.as_str()),
            };
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(letter.to_string()).monospace().color(color)).on_hover_text(match letter {
                        'M' => "Modified",
                        'A' => "Added",
                        'U' => "Untracked",
                        'D' => "Deleted",
                        'R' => "Renamed",
                        'C' => "Copied",
                        '!' => "Conflict",
                        _ => "",
                    });
                    if staged {
                        if ui.small_button("\u{2212}").on_hover_text("Unstage").clicked() {
                            self.unstage(vec![e.path.clone()]);
                        }
                    } else {
                        if ui.small_button("+").on_hover_text("Stage").clicked() {
                            self.stage(vec![e.path.clone()]);
                        }
                        if !e.conflicted() && ui.small_button("\u{21b6}").on_hover_text("Discard changes").clicked() {
                            self.discard(&[(*e).clone()], root);
                        }
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let r = ui.selectable_label(selected, name);
                        let tip = match &e.orig {
                            Some(o) => format!("{} (was {o})\nDouble-click to open", e.path),
                            None => format!("{}\nDouble-click to open", e.path),
                        };
                        let r = r.on_hover_text(tip);
                        if r.clicked() {
                            self.show(Shown::File { path: e.path.clone(), staged });
                        }
                        if r.double_clicked() && e.worktree != 'D' {
                            *action = Some(Action::Open(root.join(&e.path)));
                        }
                        if !dir.is_empty() {
                            ui.add(egui::Label::new(RichText::new(dir).weak().small()).truncate());
                        }
                    });
                });
            });
        }
        ui.add_space(6.0);
    }

    fn stage(&mut self, paths: Vec<String>) {
        let mut args = vec!["add".to_owned(), "-A".to_owned(), "--".to_owned()];
        args.extend(paths);
        self.run("Stage", args, None);
    }

    fn unstage(&mut self, paths: Vec<String>) {
        // Without a commit there is nothing to restore from: take them out of the index.
        let mut args: Vec<String> = if self.status.initial {
            vec!["rm".into(), "--cached".into(), "-r".into(), "-q".into(), "--".into()]
        } else {
            vec!["restore".into(), "--staged".into(), "--".into()]
        };
        args.extend(paths);
        self.run("Unstage", args, None);
    }

    fn unstage_all(&mut self) {
        let paths = self.status.entries.iter().filter(|e| e.staged()).map(|e| e.path.clone()).collect();
        self.unstage(paths);
    }

    /// Throws away working-tree changes (asks first): tracked files are restored, new files
    /// deleted.
    fn discard(&mut self, entries: &[Entry], root: &Path) {
        let what = match entries {
            [one] => format!("the changes in {}", one.path),
            many => format!("the changes in {} files", many.len()),
        };
        let sure = rfd::MessageDialog::new()
            .set_title("Discard changes")
            .set_description(format!("Discard {what}? This cannot be undone."))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes;
        if !sure {
            return;
        }
        let (untracked, tracked): (Vec<&Entry>, Vec<&Entry>) = entries.iter().partition(|e| e.index == '?');
        for e in untracked {
            let _ = std::fs::remove_file(root.join(&e.path));
        }
        if tracked.is_empty() {
            self.refresh();
        } else {
            let mut args = vec!["restore".to_owned(), "--worktree".to_owned(), "--".to_owned()];
            args.extend(tracked.iter().map(|e| e.path.clone()));
            self.run("Discard", args, None);
        }
    }

    fn history_list(&mut self, ui: &mut Ui) {
        if self.log.is_empty() {
            ui.weak(if self.status.initial { "No commits yet." } else { "Reading the history..." });
            return;
        }
        let log = self.log.clone();
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, 40.0, log.len(), |ui, range| {
            for c in &log[range] {
                let selected = self.shown == Some(Shown::Commit(c.hash.clone()));
                let r = ui
                    .allocate_ui(egui::vec2(ui.available_width(), 40.0), |ui| {
                        ui.set_min_size(egui::vec2(ui.available_width(), 40.0));
                        let r = ui.selectable_label(selected, RichText::new(&c.subject).strong());
                        ui.horizontal(|ui| {
                            ui.weak(RichText::new(&c.short).monospace().small());
                            ui.weak(RichText::new(format!("{}, {}", c.author, c.when)).small());
                            if !c.refs.is_empty() {
                                ui.label(RichText::new(&c.refs).small().color(Color32::from_rgb(90, 150, 220)));
                            }
                        });
                        r
                    })
                    .inner;
                if r.on_hover_text(&c.hash).clicked() {
                    self.show(Shown::Commit(c.hash.clone()));
                }
            }
        });
    }

    fn diff_view(&mut self, ui: &mut Ui) {
        let Some(shown) = &self.shown else {
            ui.add_space(12.0);
            ui.weak(match self.view {
                View::Changes => "Select a file to see its changes.",
                View::History => "Select a commit to see it.",
            });
            return;
        };
        let title = match shown {
            Shown::File { path, staged: true } => format!("{path} (staged)"),
            Shown::File { path, staged: false } => path.clone(),
            Shown::Commit(hash) => format!("commit {}", &hash[..hash.len().min(10)]),
        };
        ui.strong(title);
        ui.separator();
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let row = ui.fonts_mut(|f| f.row_height(&font));
        let text = ui.visuals().text_color();
        let weak = ui.visuals().weak_text_color();
        let (add, del, hunk) = if ui.visuals().dark_mode {
            (Color32::from_rgb(110, 200, 110), Color32::from_rgb(235, 110, 100), Color32::from_rgb(110, 170, 235))
        } else {
            (Color32::from_rgb(30, 130, 40), Color32::from_rgb(190, 40, 40), Color32::from_rgb(30, 90, 180))
        };
        let lines = &self.diff;
        egui::ScrollArea::both().id_salt("git_diff").auto_shrink(false).show_rows(ui, row, lines.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for line in &lines[range] {
                let color = if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") || line.starts_with("index ") {
                    weak
                } else if line.starts_with('+') {
                    add
                } else if line.starts_with('-') {
                    del
                } else if line.starts_with("@@") {
                    hunk
                } else {
                    text
                };
                ui.add(egui::Label::new(RichText::new(line.replace('\t', "    ")).font(font.clone()).color(color)).wrap_mode(egui::TextWrapMode::Extend));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_parsing() {
        let out = "# branch.oid 1234\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 abc abc src/a b.rs\0\
1 A. N... 000000 100644 100644 000 def new.txt\0\
2 R. N... 100644 100644 100644 abc abc R100 src/new.rs\0src/old.rs\0\
u UU N... 100644 100644 100644 100644 a b c conflict.c\0\
? notes.md\0";
        let st = parse_status(out);
        assert_eq!((st.branch.as_str(), st.upstream.as_deref(), st.ahead, st.behind, st.initial), ("main", Some("origin/main"), 2, 1, false));
        let summary: Vec<_> = st.entries.iter().map(|e| (e.path.as_str(), e.index, e.worktree, e.orig.as_deref())).collect();
        assert_eq!(
            summary,
            [
                ("conflict.c", 'U', 'U', None),
                ("new.txt", 'A', '.', None),
                ("notes.md", '?', '?', None),
                ("src/a b.rs", '.', 'M', None),
                ("src/new.rs", 'R', '.', Some("src/old.rs")),
            ]
        );
        assert!(st.entries[0].conflicted());
        assert!(st.entries[1].staged() && !st.entries[1].unstaged());
        assert!(!st.entries[2].staged() && st.entries[2].unstaged());
        assert!(parse_status("# branch.oid (initial)\0# branch.head master\0").initial);
    }

    #[test]
    fn log_parsing() {
        let out = "aaaa\x1faa\x1fAnn\x1f2 days ago\x1fHEAD -> main, origin/main\x1fFirst: subject\x1e\nbbbb\x1fbb\x1fBob\x1f3 days ago\x1f\x1fSecond\x1e\n";
        let log = parse_log(out);
        assert_eq!(log.len(), 2);
        assert_eq!((log[0].short.as_str(), log[0].refs.as_str(), log[0].subject.as_str()), ("aa", "HEAD -> main, origin/main", "First: subject"));
        assert_eq!((log[1].author.as_str(), log[1].refs.as_str()), ("Bob", ""));
    }

    /// Stage, commit and read the history in a real repository (needs git).
    #[test]
    fn works_with_a_real_repository() {
        if git(Path::new("."), &["--version"]).is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("alien_git_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]).unwrap();
        git(&dir, &["config", "user.email", "test@example.com"]).unwrap();
        git(&dir, &["config", "user.name", "Test"]).unwrap();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        let st = parse_status(&git(&dir, &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all"]).unwrap());
        assert!(st.initial);
        assert_eq!(st.entries[0].index, '?');
        git(&dir, &["add", "-A", "--", "a.txt"]).unwrap();
        git_input(&dir, &["commit", "-F", "-"], Some("First commit\n\nBody")).unwrap();
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        let st = parse_status(&git(&dir, &["status", "--porcelain=v2", "--branch", "-z"]).unwrap());
        assert!(!st.initial);
        assert_eq!((st.entries[0].index, st.entries[0].worktree), ('.', 'M'));
        let log = parse_log(&git(&dir, &["log", "-n", "300", LOG_FORMAT]).unwrap());
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].subject, "First commit");
        let diff = git(&dir, &["diff", "--no-color", "--", "a.txt"]).unwrap();
        assert!(diff.lines().any(|l| l == "+two"));
        assert!(git(&dir, &["no-such-command"]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
