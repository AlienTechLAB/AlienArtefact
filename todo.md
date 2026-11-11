# TODO: gaps in the Blueprint language

Found by drawing a real program with it: **MechHeroServer** (Rust, branch `rust-refactoring`, about 148k lines in 4 crates). The diagram is [examples/mech_hero_server.json](examples/mech_hero_server.json). It covers the runtime core:
- the configuration,
- the web layer's shared state and sessions,
- the request handler and its handlers,
- the world tick,
- the store down to its tables and files.

The spec is [docs/language-spec.md](docs/language-spec.md) (now version 0.2).

## Done in language version 0.2

The first pass over the server found these gaps. They are now in the language and in the editor, and the diagram uses them:

| Gap | Now |
|---|---|
| Who calls whom | **Uses** relation (dash-dot, open arrow), between types and between packages (spec 5.4) |
| Locks (`Arc<Mutex<T>>`) | pointer **lock**: `mutex` / `rwlock`, shown as `+ mutex` in the badge (4.6) |
| Crates, modules, duplicate names | **Package** frames; names unique per package; package **Uses** = allowed dependency; rules `L1`–`L3` (3.9) |
| Generics (`Table<R: Row>`) | **type parameters** on classes, traits and enums (3.10) |
| 74 generated table fields | member **repeat** (`×74`) and the **generated** property (3.4, 3.10) |
| The 150-variant `Request` enum, `Privileges` | **Enum** object with variants (data in parentheses) and **flags** (3.7) |
| Tasks, timers, async | **task** property with a period (`«task every 1 s»`); **async** methods (3.10, 3.4) |
| Singleton vs. per request | **scope**: singleton / per request / per session (3.10) |
| Store directory, `<Table>.tbl` | **Directory** object; **name patterns** with `<placeholders>` (3.5) |
| `pub(crate)` | **internal** visibility `~` (3.4) |
| `Option<Arc<Store>>` vs. always set | pointer **never null** `[1]` (4.6) |
| Library types (`tokio::sync::Notify`) | **External type** object (3.8) |
| Whole-diagram rule check | **Validate** (toolbar, Diagram menu, and `architect_ai --validate <file>`) |

### What the Validate run found in the server

The server diagram passes every rule except one. That one is a real property of the code:

> **L1**: `GameConfig` (package web) points to `GameEngine` (package store), but package web does not use package store.

The web crate does not depend on store, but `web::GameConfig` holds an `Arc<GameEngine>`, a store type that it reaches through serverapi's `pub use store::engine::GameEngine`. The language cannot say "re-exported", so the diagram shows the crossing honestly, and the rule flags it (see the first item below).

## Still open: language

1. **Re-exports and aliases.** `serverapi::config::GameEngine` is `store::engine::GameEngine`. An object needs an "also known as" list (or a re-export line from a package to an object), so that `L1` can accept the path through serverapi.
2. **Value members.** An owned struct field (`Ctx { api: GameApi }`) is still drawn as a Unique pointer, the closest kind. A **value** pointer kind (no indirection, never null) would make that exact.
3. **Free functions and the entry point.** `main`, `run_server`, `serverapi::build_handler` (which builds the whole object graph) have no element. Packages could list module-level functions.
4. **Object visibility.** `pub struct` vs. a private type in a package. Today only members have visibility.
5. **Method details.** `&self` vs. `&mut self`, default trait methods, `virtual`/`override`.
6. **Generic instantiation as a link.** A pointer to `Table` cannot say "`Table<PlayerRow>`" except in text.

## Still open: tool

- **Views.** 48 boxes fit the screen only at about 20% zoom. Needed: showing one package at a time, collapsing a box (or a package) to its header, and search.
- **Auto-layout and routing.** Orthogonal lines run through boxes, and parallel lines overlap: the two `AccountHandler → Database` pointers draw on top of each other, labels too. Needed: a layered auto-layout (owners left of what they own) and routing that avoids boxes and separates parallel lines.
- **Packages and dragging.** Moving a package carries the objects inside it, but dropping an object into or out of a frame is not highlighted. Membership is purely geometric.
- **Import.** This diagram was written by a script after reading the code. A Rust importer (struct fields to members, `Arc`/`Box`/`&` to pointer kinds, `Mutex` to locks, `impl Trait for` to Implements, `Cargo.toml` to packages and package Uses) would get most of it automatically, using the mapping tables in spec section 6 in reverse.
