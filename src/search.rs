//! Searching for files by name in the explorer's folder, as in VS Code's search panel.
//!
//! The pattern rules (as in the "Everything" file finder):
//! - without wildcards, the text is found anywhere in the name: `main` finds `main.rs` and
//!   `domain.rs`;
//! - with wildcards, the pattern must match the whole name: `*` is any run of characters (also
//!   none), `?` is exactly one. `abc*xyz.cpp` finds `abc_1_xyz.cpp` but not `abc_xyz.cpp.bak`;
//! - a pattern with `/` (or `\`) is matched against the path inside the folder instead of the
//!   name: `src/*.rs`.
//!
//! Options: case sensitivity (off by default), and whole words, where a match may not start or
//! end inside a word (letters, digits and `_`). In a wildcard pattern, the text on each side of a
//! `*` must also be whole words: `abc*.cpp` finds `abc-old.cpp` but not `abcd.cpp`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

/// At most this many files are listed.
pub const MAX_RESULTS: usize = 10_000;

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

/// A compiled search pattern.
#[derive(Clone, Debug)]
pub struct Matcher {
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
    /// `None` for an empty pattern (nothing to search for).
    pub fn new(pattern: &str, opts: SearchOptions) -> Option<Matcher> {
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
        Some(Matcher { toks, case_sensitive: opts.case_sensitive, on_path })
    }

    /// Whether the file matches: `name` is its name, `rel_path` its path inside the folder with
    /// `/` between the parts.
    pub fn is_match(&self, name: &str, rel_path: &str) -> bool {
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

/// One file found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    pub name: String,
    /// The folder it is in, relative to the searched folder ("" for the folder itself).
    pub dir: String,
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
#[derive(Default)]
pub struct FileSearch {
    pub query: String,
    pub opts: SearchOptions,
    pub results: Vec<Hit>,
    /// The search has finished (false while it runs).
    pub done: bool,
    /// More than `MAX_RESULTS` files matched; only the first ones are listed.
    pub truncated: bool,
    /// The query, options and folder of the current results (a change starts a new search).
    last: Option<(String, SearchOptions, PathBuf)>,
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

    /// Call every frame: starts a new search when the query, options or folder changed, and
    /// takes the files found since the last call. `repaint` is called from the search thread
    /// when it has found more.
    pub fn update(&mut self, root: Option<&Path>, repaint: impl Fn() + Send + 'static) {
        let key = root.map(|r| (self.query.clone(), self.opts, r.to_path_buf()));
        if key != self.last {
            self.job = None;
            self.results.clear();
            self.truncated = false;
            self.done = false;
            let matcher = Matcher::new(&self.query, self.opts);
            match (root, matcher) {
                (Some(root), Some(m)) => self.job = Some(start(root.to_path_buf(), m, repaint)),
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

fn start(root: PathBuf, matcher: Matcher, repaint: impl Fn() + Send + 'static) -> Job {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    std::thread::spawn(move || {
        let truncated = walk(&root, &matcher, &stop, &tx, &repaint);
        let _ = tx.send(Msg::Done { truncated });
        repaint();
    });
    Job { rx, cancel }
}

/// Goes through every file under `root` (not into `.git`, nor through links to folders) and
/// sends the matching ones in batches. Returns whether it stopped at `MAX_RESULTS`.
fn walk(root: &Path, matcher: &Matcher, stop: &AtomicBool, tx: &Sender<Msg>, repaint: &dyn Fn()) -> bool {
    let mut found = 0;
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
            } else if matcher.is_match(&name, &rel_path) {
                if found == MAX_RESULTS {
                    let _ = tx.send(Msg::Found(batch));
                    return true;
                }
                found += 1;
                batch.push(Hit { path: d.path(), name, dir: rel.clone() });
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
