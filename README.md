# Alien Artefact

A cross-platform (Windows / Linux) diagram editor written in Rust with [eframe/egui](https://crates.io/crates/eframe), modelled on the draw.io workflow.

## Features

- Objects: class/struct, and trait/interface (Rust trait, C# interface, C++ pure virtual class) shown with a `«trait»` stereotype above the name. Names are edited in the right panel or by double-clicking
- Files: a text file or a binary file, chosen when created (page shape with a folded corner and a format tag). A text file's format (txt, json, toml, rs, py, ...) comes from its extension (unknown ones are plain text): double-click one to edit its contents in a code editor tab (line numbers, syntax colours for json, toml, ini, yaml, xml, html, md, c, h, cpp, rs, py). Connected to types with Related only (no pointers, no members)
- Collections: set, vector, dictionary, tree, queue (stacked-cards shape with a «kind» stereotype). The element type comes from the collection's one outgoing pointer, or from text for outside types; dictionaries also have a key type
- Enums (variants, flags), external types, packages (frames for crates/modules) and directories
- Object properties: type parameters, scope (singleton / per request / per session), task (with period), generated; members can be async, repeated (×N), internal (~)
- Uses relation (calls / package dependencies); pointer lock (mutex / rwlock) and never-null [1]
- Validate: checks the whole diagram against the language rules (all the time while editing: objects and lines with a problem get an amber marker, and the strip at the top of the diagram shows the count, which opens the list; or `alien_artefact --validate file.json`)
- Shapes: rectangle, rounded rectangle, ellipse, diamond, triangle, hexagon, parallelogram, cylinder, text
- Select (click, Shift+click, rubber band), move with grid snapping, resize via handles
- Pointers (palette section): drag from the owner to the pointed-to object. Each kind has a fixed look:

  | Pointer | Line | Owner end | Target end | Badge |
  | --- | --- | --- | --- | --- |
  | Raw `T*` | dashed | - | open arrow | `*` |
  | Reference `T&` | solid | - | open arrow | `&` |
  | `unique_ptr` | solid | filled diamond | filled arrow | `unique_ptr` |
  | `shared_ptr` | solid | hollow diamond | filled arrow | `shared_ptr` |
  | `weak_ptr` | dotted | hollow diamond | open arrow | `weak_ptr` |

  A const target shows in the badge (`const *`, `unique_ptr<const>`). The label (member name) sits near the target. Ends dropped on a shape attach to it; drag a selected pointer's end handles to re-attach. Release on the owner itself for a self-pointer (drawn as a loop off its right side). Every pointer has one of the five kinds
- Relations (palette section):
  - Related: dotted line, no arrowheads. A loose, non-technical link (e.g. a config file and the struct loaded from it)
  - Implements: dashed line with a hollow triangle at the trait. Only from a class to a trait
  - Inheritance: solid line with a hollow triangle at the base. From the derived class to its base class, or from a trait to the trait it extends
  - Drawing a relation between the wrong objects shows an error window explaining why; OK removes it
- Routing: orthogonal or straight
- Format panel: fill/line/font colors, line width, font size, position and size
- Undo/redo, cut/copy/paste, duplicate, delete, bring to front / send to back
- Pan and zoom, fit to content
- Themes: Graphite (default: neutral dark), Neon (navy, glowing cyan and violet lines), Amethyst (purple, glowing lilac and pink) and Light, chosen in File > Preferences or View > Theme. In the dark themes the canvas adapts the diagram's colours on screen; saved files and SVG exports keep the real colours
- Explorer (first icon on the left, or File > Open Folder...): the folder as a tree like VS Code's, folders first; click a folder to expand it, a file to open it (text in the code editor, PNG/JPEG as a picture). Ctrl+S writes changed files back to disk; renaming a file's tab renames the file. The project remembers the folder and the open files
- Search (second icon on the left, or Ctrl+Shift+F): finds files by name in the explorer's folder (not inside `.git`); click a result to open it. Plain text is found anywhere in the name (`main` finds `domain.rs`); with wildcards the whole name must match: `*` is any text, `?` one character (`abc*xyz.cpp`, `*.rs`). A pattern with `/` is matched against the path in the folder (`src/*.rs`). Toggles in the field: Match Case (`Aa`, Alt+C) and Match Whole Word (Alt+W: no match starting or ending inside a word, and in a wildcard pattern the text beside each `*` is a whole word too, so `abc*.cpp` finds `abc-old.cpp` but not `abcd.cpp`)
- Projects and a docking workspace: a project holds diagrams, text files (same code editor as file objects; the tab name's extension picks the colours) and images (opened from a PNG or JPEG file with File > Open image), one tab each. Drag tabs between groups, onto an edge to split the area (as in VS Code), or out into a floating window; the layout is saved with the project. A dot after a tab's name means it has unsaved changes. Ctrl+T new diagram, Ctrl+W close tab; double-click a tab to rename, right-click to duplicate. Older project and single-diagram files still open
- Save/open as `.json`, export the shown diagram to `.svg`

## Controls

| Action | Input |
| --- | --- |
| Add shape | Click or drag from the palette (pick a category in the icon strip on the left) |
| Search for files | Ctrl+Shift+F (Alt+C match case, Alt+W whole word) |
| Show / hide the palette | Click the category's icon again, the ‹ button, or Ctrl+B |
| Edit name / label | Double-click an object or line, or F2 |
| Add text | Double-click empty canvas |
| Zoom | Ctrl + mouse wheel |
| Scroll | Mouse wheel (Shift for horizontal) |
| Pan | Middle- or right-drag |
| Draw a pointer | Click a type in Pointers, then drag from owner to target (Esc cancels) |
| Disable snapping while dragging | Hold Alt |
| Nudge | Arrow keys (Shift = 10 units) |
| Undo / Redo | Ctrl+Z / Ctrl+Y (or Ctrl+Shift+Z) |
| Copy / Cut / Paste / Duplicate | Ctrl+C / Ctrl+X / Ctrl+V / Ctrl+D |
| Select all / Delete | Ctrl+A / Delete |
| New / Open / Save / Save as | Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+S |
| New diagram (tab) / Close diagram | Ctrl+T / Ctrl+W |

## Run

```sh
cargo run
cargo run -- path/to/diagram.json   # open a diagram on startup
cargo test
```

## Platform notes

**Windows**: use the MSVC toolchain (needs Visual Studio Build Tools with the "Desktop development with C++" workload):

```sh
rustup default stable-x86_64-pc-windows-msvc
```

The GNU toolchain also works, but needs MinGW-w64 (`dlltool`) on `PATH`.

On Windows the app renders with DirectX 12 by default, because some older Intel Vulkan drivers crash during setup. Set `WGPU_BACKEND=vulkan` (or `gl`) to override.

**Linux**: install the windowing and graphics dev packages, e.g. on Debian/Ubuntu:

```sh
sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev
```

Both X11 and Wayland are supported. File dialogs use the XDG desktop portal (`xdg-desktop-portal` plus a backend such as `-gtk` or `-kde`).
