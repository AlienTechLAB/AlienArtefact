//! Code navigation for C and C++ with clangd (the language server VS Code's C++ tools use):
//! go to definition / declaration / type definition, references, and switching between a
//! header and its source file.
//!
//! clangd runs as a child process and is spoken to with the Language Server Protocol (JSON-RPC
//! over its standard input and output). A thread reads its answers; the app polls them every
//! frame, so nothing waits. clangd finds the project's `compile_commands.json` by itself
//! (in the file's folder or a folder above it).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};

use serde_json::{json, Value};

/// What to look for at a place in a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavKind {
    Definition,
    Declaration,
    TypeDefinition,
    References,
    FindAllReferences,
    SwitchHeaderSource,
}

impl NavKind {
    pub fn title(self) -> &'static str {
        match self {
            NavKind::Definition => "Go to Definition",
            NavKind::Declaration => "Go to Declaration",
            NavKind::TypeDefinition => "Go to Type Definition",
            NavKind::References => "Go to References",
            NavKind::FindAllReferences => "Find All References",
            NavKind::SwitchHeaderSource => "Switch Header/Source",
        }
    }

    /// What is looked for, for messages ("No definition found").
    pub fn noun(self) -> &'static str {
        match self {
            NavKind::Definition => "definition",
            NavKind::Declaration => "declaration",
            NavKind::TypeDefinition => "type definition",
            NavKind::References | NavKind::FindAllReferences => "references",
            NavKind::SwitchHeaderSource => "matching header or source file",
        }
    }

    fn method(self) -> &'static str {
        match self {
            NavKind::Definition => "textDocument/definition",
            NavKind::Declaration => "textDocument/declaration",
            NavKind::TypeDefinition => "textDocument/typeDefinition",
            NavKind::References | NavKind::FindAllReferences => "textDocument/references",
            NavKind::SwitchHeaderSource => "textDocument/switchSourceHeader",
        }
    }
}

/// A place in a file found by clangd. Columns are in UTF-16 units, as in the protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    /// From 0.
    pub line: usize,
    pub start_utf16: usize,
    pub end_utf16: usize,
}

/// The answer to a request: what was asked, and the places found (for Switch Header/Source, the
/// other file at line 0).
pub struct NavResult {
    pub kind: NavKind,
    pub locations: Vec<Location>,
}

/// A running clangd.
pub struct Lsp {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_id: i64,
    init_id: i64,
    /// The server answered `initialize`; before that, messages wait in `queue`.
    ready: bool,
    queue: Vec<Value>,
    pending: HashMap<i64, NavKind>,
    /// The documents sent to clangd: version and text.
    docs: HashMap<String, (i64, String)>,
    pub root: PathBuf,
}

impl Drop for Lsp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Lsp {
    /// Starts clangd for the folder `root`. `repaint` is called when an answer arrives.
    pub fn start(root: &Path, repaint: impl Fn() + Send + 'static) -> Result<Lsp, String> {
        Self::start_program("clangd", root, repaint)
    }

    fn start_program(program: &str, root: &Path, repaint: impl Fn() + Send + 'static) -> Result<Lsp, String> {
        let mut child = Command::new(program)
            .args(["--background-index", "--header-insertion=never", "--log=error"])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "clangd is not installed (on Debian/Ubuntu: sudo apt install clangd)".to_owned()
                } else {
                    format!("cannot start clangd: {e}")
                }
            })?;
        let stdin = child.stdin.take().ok_or("clangd has no input")?;
        let stdout = child.stdout.take().ok_or("clangd has no output")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Some(msg) = read_message(&mut reader) {
                if tx.send(msg).is_err() {
                    break;
                }
                repaint();
            }
        });
        let mut lsp = Lsp {
            child,
            stdin,
            rx,
            next_id: 1,
            init_id: 0,
            ready: false,
            queue: Vec::new(),
            pending: HashMap::new(),
            docs: HashMap::new(),
            root: root.to_path_buf(),
        };
        lsp.init_id = lsp.alloc_id();
        let init = json!({
            "jsonrpc": "2.0",
            "id": lsp.init_id,
            "method": "initialize",
            "params": {
                "processId": std::process::id(),
                "rootUri": path_to_uri(root),
                "capabilities": {
                    "general": { "positionEncodings": ["utf-16"] },
                    "textDocument": {
                        "definition": { "linkSupport": true },
                        "declaration": { "linkSupport": true },
                        "typeDefinition": { "linkSupport": true },
                        "references": {},
                        "synchronization": {}
                    }
                },
                "workspaceFolders": [{ "uri": path_to_uri(root), "name": "root" }]
            }
        });
        lsp.write(&init).map_err(|e| format!("clangd: {e}"))?;
        Ok(lsp)
    }

    fn alloc_id(&mut self) -> i64 {
        self.next_id += 1;
        self.next_id
    }

    fn write(&mut self, msg: &Value) -> std::io::Result<()> {
        let body = msg.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len())?;
        self.stdin.flush()
    }

    /// Sends now, or once the server is initialised.
    fn send(&mut self, msg: Value) {
        if self.ready {
            let _ = self.write(&msg);
        } else {
            self.queue.push(msg);
        }
    }

    /// Whether clangd has stopped.
    pub fn exited(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }

    /// Tells clangd the current text of `path` (opening it the first time).
    fn sync(&mut self, path: &Path, text: &str) {
        let uri = path_to_uri(path);
        match self.docs.get_mut(&uri) {
            Some((_, old)) if old == text => {}
            Some((version, old)) => {
                *version += 1;
                *old = text.to_owned();
                let v = *version;
                self.send(json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/didChange",
                    "params": {
                        "textDocument": { "uri": uri, "version": v },
                        "contentChanges": [{ "text": text }]
                    }
                }));
            }
            None => {
                let lang = match path.extension().and_then(|e| e.to_str()) {
                    Some("c") => "c",
                    _ => "cpp",
                };
                self.docs.insert(uri.clone(), (1, text.to_owned()));
                self.send(json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/didOpen",
                    "params": {
                        "textDocument": { "uri": uri, "languageId": lang, "version": 1, "text": text }
                    }
                }));
            }
        }
    }

    /// Asks for `kind` at `line` (from 0) and `col_utf16` of `path`, whose text is `text`.
    pub fn request(&mut self, kind: NavKind, path: &Path, text: &str, line: usize, col_utf16: usize) {
        self.sync(path, text);
        let id = self.alloc_id();
        let uri = path_to_uri(path);
        let mut params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col_utf16 }
        });
        if kind == NavKind::SwitchHeaderSource {
            params = json!({ "uri": uri });
        }
        if matches!(kind, NavKind::References | NavKind::FindAllReferences) {
            params["context"] = json!({ "includeDeclaration": true });
        }
        self.pending.insert(id, kind);
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": kind.method(), "params": params }));
    }

    /// The answers that arrived since the last call.
    pub fn poll(&mut self) -> Vec<NavResult> {
        let mut results = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            let id = msg.get("id").and_then(Value::as_i64);
            if msg.get("method").is_some() {
                // A request from the server (none are expected): answer it, so it does not wait.
                if let Some(id) = msg.get("id").cloned() {
                    let _ = self.write(&json!({ "jsonrpc": "2.0", "id": id, "result": null }));
                }
                continue;
            }
            if id == Some(self.init_id) {
                self.ready = true;
                let _ = self.write(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
                for m in std::mem::take(&mut self.queue) {
                    let _ = self.write(&m);
                }
                continue;
            }
            let Some(kind) = id.and_then(|id| self.pending.remove(&id)) else { continue };
            let result = msg.get("result").cloned().unwrap_or(Value::Null);
            let locations = if kind == NavKind::SwitchHeaderSource {
                result.as_str().and_then(uri_to_path).map(|path| Location { path, line: 0, start_utf16: 0, end_utf16: 0 }).into_iter().collect()
            } else {
                parse_locations(&result)
            };
            results.push(NavResult { kind, locations });
        }
        results
    }
}

/// Reads one message (`Content-Length` header, blank line, JSON body). `None` at the end.
fn read_message(reader: &mut impl BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            length = v.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; length?];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

/// The places in a definition / references answer: a Location, a list of Locations, or a list
/// of LocationLinks.
fn parse_locations(v: &Value) -> Vec<Location> {
    let one = |v: &Value| -> Option<Location> {
        let (uri, range) = match v.get("targetUri") {
            Some(uri) => (uri, v.get("targetSelectionRange").or_else(|| v.get("targetRange"))?),
            None => (v.get("uri")?, v.get("range")?),
        };
        let start = range.get("start")?;
        let end = range.get("end")?;
        let line = start.get("line")?.as_u64()? as usize;
        let start_utf16 = start.get("character")?.as_u64()? as usize;
        let end_line = end.get("line")?.as_u64()? as usize;
        let end_utf16 = if end_line == line { end.get("character")?.as_u64()? as usize } else { start_utf16 };
        Some(Location { path: uri_to_path(uri.as_str()?)?, line, start_utf16, end_utf16 })
    };
    match v {
        Value::Array(items) => items.iter().filter_map(one).collect(),
        Value::Null => Vec::new(),
        v => one(v).into_iter().collect(),
    }
}

/// `file://` URI of a path, with the characters URIs do not allow percent-encoded.
pub fn path_to_uri(path: &Path) -> String {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let s = abs.to_string_lossy().replace('\\', "/");
    let mut out = String::from(if s.starts_with('/') { "file://" } else { "file:///" });
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~:".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The path of a `file://` URI.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let s = String::from_utf8(out).ok()?;
    // Windows: "/C:/dir" -> "C:/dir".
    let s = if s.len() > 2 && s.as_bytes()[2] == b':' { s[1..].to_owned() } else { s };
    Some(PathBuf::from(s))
}

/// The UTF-16 column of character `col` of `line`.
pub fn utf16_col(line: &str, col: usize) -> usize {
    line.chars().take(col).map(char::len_utf16).sum()
}

/// The character column of UTF-16 column `col16` of `line`.
pub fn char_col(line: &str, col16: usize) -> usize {
    let mut units = 0;
    for (i, c) in line.chars().enumerate() {
        if units >= col16 {
            return i;
        }
        units += c.len_utf16();
    }
    line.chars().count()
}

const HEADERS: [&str; 6] = ["h", "hh", "hpp", "hxx", "h++", "inl"];
const SOURCES: [&str; 6] = ["c", "cc", "cpp", "cxx", "c++", "m"];

/// Whether `path` is a C or C++ header or source file.
pub fn is_c_family(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    HEADERS.contains(&ext.as_str()) || SOURCES.contains(&ext.as_str())
}

/// The header of a source file or the source of a header, by name: the same name with the
/// other kind of extension, in the same folder, or else the nearest one under `root` (not inside
/// `.git`). For Switch Header/Source without clangd, or when clangd does not know.
pub fn counterpart(path: &Path, root: Option<&Path>) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    let wanted: &[&str] = if HEADERS.contains(&ext.as_str()) {
        &SOURCES
    } else if SOURCES.contains(&ext.as_str()) {
        &HEADERS
    } else {
        return None;
    };
    let matches = |p: &Path| {
        p.file_stem().is_some_and(|s| s.to_string_lossy() == stem)
            && p.extension().is_some_and(|e| wanted.contains(&e.to_string_lossy().to_lowercase().as_str()))
    };
    let dir = path.parent()?;
    for e in wanted {
        let p = dir.join(format!("{stem}.{e}"));
        if p.is_file() {
            return Some(p);
        }
    }
    // Elsewhere under the root (e.g. include/ and src/): the one sharing the longest path.
    let root = root?;
    let mut best: Option<(usize, PathBuf)> = None;
    let mut dirs = vec![root.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let Ok(read) = std::fs::read_dir(&d) else { continue };
        for entry in read.filter_map(Result::ok) {
            let p = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if entry.file_name() != ".git" {
                    dirs.push(p);
                }
            } else if matches(&p) {
                let shared = p.components().zip(path.components()).take_while(|(a, b)| a == b).count();
                if best.as_ref().is_none_or(|(s, _)| shared > *s) {
                    best = Some((shared, p));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_and_columns() {
        let p = Path::new("/tmp/a dir/x+y.cpp");
        let uri = path_to_uri(p);
        assert_eq!(uri, "file:///tmp/a%20dir/x%2By.cpp");
        assert_eq!(uri_to_path(&uri).unwrap(), p);
        assert_eq!(uri_to_path("file:///C:/src/a.cpp").unwrap(), PathBuf::from("C:/src/a.cpp"));
        // "😀" is two UTF-16 units.
        let line = "a😀bc";
        assert_eq!(utf16_col(line, 2), 3);
        assert_eq!(char_col(line, 3), 2);
        assert_eq!(char_col(line, 99), 4);
    }

    #[test]
    fn locations_in_answers() {
        let loc = json!({ "uri": "file:///a.cpp", "range": { "start": { "line": 3, "character": 4 }, "end": { "line": 3, "character": 9 } } });
        let link = json!({ "targetUri": "file:///b.h", "targetRange": {}, "targetSelectionRange": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 2 } } });
        assert_eq!(parse_locations(&loc), [Location { path: "/a.cpp".into(), line: 3, start_utf16: 4, end_utf16: 9 }]);
        assert_eq!(parse_locations(&json!([loc, link])).len(), 2);
        assert_eq!(parse_locations(&json!([link]))[0].path, PathBuf::from("/b.h"));
        assert!(parse_locations(&Value::Null).is_empty());
        let msg = b"Content-Length: 14\r\n\r\n{\"id\":1,\"a\":2}";
        assert_eq!(read_message(&mut &msg[..]).unwrap()["a"], 2);
    }

    /// The whole exchange with a stand-in for clangd (a Python script answering like it).
    #[test]
    fn talks_to_a_language_server() {
        let dir = std::env::temp_dir().join(format!("alien_fake_clangd_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("log.txt");
        let server = dir.join("fake_clangd.py");
        std::fs::write(
            &server,
            format!(
                r#"#!/usr/bin/env python3
import sys, json
log = open({log:?}, "w")
def read():
    n = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line: sys.exit(0)
        line = line.strip()
        if not line: break
        if line.startswith(b"Content-Length:"): n = int(line.split(b":")[1])
    return json.loads(sys.stdin.buffer.read(n))
def send(m):
    b = json.dumps(m).encode()
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(b) + b)
    sys.stdout.buffer.flush()
while True:
    m = read()
    log.write(m.get("method", "?") + "\n"); log.flush()
    meth = m.get("method")
    if meth == "initialize": send({{"jsonrpc": "2.0", "id": m["id"], "result": {{"capabilities": {{}}}}}})
    elif meth == "textDocument/definition":
        p = m["params"]["position"]
        send({{"jsonrpc": "2.0", "id": m["id"], "result": [{{"uri": "file:///src/a.h", "range": {{"start": {{"line": p["line"], "character": 2}}, "end": {{"line": p["line"], "character": 5}}}}}}]}})
    elif meth == "textDocument/switchSourceHeader": send({{"jsonrpc": "2.0", "id": m["id"], "result": "file:///src/a.h"}})
    elif "id" in m: send({{"jsonrpc": "2.0", "id": m["id"], "result": None}})
"#
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut lsp = Lsp::start_program(server.to_str().unwrap(), &dir, || {}).unwrap();
        // Asked before the server is initialised: waits in the queue, then goes out in order.
        let file = dir.join("a.cpp");
        lsp.request(NavKind::Definition, &file, "int a;\nint b;", 1, 4);
        lsp.request(NavKind::SwitchHeaderSource, &file, "int a;\nint b;", 0, 0);
        lsp.request(NavKind::References, &file, "int a;\nint b; // edited", 0, 0);
        let mut results = Vec::new();
        let t = std::time::Instant::now();
        while results.len() < 3 && t.elapsed().as_secs() < 10 {
            results.extend(lsp.poll());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(results.len(), 3, "all three answered");
        assert_eq!(results[0].kind, NavKind::Definition);
        assert_eq!(results[0].locations, [Location { path: "/src/a.h".into(), line: 1, start_utf16: 2, end_utf16: 5 }]);
        assert_eq!(results[1].locations[0].path, PathBuf::from("/src/a.h"));
        assert!(results[2].locations.is_empty());
        drop(lsp);
        let methods = std::fs::read_to_string(&log).unwrap();
        assert_eq!(
            methods.lines().collect::<Vec<_>>(),
            [
                "initialize",
                "initialized",
                "textDocument/didOpen",
                "textDocument/definition",
                "textDocument/switchSourceHeader",
                "textDocument/didChange",
                "textDocument/references"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn header_and_source_by_name() {
        let root = std::env::temp_dir().join(format!("alien_lsp_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("include/lib")).unwrap();
        for f in ["src/Arith.cpp", "src/Arith.h", "src/Buffer.cpp", "include/lib/Buffer.hpp", "src/Lone.cpp"] {
            std::fs::write(root.join(f), "").unwrap();
        }
        assert_eq!(counterpart(&root.join("src/Arith.cpp"), Some(&root)), Some(root.join("src/Arith.h")));
        assert_eq!(counterpart(&root.join("src/Arith.h"), Some(&root)), Some(root.join("src/Arith.cpp")));
        assert_eq!(counterpart(&root.join("src/Buffer.cpp"), Some(&root)), Some(root.join("include/lib/Buffer.hpp")));
        assert_eq!(counterpart(&root.join("include/lib/Buffer.hpp"), Some(&root)), Some(root.join("src/Buffer.cpp")));
        assert_eq!(counterpart(&root.join("src/Lone.cpp"), Some(&root)), None);
        assert!(is_c_family(Path::new("a.HPP")) && !is_c_family(Path::new("a.rs")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
