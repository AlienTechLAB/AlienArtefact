# architect.ai

A minimal cross-platform (Windows / Linux) desktop window written in Rust using [eframe](https://crates.io/crates/eframe).

## Run

```sh
cargo run
```

## Platform notes

**Windows** — use the MSVC toolchain (requires Visual Studio Build Tools with the "Desktop development with C++" workload):

```sh
rustup default stable-x86_64-pc-windows-msvc
```

The GNU toolchain also works, but needs MinGW-w64 (`dlltool`) on `PATH`.

**Linux** — install the windowing/graphics dev packages, e.g. on Debian/Ubuntu:

```sh
sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev
```

Both X11 and Wayland are supported.
