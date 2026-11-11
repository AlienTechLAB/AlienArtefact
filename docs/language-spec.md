# Blueprint Language Specification

**Version:** 0.2 (draft)
**Status:** working draft; "Blueprint" is a working name.

## 1. Purpose and scope

Blueprint is a visual language for describing the structure of a program: its types, how they hold on to each other, and how they relate. A Blueprint diagram is a **blueprint for source code**. Every element that carries meaning maps to a definite construct in the target program. Because of that, a programmer or a code generator can write the program's type skeleton from the diagram alone.

This document defines:

- the **elements** of the language (objects, pointers, relations) and what each one means,
- the **well-formedness rules** a diagram must satisfy to be a valid blueprint,
- the **translation** of each element into source code (C++, Rust, C#).

It does not describe any editor, file format or user interface.

### 1.1 Level of abstraction

A diagram describes **types, not instances**. A box named `Dog` stands for the type `Dog`, not for one particular dog. An arrow from `Owner` to `Dog` means "every `Owner` value contains a member that refers to a `Dog`", not "this owner refers to that dog".

Consequences:

- Several types may each own a `Dog` through their own `unique_ptr`. That is not a conflict, because each `Owner` value owns a different `Dog` value.
- A pointer from a type to itself (a self-pointer) describes a recursive type such as a linked list node. It does not describe an object pointing at itself.

### 1.2 Conventions

- **MUST**, **MUST NOT**, **SHOULD** and **MAY** have their usual normative meaning.
- Rules are numbered (`O1`, `P3`, `R5`, ...) so tools and reviews can refer to them. Section 8 lists all rules.
- The **source** of a directed element is where it starts and its **target** is where it ends. Arrowheads are drawn at the target.

### 1.3 Meaning and presentation

Only the following carry meaning:

- the **kind** of each element,
- an object's **name**, its **members** (in order) and its **properties** (section 3.10),
- a line's **source**, **target**, **label** and, for pointers, its **const**, **lock** and **never null** settings,
- which **package** frame an object is drawn inside (section 3.9).

Position, size, color, line width, font and routing (straight or orthogonal) are presentation only and MUST NOT affect the generated code.

## 2. Diagram

A diagram is a set of **objects** and a set of **links**. A link is either a **pointer** or a **relation**.

| Element | Kinds |
|---|---|
| Object | Class, Trait, Enum, External type, Collection (Set, Vector, Dictionary, Tree, Queue), File, Directory, Package |
| Pointer | Raw, Reference, Unique, Shared, Weak |
| Relation | Related, Implements, Inheritance, Uses |

Nothing else is part of the language. Free-standing shapes, text and untyped lines are not Blueprint elements (rule `D1`).

## 3. Objects

An object is a named element of the program's world. **Classes**, **traits** and **enums** are the program's own *types*: each is drawn as a rectangle with a **header compartment** containing the name and a **body** below it. A **collection** is a container type whose elements are of another type (section 3.6). An **external type** comes from a library (section 3.8). **Files** and **directories** are not types but things the program works with outside memory (section 3.5). A **package** is a frame that groups objects into a crate, module or namespace (section 3.9).

### 3.1 Class

A **class** is a concrete type with state: a class or struct.

```
+--------------------------------+
|           ClassName            |   <- name
+--------------------------------+
| - count: int                   |   <- fields
| + MAX: int                     |      (static members are underlined)
+--------------------------------+
| + add(n: int): bool            |   <- methods
| - reset()                      |
+--------------------------------+
```

- A class can be instantiated.
- A class may own, share and reference other objects through pointers (section 4).
- A class may implement traits and inherit from other classes (section 5).

### 3.2 Trait

A **trait** is an interface: a set of behaviors that classes promise to provide. It corresponds to a Rust `trait`, a C# `interface`, or a C++ class with only pure virtual functions.

```
+--------------------------------+
|            «trait»             |   <- stereotype
|           TraitName            |   <- name
+--------------------------------+
| + draw(ctx: Context): bool     |   <- methods only
+--------------------------------+
```

- A trait is drawn like a class, with the stereotype `«trait»` above its name.
- A trait cannot be instantiated. Values are only ever reached through a pointer whose target is the trait, which makes it a *polymorphic* pointer (section 4.4).
- A trait is implemented by classes (`Implements`) and may extend other traits (`Inheritance`).

### 3.3 Object rules

| Rule | Statement |
|---|---|
| **O1** | Every object MUST have a non-empty name. |
| **O2** | Object names MUST be unique within their package (section 3.9). Objects outside every package share one top-level namespace. |
| **O3** | The name of a class, trait or enum MUST be a valid type identifier in the target language: a letter or `_`, followed by letters, digits or `_`. (File names follow `F1` instead.) |
| **O4** | An object's body holds its members (section 3.4). A class without members has an empty body. |

### 3.4 Members

A **member** is a field or a method declared inside an object.

| Part | Field | Method |
|---|---|---|
| Visibility | `+` public, `~` internal, `#` protected, `-` private | same |
| Name | the field name | the method name |
| Type | the field's type | the return type (empty = returns nothing) |
| Parameters | – | the parameter list, written as in the target language, e.g. `a: int, b: &str` |
| Static | belongs to the type, not to each value | called on the type, without a receiver |
| Async | – | runs asynchronously (`async fn`, `async Task`) |
| Repeat | this line stands for *N* fields of the same shape | this line stands for *N* methods of the same shape |

An **enum** has a third kind of member, the **variant** (section 3.7).

**Notation.** One line per member, in UML form:

- fields: `<visibility> <name>: <type>`, for example `- count: int`,
- methods: `<visibility> [async] <name>(<parameters>): <return type>`, for example `+ add(n: int): bool` or `+ async run(): i32`. The `: <return type>` part is left out when the method returns nothing,
- a repeated member ends in `×N`, for example `+ Accounts: Table<AccountsRow>  ×74`.

Static members are **underlined**. A class shows two compartments below its name, fields first and methods second, as soon as it has any member. A trait shows one compartment, for methods.

**Visibility meaning:**

- **public** (`+`): accessible to any code.
- **internal** (`~`): accessible to any code in the same package (section 3.9).
- **protected** (`#`): accessible to the object itself and to types that inherit from it.
- **private** (`-`): accessible only to the object itself.

**Types and parameters** are written as plain text in the syntax of the target language. When a type names another object of the diagram, it MUST use that object's exact name.

#### Member rules

| Rule | Statement |
|---|---|
| **M1** | Every member MUST have a non-empty name that is a valid identifier (as in `O3`). |
| **M2** | Field names MUST be unique within an object. |
| **M3** | Method names SHOULD be unique within an object. Overloading (same name, different parameters) is valid only for target languages that support it (C++, C#), not for Rust. |
| **M4** | A field MUST NOT have the same name as a pointer leaving the same object (`P3`), because a pointer is also a member. |
| **M5** | A trait MUST NOT have fields. It declares behavior only. |
| **M6** | Every trait method is public. A trait method MUST NOT be protected or private. |
| **M7** | A field MUST have a type. |
| **M8** | A field whose value is reached through a pointer to another object of the diagram SHOULD be drawn as a pointer (section 4) rather than as a field, so that ownership is visible. Fields are for values: numbers, strings, collections, and types outside the diagram. |
| **M9** | A class that **implements** a trait MUST provide every method of that trait, with the same name, parameters and return type. A class that inherits from a class that implements the trait gets them through inheritance (`R7`). |
| **M10** | A derived type MAY access the **protected** members of its base types. No other type may. |
| **M11** | A **repeated** member (`×N`, N > 1) is shorthand for N members that differ only in name and type parameters, typically produced by a tool. Its line shows one of them as an example. A code generator MUST NOT expand it by itself; the real list comes from the generating tool (see `generated`, section 3.10). |
| **M12** | Only methods can be **async**. |

### 3.5 File and directory

A **file** is something the program reads or writes outside its memory: a configuration file, a data file, an asset, a log. It is not a type and has no values in memory, so it is not translated into code by itself. It exists so a blueprint can show *which* types depend on *which* files.

```
+----------------------+\
|                      | \     <- folded corner
|                      +--+
|     settings.toml       |
|                         |
+-------------------------+
```

- **Notation:** a rectangle with a folded top-right corner, with the file name in the middle.
- **Name:** the file's name or path, e.g. `settings.toml`, `assets/shaders/basic.wgsl`, `logs/app.log`.
- **Connections:** a file is connected to types only by **Related**. The label says what the type does with the file: "reads", "writes", "loaded into", "saved from".

A **directory** is a folder of files: a store directory, a web root, a backup folder. It follows the same rules as a file (F1–F6) and is drawn as a folder: a rectangle with a small tab on its top edge.

**Name patterns.** A file or directory name MAY contain placeholders in angle brackets for parts that vary at run time: `<store>/<Table>.tbl` is one file per table in the store directory, `<store>/<n>/<id>.dat` one file per blob. A pattern names a *family* of files.

#### File rules

| Rule | Statement |
|---|---|
| **F1** | A file or directory MUST have a non-empty name: a file name, a path or a name pattern. It need not be an identifier. |
| **F2** | File and directory names MUST be unique within a diagram. Use a path to tell apart files with the same name. |
| **F3** | A file MUST NOT have members. |
| **F4** | A pointer MUST NOT start or end at a file or directory. A file is not a value in memory. A type that keeps a file open has a **field** holding the handle (e.g. `std::fstream`, `std::fs::File`, `FileStream`) and a Related line to the file. |
| **F5** | A file MUST NOT take part in **Implements** or **Inheritance**. |
| **F6** | A Related line to a file SHOULD have a label stating what the type does with it ("reads", "writes", "loaded into"). |

### 3.6 Collections

A **collection** is a container type that holds any number of **elements** of one element type. There are five kinds:

| Kind | Meaning | Order | Duplicates | Access |
|---|---|---|---|---|
| **Set** | A group of distinct elements | none | not allowed | by membership test |
| **Vector** | A sequence | insertion order, indexable | allowed | by index |
| **Dictionary** | Maps each **key** to one **value** | none | keys distinct | by key |
| **Tree** | A hierarchy: each element has an ordered list of child elements | parent before children | allowed | by walking from the root |
| **Queue** | A waiting line | first in, first out | allowed | front (remove) and back (add) |

**Notation:** two stacked cards (meaning "many"), with the kind as a stereotype (`«set»`, `«vector»`, `«dictionary»`, `«tree»`, `«queue»`) and the element type below it. A dictionary shows `Key → Value`.

```
  +-----------------------+
+-----------------------+ |
|       «vector»        | |
|                       | |
|        Texture        |-+
+-----------------------+
```

**Element type.** A collection's element type, which for a dictionary is its value type, comes from exactly one of:

- **an element pointer:** a pointer that starts at the collection and ends at an object of the diagram. Its kind says how each element refers to that object. Each element can be owned (Unique), shared (Shared), observed (Weak), borrowed (Reference) or addressed (Raw).
- **element text:** the name of a type outside the diagram, such as `i32` or `String`, when the collection has no element pointer.

A dictionary's **key type** is always given as text.

**Holding a collection.** An object holds a collection through a pointer from the object to the collection, and the pointer's label is the member name. A **Unique** pointer to a collection means the owner contains the collection directly, as a value member (section 6.4). Other pointer kinds keep their usual meaning: a Shared pointer to a collection shares the whole collection, and so on.

#### Collection rules

| Rule | Statement |
|---|---|
| **C1** | A collection MUST have at most one element pointer (one pointer starting at it). |
| **C2** | A collection MUST NOT be its own element: no self-pointer on a collection. |
| **C3** | A collection MUST have an element type: an element pointer, or non-empty element text. If it has an element pointer, the element text is ignored. |
| **C4** | A dictionary MUST have a key type. Key types SHOULD be types that can be compared for equality and hashed (numbers, strings, identifiers). |
| **C5** | An element pointer has no label. Its elements are not named members. (Exception to `P3`.) |
| **C6** | A collection MUST NOT have members, and MUST NOT take part in **Implements** or **Inheritance**. |
| **C7** | A collection MAY be the element of another collection: a vector of sets, a dictionary of vectors. |
| **C8** | Element pointers to a **trait** hold polymorphic elements and MUST use a pointer kind, never values (`Vec<Box<dyn T>>`, not `Vec<dyn T>`). |

### 3.7 Enum

An **enum** is a type whose value is exactly one of a closed list of **variants**. In Rust a variant may carry data; in C++ and C# variants are named constants.

```
+--------------------------------+
|            «enum»              |
|           Privileges           |
+--------------------------------+
| Everybody                      |   <- variants (with data in parentheses:
| User                           |      Login(LoginRequest))
| Admin                          |
+--------------------------------+
| + is_admin(): bool             |   <- methods
+--------------------------------+
```

- **Notation:** like a class with the stereotype `«enum»`. The first compartment lists the variants, the second the methods. A variant line is `Name` or `Name(data)`; variants have no visibility.
- **Flags:** an enum marked **flags** has variants that are bit flags and combine (`READ | WRITE`). Its stereotype reads `«flags»`.
- An enum is a value type. It may implement traits (Rust), be the target of pointers and the element of collections.

| Rule | Statement |
|---|---|
| **E1** | An enum MUST have at least one variant. |
| **E2** | Variant names MUST be unique within an enum. |
| **E3** | An enum MUST NOT have fields (data belongs to variants) and MUST NOT take part in **Inheritance**. |

### 3.8 External type

An **external type** is a type the program uses but does not define: one from a library or the standard library, such as `tokio::sync::Notify` or `std::fs::File`.

- **Notation:** a dashed rectangle with the stereotype `«external»` and the type's full path as its name.
- **Name:** the type's full path in the target language (`tokio::sync::Notify`, `System.Threading.Timer`). It need not be an identifier.
- It has no members in the diagram (they are not ours to describe).
- Pointers MAY point **to** an external type; they MUST NOT start **from** one (`P12`).
- A class or enum MAY implement an external **trait** (`impl Serialize for A`).
- Types are referenced in field text when a pointer would add nothing; an external object is worth drawing when it is pointed to, implemented or used.

### 3.9 Package

A **package** is a named group of objects: a Rust crate or module, a C++ namespace or library, a C# assembly or namespace.

```
+---------+
| web     |                          <- name tab
+---------+-------------------------+
|  +-----------+     +-----------+  |
|  | AppState  |     |   Ctx     |  |   <- objects drawn inside the frame
|  +-----------+     +-----------+  |      belong to the package
+-----------------------------------+
```

- **Notation:** a large frame with a name tab on its top-left corner, drawn behind the objects it holds.
- **Membership:** an object belongs to the innermost package whose frame contains it entirely. Packages MAY be nested (crate → module).
- **Names:** object names need only be unique within their package (`O2`). The full name of an object is `package::Name` (or `outer::inner::Name`).
- **Dependencies:** a **Uses** relation from package `A` to package `B` (section 5.4) declares that code in `A` may refer to code in `B`. These are the walls between crates: a crate that may not name another must not have a Uses line to it.

| Rule | Statement |
|---|---|
| **L1** | Every pointer, Uses, Implements or Inheritance line from an object in package `A` to an object in another package `B` MUST be matched by a **Uses** line from `A` (or a package enclosing it) to `B` (or a package enclosing it). Objects in the same package, or in nested packages, need no declaration. |
| **L2** | The Uses lines between packages MUST NOT form a cycle. |
| **L3** | A package MUST NOT be the source or target of a pointer, Implements, Inheritance or Related line. Packages connect only to packages, and only by Uses. |

### 3.10 Object properties

Classes, traits and enums have optional **properties** shown in the header, in the stereotype line between guillemets (`«singleton, task every 1 s»`):

| Property | Applies to | Meaning | Shown as |
|---|---|---|---|
| **Type parameters** | class, trait, enum | Generic parameters with their bounds, e.g. `R: Row`. | after the name: `Table<R: Row>` |
| **Scope** | class, enum | How many values a running program has: **many** (the default), **singleton** (one per process), **per request** (one per handled request), **per session** (one per user session). | `«singleton»`, `«per request»`, `«per session»` |
| **Task** | class | An **active object**: it runs on its own (a thread, a timer, an async task) instead of only being called. An optional **period** says how often it wakes, e.g. `every 1 s`. | `«task every 1 s»` |
| **Generated** | class, trait, enum | The source is produced by a tool and is not written by hand. | `«generated»` |

**Generic types.** A type parameter's bounds name traits of the diagram where possible (`R: Row` refers to the trait `Row`). A field or pointer that uses a generic type gives the arguments in its type text (`Table<PlayerRow>`), or, for a pointer, refers to the generic object itself and leaves the argument to the label or the target's text.


## 4. Pointers

A **pointer** is a directed link from an **owner** object (source) to a **pointee** object (target). It means: *the owner type has a member through which it reaches a value of the pointee type.* Each pointer is exactly one member.

### 4.1 Pointer kinds

| Kind | Meaning | Ownership | May be null | Line | At owner | At pointee | Badge |
|---|---|---|---|---|---|---|---|
| **Raw** | Plain address, no ownership | none | yes | dashed | – | open arrow | `*` |
| **Reference** | Alias to a value that outlives the owner | none | no | solid | – | open arrow | `&` |
| **Unique** | Sole owner; the pointee is destroyed with the owner | exclusive | yes | solid | filled diamond | filled arrow | `unique_ptr` |
| **Shared** | One of several owners; the pointee lives while any owner does | shared | yes | solid | hollow diamond | filled arrow | `shared_ptr` |
| **Weak** | Non-owning observer of a shared value; can detect that it is gone | none | yes (expires) | dotted | hollow diamond | open arrow | `weak_ptr` |

The diamond marks ownership: a filled diamond is exclusive ownership and a hollow one is shared. The **badge** is a small text tag near the owner end that repeats the kind in words.

### 4.2 Const target

Every pointer has a **const** flag.

- **const = true:** the owner MUST NOT modify the pointee through this pointer. The badge reads `const *`, `const &`, `unique_ptr<const>`, `shared_ptr<const>` or `weak_ptr<const>`.
- **const = false:** the owner MAY modify the pointee through this pointer.

The flag constrains the pointee as seen through this one member. It says nothing about whether the pointer itself can be re-pointed.

### 4.3 Label (member name)

A pointer's **label** is the name of the member that holds it, for example `m_renderer`. It is drawn near the pointee end.

### 4.4 Pointers to traits

A pointer whose target is a trait is **polymorphic**. At run time it reaches a value of some class that implements the trait. Every pointer kind may target a trait. For example, a Unique pointer to `Renderer` is `std::unique_ptr<Renderer>` in C++ and `Box<dyn Renderer>` in Rust.

### 4.5 Self-pointers

A pointer whose owner and pointee are the same object is a **self-pointer**. It is drawn as a loop off the object's right side and describes a recursive type, such as a list node with a `next` pointer or a tree node with a `parent` pointer.

### 4.6 Lock and never null

Two optional settings refine what a pointer reaches:

- **Lock**: `none`, `mutex` or `rwlock`. The pointee is guarded by a lock that must be held to use it: `Arc<Mutex<T>>` in Rust, a `std::mutex` held next to the pointer in C++. The badge ends in `+ mutex` or `+ rwlock`. A lock on the pointer to a collection guards the whole collection.
- **Never null** (`[1]`, UML multiplicity one): the pointer always points somewhere. It applies to **Raw**, **Unique** and **Shared** pointers. References are never null anyway, and Weak pointers can always expire. Without it, those kinds may be empty (`Option<…>` in Rust).

The badge shows both, for example `unique_ptr [1] + mutex`.

### 4.7 Pointer rules

| Rule | Statement |
|---|---|
| **P1** | Every pointer MUST have exactly one of the five kinds. |
| **P2** | Both ends of a pointer MUST be attached to objects. A pointer with a free end is incomplete and not a valid blueprint. |
| **P3** | Every pointer MUST have a label (its member name), except a collection's element pointer (`C5`). |
| **P4** | Labels of the pointers leaving one owner MUST be unique, because they are members of the same type. |
| **P5** | A pointer MAY have the same object as owner and pointee (section 4.5). |
| **P6** | A **Unique** or **Shared** self-pointer MUST be nullable: it MUST NOT be marked **never null**, and generated code uses e.g. `Option<Box<T>>`. Otherwise the type would be infinite. |
| **P7** | A **Reference** self-pointer SHOULD NOT be used. A value cannot safely hold a reference to its own type in Rust, and in C++ such a reference must be bound at construction. Use Raw or Weak instead. |
| **P8** | The target of a **Weak** pointer MUST also be the target of at least one **Shared** pointer somewhere in the diagram, because a weak pointer can only observe a shared value. |
| **P9** | The **Shared** pointers MUST NOT form a cycle, counting self-pointers as cycles of length one. A cycle of shared owners never frees itself. Replace at least one pointer in the cycle with **Weak**. |
| **P10** | The **Unique** pointers MUST NOT form a cycle unless at least one pointer in the cycle is nullable and the cycle is broken at run time. A list node's `next` pointer satisfies this through `P6`. |
| **P11** | The pointee of a **Reference** MUST outlive the owner. This cannot be checked from the diagram, but generated code MUST express it, for example with a lifetime parameter in Rust. |
| **P12** | A pointer connects types: it MUST NOT start or end at a file, directory or package (`F4`, `L3`), and MUST NOT start at an external type (`3.8`). |
| **P13** | **Never null** applies only to Raw, Unique and Shared pointers. |

## 5. Relations

A **relation** is a link that is not a member. It states how two objects relate as types (Implements, Inheritance), that one calls or depends on the other (Uses), or that they are loosely connected (Related).

### 5.1 Related

**Related** means the two objects belong together conceptually, without a technical link: there is no member, inheritance or call implied. Example: a configuration file and the struct its contents are loaded into.

- **Notation:** dotted line, no arrowheads.
- **Direction:** none; the relation is symmetric.
- **Label:** optional free text describing the connection, for example "loaded into".
- **Code:** none. Related is documentation for the reader of the blueprint.

### 5.2 Implements

**Implements** means the source class (or enum) provides every behavior that the target trait declares.

- **Notation:** dashed line with a hollow triangle at the trait.
- **Direction:** from the implementing class or enum to the trait. The trait may be an external one (section 3.8).
- **Label:** none.

### 5.3 Inheritance

**Inheritance** means the source (derived) type is a specialization of the target (base) type: it has everything the base has, and a derived value can be used wherever a base value is expected.

- **Notation:** solid line with a hollow triangle at the base.
- **Direction:** from the derived type to the base type.
- **Between classes:** class inheritance.
- **Between traits:** the source trait extends the target trait. A Rust supertrait (`trait B: A`) and a C# interface extending another interface are both trait-to-trait inheritance.
- **Label:** none.

### 5.4 Uses

**Uses** means the source calls or otherwise depends on the target without holding it as a member: it calls the target's methods, creates its values, or names it in a signature. It is the "who calls whom" of a design.

- **Notation:** dash-dot line with an open arrow at the target.
- **Direction:** from the user to the used.
- **Label:** optional; the methods called, e.g. `parse, self_test`.
- **Between types:** from a class, trait or enum to a class, trait, enum or external type.
- **Between packages:** a declared dependency (section 3.9, `L1`).
- **Code:** none by itself. It documents and checks dependencies; the calls are in method bodies.

### 5.5 Relation rules

| Rule | Statement |
|---|---|
| **R1** | **Implements** MUST go from a class or enum to a trait or an external type. |
| **R2** | **Inheritance** MUST go from a class to a class, or from a trait to a trait. |
| **R3** | **Inheritance** MUST NOT connect an object to itself: a class cannot derive from itself, and a trait cannot extend itself. |
| **R4** | **Implements** and **Inheritance** MUST have both ends attached to objects. |
| **R5** | The Inheritance relations MUST NOT form a cycle. A type cannot be its own ancestor. |
| **R6** | There MUST be at most one Implements, one Inheritance and one Uses relation between a given pair of objects. |
| **R7** | A class SHOULD NOT implement a trait that it already gets through inheritance: if `B` inherits from `A` and `A` implements `T`, `B` does not also need to implement `T`. |
| **R8** | A class MAY inherit from several classes (multiple inheritance). The blueprint is then valid only for target languages that support it (C++). See section 6.3. |
| **R9** | **Related** MUST NOT connect an object to itself. Its ends SHOULD be attached to objects. |
| **U1** | **Uses** MUST connect two different objects: a class, trait or enum to a class, trait, enum or external type, or a package to a package. It MUST NOT touch files, directories or collections. |

## 6. Translation to source code

This section is normative for code generators and a guide for programmers. Placeholders: `A` is the source object, `B` the target object, `m` the pointer label, and `T` the target type.

### 6.1 Objects

| Element | C++ | Rust | C# |
|---|---|---|---|
| Class `A` | `class A { ... };` | `struct A { ... }` | `class A { ... }` |
| Trait `T` | `class T { public: virtual ~T() = default; /* pure virtual methods */ };` | `trait T { ... }` | `interface T { ... }` |
| Enum `E` | `enum class E { ... };` (data variants: `std::variant` of structs) | `enum E { ... }` | `enum E { ... }` (data variants: a record hierarchy) |
| Flags enum `E` | `enum class E : uint32_t` plus bit operators | a `bitflags!` type or `u32` constants | `[Flags] enum E` |
| External type | – (imported: `#include`) | – (imported: `use path::Type`) | – (imported: `using`) |
| Package | `namespace` (and a library target) | a crate or `mod` | a namespace (and an assembly) |
| File, directory | – (no code; documents which types use them) | – | – |
| Collection | see section 6.4 | see section 6.4 | see section 6.4 |

Members translate as follows:

| Member | C++ | Rust | C# |
|---|---|---|---|
| public | in a `public:` section | `pub` | `public` |
| protected | in a `protected:` section | `pub(crate)` (Rust has no protected; the closest is visibility within the crate) | `protected` |
| internal | public, inside the package's `detail` namespace | `pub(crate)` | `internal` |
| private | in a `private:` section | no modifier (private to the module) | `private` |
| async method | returns `std::future<R>` (or a coroutine) | `async fn name(...) -> R` | `async Task<R> Name(...)` |
| field `name: T` | `T name;` | `name: T` | `T name;` |
| static field | `static T name;` | an associated `const` or a module-level `static` | `static T name;` |
| method `name(p): R` | `R name(p);` | `fn name(&self, p) -> R` | `R name(p)` |
| static method | `static R name(p);` | `fn name(p) -> R` (no `self`) | `static R name(p)` |
| trait method | `virtual R name(p) = 0;` | `fn name(&self, p) -> R;` in the `trait` | `R name(p);` in the `interface` |

In Rust, a non-static method takes `&self`. It SHOULD take `&mut self` when it changes the value's fields; this version of the language does not record which.

### 6.2 Pointers

Each pointer from `A` to `B` labeled `m` becomes a member `m` of `A`. The table shows the member's type. Rust has two columns, because the const flag decides between shared and mutable access.

| Kind | C++ (const = false / true) | Rust, const = true | Rust, const = false | C# |
|---|---|---|---|---|
| Raw | `B*` / `const B*` | `*const B` | `*mut B` | – (no raw pointers in safe code; use a nullable reference `B?`) |
| Reference | `B&` / `const B&` | `&'a B` | `&'a mut B` | `B` (non-null reference, set in constructor) |
| Unique | `std::unique_ptr<B>` / `std::unique_ptr<const B>` | `Box<B>` | `Box<B>` | `B` (sole reference; the owner is responsible for `Dispose`) |
| Shared | `std::shared_ptr<B>` / `std::shared_ptr<const B>` | `Rc<B>` (or `Arc<B>` across threads) | `Rc<RefCell<B>>` (or `Arc<Mutex<B>>`) | `B` |
| Weak | `std::weak_ptr<B>` / `std::weak_ptr<const B>` | `Weak<B>` | `Weak<RefCell<B>>` | `WeakReference<B>` |

Further rules for generated code:

- **Pointers to a trait** use the trait as `B`. In Rust that means `dyn` types: `Box<dyn T>`, `&dyn T`, `Rc<dyn T>`, `*const dyn T`. In C++ it is a pointer or reference to the abstract base.
- **Nullable kinds** (Raw, Unique, Shared, Weak) map to nullable members. In Rust, Unique becomes `Option<Box<B>>` and Shared becomes `Option<Rc<B>>` wherever the member can be empty. This is required for self-pointers and ownership cycles (`P6`, `P10`).
- **Reference** members require a lifetime parameter on `A` in Rust (`struct A<'a>`). In C++ they must be bound in the constructor.
- **Never null** (`[1]`): no `Option` in Rust, a reference-like non-null member in C++ (`gsl::not_null`, or a reference if it never changes), a non-nullable reference type in C#.
- **Lock** `mutex` / `rwlock`: Rust `Arc<Mutex<B>>` / `Arc<RwLock<B>>` (with a Unique pointer, `Mutex<B>` in place); C++ the pointer plus a `std::mutex` / `std::shared_mutex` member that guards it; C# a lock object (`lock (...)`) or `ReaderWriterLockSlim`.
- **C# note:** C# has garbage collection and cannot express ownership in types. Ownership in a blueprint becomes a convention of who creates and disposes the value, and SHOULD be documented on the member.

### 6.3 Relations

| Relation | C++ | Rust | C# |
|---|---|---|---|
| `A` Implements `T` | `class A : public T` (overrides every pure virtual method) | `impl T for A { ... }` | `class A : T` |
| Class `A` Inheritance `B` | `class A : public B` | No class inheritance. Translate as composition: a field `base: B`, plus a trait extracted from `B` that both types implement (see below). | `class A : B` (single base class only) |
| Trait `S` Inheritance `T` | `class S : public T` (both abstract) | `trait S: T` | `interface S : T` |
| Related | – | – | – |
| Uses (types) | – (calls inside methods; `#include` of the used type) | – (calls; `use` of the used type) | – (calls; `using`) |
| Uses (packages) | the library links to the used one | a `[dependencies]` entry in `Cargo.toml` | a project reference |

**Class inheritance in Rust.** Rust has no class inheritance, so a generator targeting Rust MUST NOT emit `Deref`-based pseudo-inheritance. It SHOULD do the following:

1. Extract the behavior of base `B` into a trait named after `B`, for example `trait Animal`.
2. Give each derived class `A` a field holding the base state, if `B` has state.
3. Implement the extracted trait for `B` and for every derived `A`.

**Multiple inheritance (`R8`)** translates directly only to C++. For Rust and C#, a blueprint with multiple class inheritance MUST be rewritten so that at most one base is a class and the others are traits.

### 6.4 Collections

A collection becomes the container type below, with `E` the **element type** and `K` the key type:

| Kind | C++ | Rust | C# |
|---|---|---|---|
| Set | `std::unordered_set<E>` | `HashSet<E>` | `HashSet<E>` |
| Vector | `std::vector<E>` | `Vec<E>` | `List<E>` |
| Dictionary | `std::unordered_map<K, E>` | `HashMap<K, E>` | `Dictionary<K, E>` |
| Tree | `Tree<E>` (no standard type; see below) | `Tree<E>` (see below) | `Tree<E>` (see below) |
| Queue | `std::queue<E>` (or `std::deque<E>`) | `VecDeque<E>` | `Queue<E>` |

**Element type `E`:**

- From element text: the text as written. A vector with element text `i32` becomes `Vec<i32>` / `std::vector<i32>`.
- From an element pointer to object `B`: the pointer member type from section 6.2, with two refinements for elements:
  - **Unique** to a class means the elements are owned values: `std::vector<B>`, `Vec<B>`. To a trait it means boxed values: `std::vector<std::unique_ptr<T>>`, `Vec<Box<dyn T>>` (`C8`).
  - **Reference** elements are not allowed in C++ containers. A C++ generator MUST use `std::reference_wrapper<const B>` or `B*` instead.

**Holding a collection:** a **Unique** pointer from owner `A` to a collection, labeled `m`, becomes a value member: `std::vector<E> m;` in C++, `m: Vec<E>` in Rust, and in C# a field `List<E> m` initialized in the constructor. Other pointer kinds wrap the container type as in section 6.2, e.g. Shared becomes `Rc<RefCell<Vec<E>>>`.

**Tree.** No target language has a standard general tree, so a generator MUST emit one node type per tree element type:

```rust
struct Tree<E> { value: E, children: Vec<Tree<E>> }
```

```cpp
template <class E> struct Tree { E value; std::vector<Tree<E>> children; };
```

### 6.5 Object properties

| Property | C++ | Rust | C# |
|---|---|---|---|
| Type parameters `R: Row` | `template <class R> requires Row<R>` | `struct Table<R: Row>` | `class Table<R> where R : IRow` |
| Scope singleton | one instance created at startup and passed by reference (or a function-local static) | one value created in `main` and shared (`Arc`), or a `static OnceLock<T>` | registered as a singleton (DI), or a static instance |
| Scope per request / per session | created per request / per session by the code that handles it | same | DI *scoped* lifetime |
| Task with period | a `std::thread` (or `std::jthread`) looping with `sleep_for(period)` | `tokio::spawn` of a loop over `tokio::time::interval(period)` | a `System.Threading.Timer` or a background `Task` loop |
| Generated | not written by hand; the generating tool is part of the build | same (a `build.rs`, a macro, or a checked-in generated module) | same (a source generator) |

## 7. Worked example

A diagram containing:

- class `Application`, with a Unique pointer `m_renderer` to trait `Renderer`, and a const Reference `m_config` to class `Config`,
- class `VulkanRenderer` Implements `Renderer`,
- classes `Dog` and `Cat`, each with an Inheritance relation to class `Animal`,
- file `settings.toml`, Related to class `Settings` with the label "loaded into",
- class `Node`, with a Unique self-pointer `next`.

translates to Rust as:

```rust
trait Renderer {}

struct VulkanRenderer {}
impl Renderer for VulkanRenderer {}

struct Config {}

struct Application<'a> {
    m_renderer: Box<dyn Renderer>, // Unique -> trait
    m_config: &'a Config,          // Reference, const
}

trait Animal {}                    // extracted from base class Animal (6.3)
struct Dog {}
struct Cat {}
impl Animal for Dog {}
impl Animal for Cat {}

struct Node {
    next: Option<Box<Node>>,       // Unique self-pointer, nullable (P6)
}

struct Settings {}                 // loaded from settings.toml (Related: no code)
```

and to C++ as:

```cpp
class Renderer { public: virtual ~Renderer() = default; };
class VulkanRenderer : public Renderer {};

class Config {};

class Application {
    std::unique_ptr<Renderer> m_renderer;  // Unique -> trait
    const Config& m_config;                // Reference, const
};

class Animal {};
class Dog : public Animal {};
class Cat : public Animal {};

class Node {
    std::unique_ptr<Node> next;            // Unique self-pointer
};

class Settings {};                         // loaded from settings.toml (Related: no code)
```

## 8. Rule index

**Checked** says how the reference editor (architect.ai) checks each rule:
- **drawing:** the editor refuses the drawing action and explains why.
- **validate:** the **Validate** command reports it (and `architect_ai --validate <file>` from the command line).
- **–:** cannot be decided from the diagram; it applies to generated code.

Unchecked rules are still part of the language, and a diagram that breaks one is not a valid blueprint.

| Rule | Summary | Checked |
|---|---|---|
| D1 | Only language elements carry meaning; other shapes and untyped lines are invalid | validate |
| O1 | Object has a non-empty name | validate |
| O2 | Object names unique within their package | validate |
| O3 | Class/trait/enum name is a valid identifier | validate |
| O4 | Body holds members | – |
| M1 | Member has a valid identifier name | validate |
| M2 | Field names unique per object | validate |
| M3 | Method names unique per object (no overloading for Rust) | no |
| M4 | Field name differs from the object's pointer labels | validate |
| M5 | Traits have no fields | drawing, validate |
| M6 | Trait methods are public | drawing, validate |
| M7 | Field has a type | validate |
| M8 | Pointers to diagram objects drawn as pointers, not fields | no |
| M9 | Implementing class provides every trait method | no |
| M10 | Protected members visible to derived types only | – |
| M11 | Repeated members are not expanded by generators | – |
| M12 | Only methods are async | drawing |
| E1 | Enum has at least one variant | validate |
| E2 | Variant names unique | validate |
| E3 | Enums have no fields and no inheritance | drawing, validate |
| F1 | File/directory has a name, path or pattern | validate |
| F2 | File/directory names unique | validate |
| F3 | Files have no members | drawing |
| F4 | No pointers to or from files and directories | drawing, validate |
| F5 | Files take no part in Implements or Inheritance | drawing, validate |
| F6 | Related lines to files are labeled | no |
| C1 | At most one element pointer per collection | drawing, validate |
| C2 | A collection is not its own element | drawing, validate |
| C3 | A collection has an element type | validate |
| C4 | A dictionary has a key type | validate |
| C5 | Element pointers have no label | – |
| C6 | Collections have no members, no Implements/Inheritance | drawing, validate |
| C7 | Collections may contain collections | drawing |
| C8 | Trait elements use a pointer kind | – |
| L1 | Cross-package lines match a declared package Uses | validate |
| L2 | No cycles of package dependencies | validate |
| L3 | Packages connect only to packages, only by Uses | drawing, validate |
| P1 | Pointer has exactly one kind | drawing |
| P2 | Pointer ends are attached to objects | validate (free ends are allowed while drawing) |
| P3 | Pointer has a label | validate |
| P4 | Pointer labels unique per owner | validate |
| P5 | Self-pointers allowed | drawing |
| P6 | Owning self-pointers are nullable | validate |
| P7 | No Reference self-pointers | validate |
| P8 | Weak target is also a Shared target | validate |
| P9 | No cycles of Shared pointers | validate |
| P10 | No unbroken cycles of Unique pointers | no |
| P11 | Reference pointee outlives owner | – |
| P12 | Pointers connect types (no files, directories, packages; not from external types) | drawing, validate |
| P13 | Never null only on Raw, Unique, Shared | drawing |
| R1 | Implements: class or enum to trait or external type | drawing, validate |
| R2 | Inheritance: class to class, or trait to trait | drawing, validate |
| R3 | Inheritance: not to itself | drawing, validate |
| R4 | Implements and Inheritance ends attached | drawing, validate |
| R5 | No inheritance cycles | validate |
| R6 | No duplicate Implements, Inheritance or Uses | validate |
| R7 | No redundant Implements | no |
| R8 | Multiple inheritance only for C++ targets | no |
| R9 | Related: not to itself | drawing, validate |
| U1 | Uses: type to type, or package to package | drawing, validate |

## 9. Open questions (planned for later versions)

- **Method details.** Whether a method changes its object (`&mut self` vs `&self`, C++ `const` methods), default implementations in traits, and `virtual`/`override` for class methods.
- **Ordered sets and dictionaries** (`BTreeSet`, `std::map`) as opposed to hashed ones.
- **Object keys** for dictionaries (a key that is an object of the diagram).
- **Value members.** A member held by value rather than through a pointer (composition without indirection). Today a Unique pointer stands in for it.
- **Mutability of the pointer itself**, as opposed to the pointee (`T* const`).
- **Re-exports and aliases.** A type re-exported by another package (`pub use store::engine::GameEngine` in `serverapi`) is reached through that package, but `L1` sees only the defining one.
- **Free functions.** Module-level functions (`main`, `build_handler`) have no element yet; packages could list them.
- **Object visibility.** Whether an object is visible outside its package (`pub struct` vs. private).
