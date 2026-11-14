//! The explorer: a folder on disk shown as a tree in the side panel, as in VS Code.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// One file or folder in a listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// The open folder: which subfolders are expanded, and the listings read so far.
#[derive(Debug)]
pub struct Explorer {
    pub root: PathBuf,
    pub expanded: BTreeSet<PathBuf>,
    listings: HashMap<PathBuf, Vec<Entry>>,
}

impl Explorer {
    pub fn new(root: PathBuf) -> Self {
        let mut expanded = BTreeSet::new();
        expanded.insert(root.clone());
        Explorer { root, expanded, listings: HashMap::new() }
    }

    /// The folder's name, as shown at the top of the tree.
    pub fn root_name(&self) -> String {
        self.root.file_name().map_or_else(|| self.root.display().to_string(), |n| n.to_string_lossy().into_owned())
    }

    /// Reads the folders again (after files were added, removed or renamed).
    pub fn refresh(&mut self) {
        self.listings.clear();
    }

    pub fn toggle(&mut self, dir: &Path) {
        if !self.expanded.remove(dir) {
            self.expanded.insert(dir.to_path_buf());
        }
    }

    /// The contents of `dir`, read once and then kept until `refresh`.
    pub fn entries(&mut self, dir: &Path) -> &[Entry] {
        self.listings.entry(dir.to_path_buf()).or_insert_with(|| list_dir(dir))
    }

    /// Every row of the tree under the root (not the root itself), with its depth, in display
    /// order: the contents of expanded folders follow them.
    pub fn rows(&mut self) -> Vec<(usize, Entry)> {
        let mut rows = Vec::new();
        let root = self.root.clone();
        self.collect(&root, 0, &mut rows);
        rows
    }

    fn collect(&mut self, dir: &Path, depth: usize, rows: &mut Vec<(usize, Entry)>) {
        let entries = self.entries(dir).to_vec();
        for e in entries {
            let open = e.is_dir && self.expanded.contains(&e.path);
            let path = e.path.clone();
            rows.push((depth, e));
            if open {
                self.collect(&path, depth + 1, rows);
            }
        }
    }
}

/// The contents of `dir`: folders first, then files, each sorted by name ignoring case. `.git` is
/// left out, as in VS Code. An unreadable folder is empty.
pub fn list_dir(dir: &Path) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut entries: Vec<Entry> = read
        .filter_map(Result::ok)
        .filter_map(|d| {
            let name = d.file_name().to_string_lossy().into_owned();
            if name == ".git" {
                return None;
            }
            let is_dir = d.file_type().ok()?.is_dir();
            Some(Entry { path: d.path(), name, is_dir })
        })
        .collect();
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    entries
}

/// Whether a file opens as a picture.
pub fn is_image(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg")
}

/// The text of a file to edit, or why it cannot be (too big, or not text).
pub fn read_text(path: &Path) -> Result<String, String> {
    const MAX: u64 = 8 * 1024 * 1024;
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX {
        return Err(format!("it is too big to edit ({} MB)", size / (1024 * 1024)));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|_| "it is not a text file".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tree() -> PathBuf {
        let root = std::env::temp_dir().join(format!("alien_explorer_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("b.txt"), "b").unwrap();
        std::fs::write(root.join("A.md"), "a").unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("src/deep/x.toml"), "x = 1").unwrap();
        std::fs::write(root.join("pic.png"), [0x89, b'P', b'N', b'G', 0xff, 0xfe]).unwrap();
        root
    }

    #[test]
    fn folders_first_sorted_without_git_and_expanding() {
        let root = temp_tree();
        let names = |rows: &[(usize, Entry)]| rows.iter().map(|(d, e)| format!("{d}:{}", e.name)).collect::<Vec<_>>();
        let mut ex = Explorer::new(root.clone());
        assert_eq!(names(&ex.rows()), ["0:Assets", "0:src", "0:A.md", "0:b.txt", "0:pic.png"]);

        ex.toggle(&root.join("src"));
        ex.toggle(&root.join("src/deep"));
        assert_eq!(
            names(&ex.rows()),
            ["0:Assets", "0:src", "1:deep", "2:x.toml", "1:main.rs", "0:A.md", "0:b.txt", "0:pic.png"]
        );

        // A new file shows after a refresh.
        std::fs::write(root.join("c.txt"), "c").unwrap();
        assert!(!ex.rows().iter().any(|(_, e)| e.name == "c.txt"));
        ex.refresh();
        assert!(ex.rows().iter().any(|(_, e)| e.name == "c.txt"));

        assert_eq!(read_text(&root.join("src/main.rs")).unwrap(), "fn main() {}");
        assert!(read_text(&root.join("pic.png")).is_err(), "binary files are not edited as text");
        assert!(is_image(&root.join("pic.png")) && !is_image(&root.join("b.txt")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
