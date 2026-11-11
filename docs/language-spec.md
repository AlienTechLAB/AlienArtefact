# Blueprint Language Specification

**Version:** 0.1 (draft)
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
- an object's **name** and its **members** (in order),
- a line's **source**, **target**, **label** and, for pointers, its **const** flag.

Position, size, color, line width, font and routing (straight or orthogonal) are presentation only and MUST NOT affect the generated code.

## 2. Diagram

A diagram is a set of **objects** and a set of **links**. A link is either a **pointer** or a **relation**.

| Element | Kinds |
|---|---|
| Object | Class, Trait |
| Pointer | Raw, Reference, Unique, Shared, Weak |
| Relation | Related, Implements, Inheritance |

Nothing else is part of the language. Free-standing shapes, text and untyped lines are not Blueprint elements (rule `D1`).

## 3. Objects

An object is a named type. It is drawn as a rectangle with a **header compartment** containing the name, and a **body** below it.

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
| **O2** | Object names MUST be unique within a diagram. |
| **O3** | An object name MUST be a valid type identifier in the target language: a letter or `_`, followed by letters, digits or `_`. |
| **O4** | An object's body holds its members (section 3.4). A class without members has an empty body. |

### 3.4 Members

A **member** is a field or a method declared inside an object.

| Part | Field | Method |
|---|---|---|
| Visibility | `+` public, `#` protected, `-` private | same |
| Name | the field name | the method name |
| Type | the field's type | the return type (empty = returns nothing) |
| Parameters | – | the parameter list, written as in the target language, e.g. `a: int, b: &str` |
| Static | belongs to the type, not to each value | called on the type, without a receiver |

**Notation.** One line per member, in UML form:

- fields: `<visibility> <name>: <type>`, for example `- count: int`,
- methods: `<visibility> <name>(<parameters>): <return type>`, for example `+ add(n: int): bool`. The `: <return type>` part is left out when the method returns nothing.

Static members are **underlined**. A class shows two compartments below its name, fields first and methods second, as soon as it has any member. A trait shows one compartment, for methods.

**Visibility meaning:**

- **public** (`+`): accessible to any code.
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

### 4.6 Pointer rules

| Rule | Statement |
|---|---|
| **P1** | Every pointer MUST have exactly one of the five kinds. |
| **P2** | Both ends of a pointer MUST be attached to objects. A pointer with a free end is incomplete and not a valid blueprint. |
| **P3** | Every pointer MUST have a label (its member name). |
| **P4** | Labels of the pointers leaving one owner MUST be unique, because they are members of the same type. |
| **P5** | A pointer MAY have the same object as owner and pointee (section 4.5). |
| **P6** | A **Unique** or **Shared** self-pointer MUST be nullable in the generated code, for example `Option<Box<T>>`. Otherwise the type would be infinite. |
| **P7** | A **Reference** self-pointer SHOULD NOT be used. A value cannot safely hold a reference to its own type in Rust, and in C++ such a reference must be bound at construction. Use Raw or Weak instead. |
| **P8** | The target of a **Weak** pointer MUST also be the target of at least one **Shared** pointer somewhere in the diagram, because a weak pointer can only observe a shared value. |
| **P9** | The **Shared** pointers MUST NOT form a cycle, counting self-pointers as cycles of length one. A cycle of shared owners never frees itself. Replace at least one pointer in the cycle with **Weak**. |
| **P10** | The **Unique** pointers MUST NOT form a cycle unless at least one pointer in the cycle is nullable and the cycle is broken at run time. A list node's `next` pointer satisfies this through `P6`. |
| **P11** | The pointee of a **Reference** MUST outlive the owner. This cannot be checked from the diagram, but generated code MUST express it, for example with a lifetime parameter in Rust. |

## 5. Relations

A **relation** is a link that is not a member. It states how two objects relate as types (Implements, Inheritance) or that they are loosely connected (Related).

### 5.1 Related

**Related** means the two objects belong together conceptually, without a technical link: there is no member, inheritance or call implied. Example: a configuration file and the struct its contents are loaded into.

- **Notation:** dotted line, no arrowheads.
- **Direction:** none; the relation is symmetric.
- **Label:** optional free text describing the connection, for example "loaded into".
- **Code:** none. Related is documentation for the reader of the blueprint.

### 5.2 Implements

**Implements** means the source class provides every behavior that the target trait declares.

- **Notation:** dashed line with a hollow triangle at the trait.
- **Direction:** from the implementing class to the trait.
- **Label:** none.

### 5.3 Inheritance

**Inheritance** means the source (derived) type is a specialization of the target (base) type: it has everything the base has, and a derived value can be used wherever a base value is expected.

- **Notation:** solid line with a hollow triangle at the base.
- **Direction:** from the derived type to the base type.
- **Between classes:** class inheritance.
- **Between traits:** the source trait extends the target trait. A Rust supertrait (`trait B: A`) and a C# interface extending another interface are both trait-to-trait inheritance.
- **Label:** none.

### 5.4 Relation rules

| Rule | Statement |
|---|---|
| **R1** | **Implements** MUST go from a class to a trait. |
| **R2** | **Inheritance** MUST go from a class to a class, or from a trait to a trait. |
| **R3** | **Inheritance** MUST NOT connect an object to itself: a class cannot derive from itself, and a trait cannot extend itself. |
| **R4** | **Implements** and **Inheritance** MUST have both ends attached to objects. |
| **R5** | The Inheritance relations MUST NOT form a cycle. A type cannot be its own ancestor. |
| **R6** | There MUST be at most one Implements relation between a given class and trait, and at most one Inheritance relation between a given pair of objects. |
| **R7** | A class SHOULD NOT implement a trait that it already gets through inheritance: if `B` inherits from `A` and `A` implements `T`, `B` does not also need to implement `T`. |
| **R8** | A class MAY inherit from several classes (multiple inheritance). The blueprint is then valid only for target languages that support it (C++). See section 6.3. |
| **R9** | **Related** MUST NOT connect an object to itself. Its ends SHOULD be attached to objects. |

## 6. Translation to source code

This section is normative for code generators and a guide for programmers. Placeholders: `A` is the source object, `B` the target object, `m` the pointer label, and `T` the target type.

### 6.1 Objects

| Element | C++ | Rust | C# |
|---|---|---|---|
| Class `A` | `class A { ... };` | `struct A { ... }` | `class A { ... }` |
| Trait `T` | `class T { public: virtual ~T() = default; /* pure virtual methods */ };` | `trait T { ... }` | `interface T { ... }` |

Members translate as follows:

| Member | C++ | Rust | C# |
|---|---|---|---|
| public | in a `public:` section | `pub` | `public` |
| protected | in a `protected:` section | `pub(crate)` (Rust has no protected; the closest is visibility within the crate) | `protected` |
| private | in a `private:` section | no modifier (private to the module) | `private` |
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
- **C# note:** C# has garbage collection and cannot express ownership in types. Ownership in a blueprint becomes a convention of who creates and disposes the value, and SHOULD be documented on the member.

### 6.3 Relations

| Relation | C++ | Rust | C# |
|---|---|---|---|
| `A` Implements `T` | `class A : public T` (overrides every pure virtual method) | `impl T for A { ... }` | `class A : T` |
| Class `A` Inheritance `B` | `class A : public B` | No class inheritance. Translate as composition: a field `base: B`, plus a trait extracted from `B` that both types implement (see below). | `class A : B` (single base class only) |
| Trait `S` Inheritance `T` | `class S : public T` (both abstract) | `trait S: T` | `interface S : T` |
| Related | – | – | – |

**Class inheritance in Rust.** Rust has no class inheritance, so a generator targeting Rust MUST NOT emit `Deref`-based pseudo-inheritance. It SHOULD do the following:

1. Extract the behavior of base `B` into a trait named after `B`, for example `trait Animal`.
2. Give each derived class `A` a field holding the base state, if `B` has state.
3. Implement the extracted trait for `B` and for every derived `A`.

**Multiple inheritance (`R8`)** translates directly only to C++. For Rust and C#, a blueprint with multiple class inheritance MUST be rewritten so that at most one base is a class and the others are traits.

## 7. Worked example

A diagram containing:

- class `Application`, with a Unique pointer `m_renderer` to trait `Renderer`, and a const Reference `m_config` to class `Config`,
- class `VulkanRenderer` Implements `Renderer`,
- classes `Dog` and `Cat`, each with an Inheritance relation to class `Animal`,
- class `Settings`, Related to class `SettingsFile` with the label "loaded into",
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

struct SettingsFile {}             // Related to Settings: no code
struct Settings {}
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

class SettingsFile {};                     // Related to Settings: no code
class Settings {};
```

## 8. Rule index

"Checked" means the reference editor (architect.ai) currently enforces the rule. Unchecked rules are still part of the language, and a diagram that breaks one is not a valid blueprint.

| Rule | Summary | Checked |
|---|---|---|
| D1 | Only objects, pointers and relations carry meaning; any other drawing element is invalid. | partly (no other elements can be created) |
| O1 | Object has a non-empty name | no |
| O2 | Object names are unique | no |
| O3 | Object name is a valid identifier | no |
| O4 | Body holds members | – |
| M1 | Member has a valid identifier name | no |
| M2 | Field names unique per object | no |
| M3 | Method names unique per object (no overloading for Rust) | no |
| M4 | Field name differs from the object's pointer labels | no |
| M5 | Traits have no fields | yes |
| M6 | Trait methods are public | yes |
| M7 | Field has a type | no |
| M8 | Pointers to diagram objects drawn as pointers, not fields | no |
| M9 | Implementing class provides every trait method | no |
| M10 | Protected members visible to derived types only | – (code generation) |
| P1 | Pointer has exactly one kind | yes |
| P2 | Pointer ends are attached to objects | no (free ends are allowed while drawing) |
| P3 | Pointer has a label | no |
| P4 | Pointer labels unique per owner | no |
| P5 | Self-pointers allowed | yes |
| P6 | Owning self-pointers are nullable | – (code generation) |
| P7 | No Reference self-pointers | no |
| P8 | Weak target is also a Shared target | no |
| P9 | No cycles of Shared pointers | no |
| P10 | No unbroken cycles of Unique pointers | no |
| P11 | Reference pointee outlives owner | – (code generation) |
| R1 | Implements: class to trait | yes |
| R2 | Inheritance: class to class, or trait to trait | yes |
| R3 | Inheritance: not to itself | yes |
| R4 | Implements and Inheritance ends attached | yes |
| R5 | No inheritance cycles | no |
| R6 | No duplicate Implements or Inheritance | no |
| R7 | No redundant Implements | no |
| R8 | Multiple inheritance only for C++ targets | no |
| R9 | Related: not to itself | yes |

## 9. Open questions (planned for later versions)

- **Method details.** Whether a method changes its object (`&mut self` vs `&self`, C++ `const` methods), default implementations in traits, and `virtual`/`override` for class methods.
- **Collections.** Multiplicity on pointers, such as `Vec<Box<B>>` or `std::vector<std::unique_ptr<B>>`.
- **Value members.** A member held by value rather than through a pointer (composition without indirection).
- **Non-code artifacts.** A dedicated element for files and other resources, for example the config file in the Related example.
- **Enums and generic types.**
- **Mutability of the pointer itself**, as opposed to the pointee (`T* const`).
