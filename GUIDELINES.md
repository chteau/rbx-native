# Rust Engineering & Style Guidelines

This guide summarizes best practices from the official Rust documentation (*The Rust Programming Language*, *The Rust Edition Guide*, *API Guidelines*, and *Clippy*).

---

## 1. Code Style & Naming Conventions

Rust uses strict casing conventions defined by standard RFCs.

| Element | Casing | Example |
| :--- | :--- | :--- |
| Types, Traits, Enums, Structs | ```CamelCase``` | ```NetworkBuffer```, ```HttpResponse``` |
| Functions, Methods, Modules, Variables | ```snake_case``` | ```fetch_data```, ```user_id``` |
| Constants, Static Variables | ```SCREAMING_SNAKE_CASE``` | ```MAX_THROTTLE_LIMIT``` |
| Type Parameters | ```CamelCase``` (short) | ```T```, ```K```, ```V``` |
| Lifetimes | ```snake_case``` (short) | ``` 'a ```, ``` 'req ``` |

### Formatting
- Use **```rustfmt```** out of the box without manual adjustments to maintain idiomatic layout across projects.
- Prefer **4 spaces** for indentation.

> **Official Reference:**
> - [Rust API Guidelines — Naming Conventions (```C-CASE```)](https://rust-lang.github.io/api-guidelines/naming.html)

---

## 2. Pragmatic Documentation & Human Comments

Comments should respect the maintainer's time. Avoid corporate boilerplate or repeating what the code already says.

### The Golden Rules of Human Comments
1. **Explain WHY, not WHAT**: The code describes *what* happens. Comments explain *why* a specific decision, trade-off, or workaround was made.
2. **Zero redundancy**: Never write doc comments that restate the function signature.
   - **Bad**: `/// Sets the age of the user.` above `fn set_age(&mut self, age: u32)`.
   - **Good**: `/// Clamps age to 120 to prevent allocation overflow in legacy systems.`
3. **Document Invariants & Non-Obvious Behavior**: Highlight thread-safety limits, lock ordering requirements, unexpected side-effects, or performance implications.

### Syntax Rules

```rust
//! # Crate / Module Overview
//! Keep high-level module docs short (2-4 sentences max). 
//! Explain what problem this module solves and point to the main entry point struct.

/// High-level entry point for database operations.
///
/// Note: Keeps an internal connection pool alive. Do not clone across thread boundaries
/// without wrapping in an `Arc`.
pub struct DbClient {
    // Internal state: mutex needed due to thread-safety limits in third-party C bindings
    raw_ptr: *mut c_void,
}
```

- **```///```**: Outer doc comments. High-density, practical explanation of public interfaces. Include short, compilable examples where usage isn't obvious.
- **```//!```**: Inner doc comments. High-level architecture summary for crate/module roots.
- **```//```**: Inline implementation notes. Use exclusively for technical reasoning, complex math formulas, or `SAFETY:` guarantees in `unsafe` blocks.

> **Official References:**
> - [The Rust Programming Language — Chapter 14.2: Documentation Comments](https://doc.rust-lang.org/book/ch14-02-publishing-to-crates-io.html#commenting-contained-items)
> - [Rust API Guidelines — Documentation (```C-DOC```)](https://rust-lang.github.io/api-guidelines/documentation.html)

---

## 3. Idiomatic Ownership, Lifetimes & Types

### Memory & Ownership
- **Borrow before cloning**: Pass references (```&T``` or ```&mut T```) instead of calling ```.clone()``` unless ownership is explicitly required.
- **Prefer slices over owned types in parameters**: Use ```&str``` over ```&String```, and ```&[T]``` over ```&Vec<T>``` to leverage Deref coercion.

```rust
// Avoid (forces allocation / owned string)
fn process_name(name: String) {}

// Idiomatic (accepts &String, &str, string literals)
fn process_name(name: &str) {}
```

### Type System & Design
- **Make invalid states unrepresentable**: Use Enums to enforce state safety at compile time.
- **Newtype pattern**: Wrap primitive types in single-field tuple structs to add type safety and domain-specific semantics.

```rust
// Prevents accidentally passing a UserId where an OrderId is expected
pub struct UserId(pub u64);
pub struct OrderId(pub u64);
```

- **Implement Standard Traits**: Derive standard behavior (```Debug```, ```Clone```, ```PartialEq```, ```Eq```, ```Default```) where applicable.

> **Official References:**
> - [The Rust Programming Language — Chapter 4: Ownership](https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html)
> - [Rust API Guidelines — Deref Coercion (```C-DEREF```)](https://rust-lang.github.io/api-guidelines/flexibility.html#types-that-have-a-clear-target-type-implement-deref-coercion-c-deref)
> - [Rust Design Patterns — Newtype Pattern](https://rust-unofficial.github.io/patterns/patterns/behavioural/newtype.html)

---

## 4. Error Handling

Rust distinguishes between recoverable and unrecoverable errors.

- **Use ```Result<T, E>```** for expected, recoverable errors.
- **Use ```panic!```** only for unrecoverable invariants or bug states.
- **Operator ```?```**: Use the ```?``` operator for clean error propagation instead of manual ```match``` statements.

```rust
use std::fs::File;
use std::io::{self, Read};

fn read_config(path: &str) -> Result<String, io::Error> {
    let mut file = File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}
```

- Custom errors should implement ```std::error::Error``` or use ecosystem standard crates:
  - **```thiserror```**: For defining structured domain errors in libraries.
  - **```anyhow```**: For flexible application-level error handling.

> **Official References:**
> - [The Rust Programming Language — Chapter 9: Error Handling](https://doc.rust-lang.org/book/ch09-00-error-handling.html)
> - [Rust by Example — The ```?``` Operator](https://doc.rust-lang.org/rust-by-example/error/result/enter_question_mark.html)

---

## 5. Algorithmic Patterns & Functional Idioms

### Iterators vs. Manual Loops
Prefer iterator combinators over manual ```for``` loops. They are idiomatic, safer, and compile down to zero-cost abstractions equal to or faster than manual loops.

```rust
let numbers = vec![1, 2, 3, 4, 5];

// Idiomatic: Declarative iterator chain
let sum_of_evens: i32 = numbers
    .iter()
    .filter(|&&x| x % 2 == 0)
    .map(|&x| x * 2)
    .sum();
```

### Pattern Matching & Early Return
Use destructuring and ```let else``` constructs (Rust 1.65+) to handle control flow cleanly without deep nesting.

```rust
// Clean unwrapping with ```let else```
let Some(user) = fetch_user(id) else {
    return Err(AppError::NotFound);
};
```

> **Official References:**
> - [The Rust Programming Language — Chapter 13.2: Processing a Series of Items with Iterators](https://doc.rust-lang.org/book/ch13-02-iterators.html)
> - [The Rust Reference — ```let else``` Statements](https://doc.rust-lang.org/reference/statements.html#let-statements)

---

## 6. Codebase Architecture & Modular Design

Codebase quality focuses on navigation, compile times, encapsulation, and structural readability rather than individual line details.

### Strict Encapsulation (Minimize `pub` Scope)
Default all structs, fields, functions, and modules to private. Explicitly control internal visibility using restricted qualifiers:
- ```pub(crate)```: Visible anywhere inside the current crate, hidden from external consumers.
- ```pub(super)```: Visible only to the parent module.

### Clean Public API (Facade Pattern & Re-exports)
Decouple internal directory structures from what consumers (or other parts of your app) use. Organize code internally into deep submodules, but re-export key primitives cleanly at the module root using ```pub use```.

```rust
// Inside src/lib.rs
mod engine;
mod storage;

// Expose a flat, clean interface to the outside world
pub use engine::Engine;
pub use storage::StorageBackend;
```

### Module Directory Hierarchy
Prefer the modern 2018 edition module style (file-based over nested ```mod.rs```):
```text
src/
├── lib.rs
├── config.rs
├── engine.rs
└── engine/
    ├── pipeline.rs
    └── parser.rs
```
- Keep individual source files small (< 400 lines). If a module grows too large, convert it into a folder and split responsibilities into submodules.

### Cargo Workspaces & Modular Boundaries
For medium-to-large applications, avoid monolithic crates. Break the codebase into a Cargo Workspace containing focused crates:
- ```crates/core```: Pure domain logic and models (no heavy dependencies).
- ```crates/db```: Database models and migrations.
- ```crates/api```: HTTP/gRPC handlers.
- **Benefits**: Faster incremental compilation (Rust compiles crates in parallel), enforced structural decoupling, and isolated unit testing.

### Dependency Hygiene
- Audit dependencies using ```cargo tree``` to catch duplicate crate versions or unnecessary transitive weight.
- Use feature flags (```[features]```) to keep default builds lean and avoid compiling unused features.

> **Official References:**
> - [The Rust Programming Language — Chapter 7: Managing Growing Projects with Packages, Crates, and Modules](https://doc.rust-lang.org/book/ch07-00-managing-growing-projects-with-packages-crates-and-modules.html)
> - [The Cargo Book — Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
> - [Rust API Guidelines — Module Hierarchy (```C-MODULE```)](https://rust-lang.github.io/api-guidelines/naming.html#module-names-follow-snake_case-c-module)

---

## 7. Code Quality, Safety & Tooling

Run these tools regularly during local development and enforce them inside CI/CD pipelines:

```bash
# Code formatting check
cargo fmt --check

# Static analysis and linting (fail on warnings)
cargo clippy -- -D warnings

# Run unit tests and embedded doc tests
cargo test
```

> **Official References:**
> - [The Cargo Book — Continuous Integration](https://doc.rust-lang.org/cargo/guide/continuous-integration.html)
> - [Clippy Documentation](https://doc.rust-lang.org/clippy/)
