# architect.ai

A cross-platform (Windows / Linux) diagram editor written in Rust with [eframe/egui](https://crates.io/crates/eframe), modelled on the draw.io workflow.

## Features

- UML: class/struct box with the name in a header compartment, editable in the Class section of the right panel
- Shapes: rectangle, rounded rectangle, ellipse, diamond, triangle, hexagon, parallelogram, cylinder, text
- Select (click, Shift+click, rubber band), move with grid snapping, resize via handles
- Connectors: hover a shape and drag a blue arrow onto another shape (or onto empty canvas to create a connected copy); click an arrow to clone the shape in that direction
- Pointers (palette section): drag from the owner to the pointed-to object. Each kind has a fixed look:

  | Pointer | Line | Owner end | Target end | Badge |
  | --- | --- | --- | --- | --- |
  | Raw `T*` | dashed | - | open arrow | `*` |
  | Reference `T&` | solid | - | open arrow | `&` |
  | `unique_ptr` | solid | filled diamond | filled arrow | `unique_ptr` |
  | `shared_ptr` | solid | hollow diamond | filled arrow | `shared_ptr` |
  | `weak_ptr` | dotted | hollow diamond | open arrow | `weak_ptr` |

  A const target shows in the badge (`const *`, `unique_ptr<const>`). The label (member name) sits near the target. Ends dropped on a shape attach to it; drag a selected pointer's end handles to re-attach. Release on the owner itself for a self-pointer (drawn as a loop off its right side). Every pointer has one of the five kinds; plain connectors (blue hover arrows) keep free pattern/arrowhead choice
- Routing: orthogonal or straight
- Format panel: fill/line/font colors, line width, font size, position and size
- Undo/redo, cut/copy/paste, duplicate, delete, bring to front / send to back
- Pan and zoom, fit to content
- Save/open as `.json`, export to `.svg`

## Controls

| Action | Input |
| --- | --- |
| Add shape | Click or drag from the Shapes panel |
| Edit text | Double-click a shape or connector, or F2 |
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
