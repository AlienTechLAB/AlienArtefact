//! Searching for files by name in the explorer's folder, as in VS Code's search panel.
//!
//! The pattern rules (as in the "Everything" file finder):
//! - without wildcards, the text is found anywhere in the name: `main` finds `main.rs` and
//!   `domain.rs`;
//! - with wildcards, the pattern must match the whole name: `*` is any run of characters (also
//!   none), `?` is exactly one. `abc*xyz.cpp` finds `abc_1_xyz.cpp` but not `abc_xyz.cpp.bak`;
//! - a pattern with `/` (or `\`) is matched against the path inside the folder instead of the
//!   name: `src/*.rs`;
//! - several patterns separated by commas (or semicolons) find files matching any of them:
//!   `*.cpp, *.h, *.py`.
//!
//! Text in files: with text to find, the files that match the name patterns (all files, also
//! binary ones, when there are none) are read and the lines containing the text are listed.
//! The text is found as it is typed (no wildcards). Binary files only report how many times.
//!
//! Options (separate for the names and the text): case sensitivity (off by default), and whole words, where a match may not start or
//! end inside a word (letters, digits and `_`). In a wildcard pattern, the text on each side of a
//! `*` must also be whole words: `abc*.cpp` finds `abc-old.cpp` but not `abcd.cpp`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

/// At most this many files are listed.
pub const MAX_RESULTS: usize = 10_000;
/// At most this many matching lines are listed (in all files together).
pub const MAX_LINES: usize = 20_000;
/// Bigger files are not searched for text.
const MAX_FILE_SIZE: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    Char(char),
    /// `?`: one character.
    One,
    /// `*`: any run of characters.
    Any,
    /// Zero width: not inside a word (whole words option).
    Edge,
}

/// A compiled search: one or more patterns, a file matches if it matches any.
#[derive(Clone, Debug)]
pub struct Matcher {
    patterns: Vec<Pattern>,
}

/// One pattern of a search.
#[derive(Clone, Debug)]
struct Pattern {
    toks: Vec<Tok>,
    case_sensitive: bool,
    /// Matched against the path inside the folder instead of the name.
    on_path: bool,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn fold(c: char, case_sensitive: bool) -> char {
    if case_sensitive { c } else { c.to_lowercase().next().unwrap_or(c) }
}

impl Matcher {
    /// The patterns in `query`, separated by `,` or `;` (spaces around them are ignored). `None`
    /// when there is none (nothing to search for).
    pub fn new(query: &str, opts: SearchOptions) -> Option<Matcher> {
        let patterns: Vec<Pattern> = query.split([',', ';']).filter_map(|p| Pattern::new(p, opts)).collect();
        (!patterns.is_empty()).then_some(Matcher { patterns })
    }

    /// Whether the file matches any of the patterns: `name` is its name, `rel_path` its path
    /// inside the folder with `/` between the parts.
    pub fn is_match(&self, name: &str, rel_path: &str) -> bool {
        self.patterns.iter().any(|p| p.is_match(name, rel_path))
    }
}

impl Pattern {
    fn new(pattern: &str, opts: SearchOptions) -> Option<Pattern> {
        let pattern = pattern.trim().replace('\\', "/");
        if pattern.is_empty() {
            return None;
        }
        let on_path = pattern.contains('/');
        let wild = pattern.contains(['*', '?']);
        let ww = opts.whole_word;
        let mut toks = Vec::new();
        let edge = |toks: &mut Vec<Tok>| {
            if ww && toks.last() != Some(&Tok::Edge) {
                toks.push(Tok::Edge);
            }
        };
        if !wild {
            // Anywhere in the name.
            toks.push(Tok::Any);
        }
        edge(&mut toks);
        for c in pattern.chars() {
            match c {
                '*' => {
                    edge(&mut toks);
                    // `**` is the same as `*`.
                    if toks.last() != Some(&Tok::Any) {
                        toks.push(Tok::Any);
                    }
                    edge(&mut toks);
                }
                '?' => toks.push(Tok::One),
                c => toks.push(Tok::Char(fold(c, opts.case_sensitive))),
            }
        }
        edge(&mut toks);
        if !wild {
            toks.push(Tok::Any);
        }
        Some(Pattern { toks, case_sensitive: opts.case_sensitive, on_path })
    }

    fn is_match(&self, name: &str, rel_path: &str) -> bool {
        let subject = if self.on_path { rel_path } else { name };
        let text: Vec<char> = subject.chars().map(|c| fold(c, self.case_sensitive)).collect();
        let (n, m) = (self.toks.len(), text.len());
        let edge_ok = |j: usize| !(j > 0 && j < m && is_word(text[j - 1]) && is_word(text[j]));
        // ok[j] after step i: tokens i.. match text j.. (computed from the last token back).
        let mut next = vec![false; m + 1];
        next[m] = true;
        let mut cur = vec![false; m + 1];
        for i in (0..n).rev() {
            for j in (0..=m).rev() {
                cur[j] = match self.toks[i] {
                    Tok::Char(c) => j < m && text[j] == c && next[j + 1],
                    Tok::One => j < m && next[j + 1],
                    Tok::Any => next[j] || (j < m && cur[j + 1]),
                    Tok::Edge => edge_ok(j) && next[j],
                };
            }
            std::mem::swap(&mut cur, &mut next);
        }
        next[0]
    }
}

/// Text to find in files.
#[derive(Clone, Debug)]
pub struct TextMatcher {
    pat: Vec<char>,
    case_sensitive: bool,
    whole_word: bool,
}

impl TextMatcher {
    /// `None` for empty text.
    pub fn new(text: &str, opts: SearchOptions) -> Option<TextMatcher> {
        if text.is_empty() {
            return None;
        }
        let pat = text.chars().map(|c| fold(c, opts.case_sensitive)).collect();
        Some(TextMatcher { pat, case_sensitive: opts.case_sensitive, whole_word: opts.whole_word })
    }

    fn fold_str(&self, s: &str) -> String {
        s.chars().map(|c| fold(c, self.case_sensitive)).collect()
    }

    /// Whether `content` may contain the text (a quick check before looking line by line).
    fn may_contain(&self, content: &str) -> bool {
        let pat: String = self.pat.iter().collect();
        if self.case_sensitive { content.contains(&pat) } else { self.fold_str(content).contains(&pat) }
    }

    /// Where the text is in `line`: character ranges (start, end), not overlapping.
    pub fn find_in_line(&self, line: &str) -> Vec<(usize, usize)> {
        let text: Vec<char> = line.chars().map(|c| fold(c, self.case_sensitive)).collect();
        let (m, k) = (text.len(), self.pat.len());
        let edge_ok = |j: usize| !(j > 0 && j < m && is_word(text[j - 1]) && is_word(text[j]));
        let mut found = Vec::new();
        let mut j = 0;
        while j + k <= m {
            if text[j..j + k] == self.pat[..] && (!self.whole_word || (edge_ok(j) && edge_ok(j + k))) {
                found.push((j, j + k));
                j += k;
            } else {
                j += 1;
            }
        }
        found
    }
}

/// A line of a file containing the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineMatch {
    /// Line number, from 1.
    pub line: usize,
    /// Where the text is in the line: character ranges.
    pub cols: Vec<(usize, usize)>,
    /// The part of the line shown in the list (without the indentation, and from a little
    /// before the first match in a long line).
    pub preview: String,
    /// The character of the line where `preview` starts.
    pub preview_start: usize,
}

const PREVIEW_CHARS: usize = 200;

impl LineMatch {
    pub fn new(line: usize, text: &str, cols: Vec<(usize, usize)>) -> LineMatch {
        let chars: Vec<char> = text.chars().collect();
        let mut start = chars.iter().take_while(|c| c.is_whitespace()).count();
        if let Some(&(first, _)) = cols.first() {
            if first > start + 60 {
                start = first - 30;
            }
        }
        let preview = chars[start.min(chars.len())..].iter().take(PREVIEW_CHARS).collect();
        LineMatch { line, cols, preview, preview_start: start }
    }

    /// The matches as character ranges of `preview` (those that show in it).
    pub fn preview_ranges(&self) -> Vec<(usize, usize)> {
        let len = self.preview.chars().count();
        self.cols
            .iter()
            .filter_map(|&(a, b)| {
                let a = a.saturating_sub(self.preview_start);
                let b = b.saturating_sub(self.preview_start).min(len);
                (a < b).then_some((a, b))
            })
            .collect()
    }
}

/// One file found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    pub name: String,
    /// The folder it is in, relative to the searched folder ("" for the folder itself).
    pub dir: String,
    /// The lines containing the text (empty when not searching for text, or in a binary file).
    pub lines: Vec<LineMatch>,
    /// How many times the text is in the file when it is binary.
    pub binary_matches: usize,
}

impl Hit {
    /// How many times the text is in the file.
    pub fn match_count(&self) -> usize {
        self.lines.iter().map(|l| l.cols.len()).sum::<usize>() + self.binary_matches
    }
}

/// The text's places in the file: its lines, or (for a binary file) only how many. `None` when
/// the text is not there or the file cannot be read.
fn search_file(path: &Path, tm: &TextMatcher) -> Option<(Vec<LineMatch>, usize)> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE_SIZE {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let binary = bytes[..bytes.len().min(8192)].contains(&0) || std::str::from_utf8(&bytes).is_err();
    let content = String::from_utf8_lossy(&bytes);
    if !tm.may_contain(&content) {
        return None;
    }
    let mut lines = Vec::new();
    let mut count = 0;
    for (i, line) in content.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let cols = tm.find_in_line(line);
        if cols.is_empty() {
            continue;
        }
        if binary {
            count += cols.len();
        } else {
            lines.push(LineMatch::new(i + 1, line, cols));
        }
    }
    (!lines.is_empty() || count > 0).then_some((lines, count))
}

enum Msg {
    Found(Vec<Hit>),
    Done { truncated: bool },
}

/// A search running on its own thread; dropping it stops the thread.
struct Job {
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The search panel's state: what is searched for, and the files found so far.
type SearchKey = (String, SearchOptions, String, SearchOptions, PathBuf);

#[derive(Default)]
pub struct FileSearch {
    /// File name patterns (empty: every file, when searching for text).
    pub query: String,
    pub opts: SearchOptions,
    /// Text to find in the files (empty: list the files by name only).
    pub text: String,
    pub text_opts: SearchOptions,
    pub results: Vec<Hit>,
    /// The search has finished (false while it runs).
    pub done: bool,
    /// More than `MAX_RESULTS` files (or `MAX_LINES` lines) matched; only the first ones are listed.
    pub truncated: bool,
    /// The query, text, options and folder of the current results (a change starts a new search).
    last: Option<SearchKey>,
    job: Option<Job>,
}

impl FileSearch {
    /// Searches again (after files were added, removed or renamed).
    pub fn rerun(&mut self) {
        self.last = None;
    }

    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    /// Whether text is searched for (results have lines), not only names.
    pub fn searching_text(&self) -> bool {
        !self.text.is_empty()
    }

    /// Call every frame: starts a new search when the query, text, options or folder changed, and
    /// takes the files found since the last call. `repaint` is called from the search thread
    /// when it has found more.
    pub fn update(&mut self, root: Option<&Path>, repaint: impl Fn() + Send + 'static) {
        let key = root.map(|r| (self.query.clone(), self.opts, self.text.clone(), self.text_opts, r.to_path_buf()));
        if key != self.last {
            self.job = None;
            self.results.clear();
            self.truncated = false;
            self.done = false;
            let names = Matcher::new(&self.query, self.opts);
            let text = TextMatcher::new(&self.text, self.text_opts);
            match root {
                Some(root) if names.is_some() || text.is_some() => {
                    self.job = Some(start(root.to_path_buf(), names, text, repaint));
                }
                _ => self.done = true,
            }
            self.last = key;
        }
        let Some(job) = &self.job else { return };
        let mut finished = false;
        loop {
            match job.rx.try_recv() {
                Ok(Msg::Found(hits)) => self.results.extend(hits),
                Ok(Msg::Done { truncated }) => {
                    self.truncated = truncated;
                    finished = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        if finished {
            self.job = None;
            self.done = true;
            // Sorted by folder, then name (the walk finds them in no useful order).
            self.results.sort_by_cached_key(|h| (h.dir.to_lowercase(), h.name.to_lowercase()));
        }
    }
}

fn start(root: PathBuf, names: Option<Matcher>, text: Option<TextMatcher>, repaint: impl Fn() + Send + 'static) -> Job {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    std::thread::spawn(move || {
        let truncated = walk(&root, names.as_ref(), text.as_ref(), &stop, &tx, &repaint);
        let _ = tx.send(Msg::Done { truncated });
        repaint();
    });
    Job { rx, cancel }
}

/// Goes through every file under `root` (not into `.git`, nor through links to folders) and
/// sends the matching ones in batches: those whose name matches `names` (any name when `None`)
/// and that contain `text` (when given). Returns whether it stopped at `MAX_RESULTS` files or
/// `MAX_LINES` lines.
fn walk(
    root: &Path,
    names: Option<&Matcher>,
    text: Option<&TextMatcher>,
    stop: &AtomicBool,
    tx: &Sender<Msg>,
    repaint: &dyn Fn(),
) -> bool {
    let mut found = 0;
    let mut found_lines = 0;
    let mut batch = Vec::new();
    let mut last_send = std::time::Instant::now();
    // (folder, its path inside the root with `/` between the parts)
    let mut dirs = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = dirs.pop() {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for d in read.filter_map(Result::ok) {
            let name = d.file_name().to_string_lossy().into_owned();
            let Ok(ft) = d.file_type() else { continue };
            let rel_path = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if ft.is_dir() {
                if name != ".git" {
                    dirs.push((d.path(), rel_path));
                }
            } else if names.is_none_or(|m| m.is_match(&name, &rel_path)) {
                if found == MAX_RESULTS || found_lines >= MAX_LINES {
                    let _ = tx.send(Msg::Found(batch));
                    return true;
                }
                let (lines, binary_matches) = match text {
                    None => (Vec::new(), 0),
                    Some(tm) => {
                        if stop.load(Ordering::Relaxed) {
                            return false;
                        }
                        match search_file(&d.path(), tm) {
                            Some(m) => m,
                            None => continue,
                        }
                    }
                };
                found += 1;
                found_lines += lines.len();
                batch.push(Hit { path: d.path(), name, dir: rel.clone(), lines, binary_matches });
            }
        }
        if !batch.is_empty() && last_send.elapsed().as_millis() > 50 {
            if tx.send(Msg::Found(std::mem::take(&mut batch))).is_err() {
                return false;
            }
            repaint();
            last_send = std::time::Instant::now();
        }
    }
    let _ = tx.send(Msg::Found(batch));
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, case_sensitive: bool, whole_word: bool) -> Matcher {
        Matcher::new(pattern, SearchOptions { case_sensitive, whole_word }).unwrap()
    }

    fn hits(pattern: &str, cs: bool, ww: bool, names: &[&str]) -> Vec<String> {
        let m = m(pattern, cs, ww);
        names.iter().filter(|n| m.is_match(n, n)).map(|n| n.to_string()).collect()
    }

    #[test]
    fn plain_text_is_found_anywhere() {
        let names = ["main.rs", "domain.rs", "Main.cpp", "other.txt"];
        assert_eq!(hits("main", false, false, &names), ["main.rs", "domain.rs", "Main.cpp"]);
        assert_eq!(hits("main", true, false, &names), ["main.rs", "domain.rs"]);
        assert_eq!(hits("Main", true, false, &names), ["Main.cpp"]);
        assert!(Matcher::new("  ", SearchOptions::default()).is_none());
    }

    #[test]
    fn wildcards_match_the_whole_name() {
        let names = ["abc_1_xyz.cpp", "abcxyz.cpp", "abc_xyz.cpp.bak", "ABC-xyz.cpp", "xabc_xyz.cpp", "abc.h"];
        assert_eq!(hits("abc*xyz.cpp", false, false, &names), ["abc_1_xyz.cpp", "abcxyz.cpp", "ABC-xyz.cpp"]);
        assert_eq!(hits("abc*xyz.cpp", true, false, &names), ["abc_1_xyz.cpp", "abcxyz.cpp"]);
        assert_eq!(hits("*.cpp", false, false, &names), ["abc_1_xyz.cpp", "abcxyz.cpp", "ABC-xyz.cpp", "xabc_xyz.cpp"]);
        assert_eq!(hits("abc.?", false, false, &names), ["abc.h"]);
        assert_eq!(hits("a**h", false, false, &names), ["abc.h"]);
    }

    #[test]
    fn whole_words() {
        let names = ["main.rs", "domain.rs", "main_window.rs", "my-main.rs"];
        assert_eq!(hits("main", false, true, &names), ["main.rs", "my-main.rs"]);
        // Text next to a `*` must be whole words too.
        let names = ["abc-old.cpp", "abcd.cpp", "abc.cpp", "abc_old.cpp"];
        assert_eq!(hits("abc*.cpp", false, true, &names), ["abc-old.cpp", "abc.cpp"]);
        assert_eq!(hits("abc*.cpp", false, false, &names), names);
        let names = ["abc-1-xyz.cpp", "abc_1_xyz.cpp", "abcxyz.cpp"];
        assert_eq!(hits("abc*xyz.cpp", false, true, &names), ["abc-1-xyz.cpp"]);
    }

    #[test]
    fn several_patterns_separated_by_commas() {
        let names = ["a.cpp", "b.h", "c.py", "d.rs", "e.cpp.bak"];
        assert_eq!(hits("*.cpp,*.h,*.py", false, false, &names), ["a.cpp", "b.h", "c.py"]);
        assert_eq!(hits(" *.cpp ; *.H ,", false, false, &names), ["a.cpp", "b.h"]);
        // Each one follows its own rules: plain text anywhere, wildcards the whole name.
        assert_eq!(hits("*.rs, bak", false, false, &names), ["d.rs", "e.cpp.bak"]);
        assert!(Matcher::new(" , ;", SearchOptions::default()).is_none());
    }

    #[test]
    fn text_in_a_line() {
        let tm = |t: &str, cs, ww| TextMatcher::new(t, SearchOptions { case_sensitive: cs, whole_word: ww }).unwrap();
        assert_eq!(tm("abc", false, false).find_in_line("x ABC abcd abc"), [(2, 5), (6, 9), (11, 14)]);
        assert_eq!(tm("abc", true, false).find_in_line("x ABC abcd abc"), [(6, 9), (11, 14)]);
        assert_eq!(tm("abc", false, true).find_in_line("x ABC abcd abc_1 (abc)"), [(2, 5), (18, 21)]);
        assert_eq!(tm("aa", false, false).find_in_line("aaa"), [(0, 2)], "not overlapping");
        assert!(TextMatcher::new("", SearchOptions::default()).is_none());
        // A long line's preview starts a little before the match.
        let line = format!("    {}abc", "x".repeat(100));
        let lm = LineMatch::new(1, &line, vec![(104, 107)]);
        assert_eq!(lm.preview_start, 74);
        assert_eq!(lm.preview_ranges(), [(30, 33)]);
    }

    #[test]
    fn searches_text_in_files() {
        let root = std::env::temp_dir().join(format!("alien_text_search_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.cpp"), "int abc = 1;\r\n// none\n  return abc + ABC;\n").unwrap();
        std::fs::write(root.join("src/a.h"), "abc").unwrap();
        std::fs::write(root.join("src/b.cpp"), "nothing here").unwrap();
        std::fs::write(root.join("data.bin"), [0u8, b'a', b'b', b'c', 0, b'a', b'b', b'c']).unwrap();
        let run = |query: &str, cs: bool| {
            let mut s = FileSearch { query: query.into(), text: "abc".into(), ..Default::default() };
            s.text_opts.case_sensitive = cs;
            while {
                s.update(Some(&root), || {});
                !s.done
            } {
                std::thread::yield_now();
            }
            s.results
        };
        let r = run("*.cpp", false);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "a.cpp");
        assert_eq!(r[0].lines.iter().map(|l| l.line).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(r[0].lines[1].preview, "return abc + ABC;");
        assert_eq!(r[0].match_count(), 3);
        assert_eq!(run("*.cpp", true)[0].match_count(), 2);
        // No name pattern: every file, binary ones too (only counted).
        let r = run("", false);
        let names: Vec<_> = r.iter().map(|h| (h.name.as_str(), h.match_count(), h.lines.len())).collect();
        assert_eq!(names, [("data.bin", 2, 0), ("a.cpp", 3, 2), ("a.h", 1, 1)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_slash_matches_the_path() {
        let glob = m("src/*.rs", false, false);
        assert!(glob.is_match("main.rs", "src/main.rs"));
        assert!(!glob.is_match("main.rs", "lib/main.rs"));
        // A backslash works as a slash.
        let back = m("deep\\x", false, false);
        assert!(back.is_match("x.toml", "src/deep/x.toml"));
    }

    #[test]
    fn searches_a_folder_on_a_thread() {
        let root = std::env::temp_dir().join(format!("alien_search_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        for f in ["src/main.rs", "src/deep/main_test.rs", "README.md", ".git/main.rs"] {
            std::fs::write(root.join(f), "").unwrap();
        }
        let mut s = FileSearch { query: "main".into(), ..Default::default() };
        let t = std::time::Instant::now();
        while {
            s.update(Some(&root), || {});
            !s.done
        } {
            assert!(t.elapsed().as_secs() < 10, "search did not finish");
            std::thread::yield_now();
        }
        let found: Vec<_> = s.results.iter().map(|h| format!("{}|{}", h.dir, h.name)).collect();
        assert_eq!(found, ["src|main.rs", "src/deep|main_test.rs"]);

        // Changing an option searches again.
        s.opts.whole_word = true;
        while {
            s.update(Some(&root), || {});
            !s.done
        } {
            std::thread::yield_now();
        }
        assert_eq!(s.results.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
