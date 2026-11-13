# Alien Artefact

A cross-platform (Windows / Linux) diagram editor written in Rust with [eframe/egui](https://crates.io/crates/eframe), modelled on the draw.io workflow.

## Features

- Objects: class/struct, and trait/interface (Rust trait, C# interface, C++ pure virtual class) shown with a `«trait»` stereotype above the name. Names are edited in the right panel or by double-clicking
- Files: a text file or a binary file, chosen when created (page shape with a folded corner and a format tag). A text file's format (txt, json, toml, rs, py, ...) comes from its extension (unknown ones are plain text): double-click one to edit its contents in a code editor tab (line numbers, syntax colours for json, toml, ini, yaml, xml, html, md, c, h, cpp, rs, py). Connected to types with Related only (no pointers, no members)
- Collections: set, vector, dictionary, tree, queue (stacked-cards shape with a «kind» stereotype). The element type comes from the collection's one outgoing pointer, or from text for outside types; dictionaries also have a key type
- Enums (variants, flags), external types, packages (frames for crates/modules) and directories
- Object properties: type parameters, scope (singleton / per request / per session), task (with period), generated; members can be async, repeated (×N), internal (~)
- Uses relation (calls / package dependencies); pointer lock (mutex / rwlock) and never-null [1]
- Validate: checks the whole diagram against the language rules (the Validate button at the top of a diagram, or `alien_artefact --validate file.json`)
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
- Dark theme (default) or light, switched with the **Dark** checkbox in the toolbar or View > Dark theme. In the dark theme the canvas adapts the diagram's colours on screen; saved files and SVG exports keep the real colours
- Projects and a docking workspace: a project holds diagrams, text files (same code editor as file objects; the tab name's extension picks the colours) and images (opened from a PNG or JPEG file with File > Open image), one tab each. Drag tabs between groups, onto an edge to split the area (as in VS Code), or out into a floating window; the layout is saved with the project. The diagram marked ● is the one the side panel and toolbar edit. Ctrl+T new diagram, Ctrl+W close tab; double-click a tab to rename, right-click to duplicate. Older project and single-diagram files still open
- Save/open as `.json`, export the shown diagram to `.svg`

## Controls

| Action | Input |
| --- | --- |
| Add shape | Click or drag from the palette (pick a category in the icon strip on the left) |
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
