# Appendix: Getting Started with Rust

Open Choice plugins are Rust binaries. You don't need to be a Rust expert — a plugin is a straightforward command-line program with no async code, no lifetimes beyond the basics, and a small dependency surface. This appendix covers what you actually need to know and where to learn it.

## Installing Rust

Rust is installed through `rustup`, the official toolchain manager. It handles compiler versions, cross-compilation targets, and component updates.

**Install on macOS / Linux:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Install on Windows:**

Download and run [rustup-init.exe](https://rustup.rs). Accept the defaults. You will also need the [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) — select "Desktop development with C++" during installation.

After installation, open a new terminal and verify:
```bash
rustc --version
cargo --version
```

Both should print version numbers. If they don't, close and reopen your terminal.

### Keeping Rust up to date

```bash
rustup update
```

Run this occasionally. The ocp-sdk targets stable Rust; no nightly features are required.

---

## Cargo basics

Cargo is Rust's build tool and package manager. Everything you need is one command away.

| Command | What it does |
|---------|-------------|
| `cargo new --bin my-plugin` | Create a new binary project |
| `cargo build` | Compile in debug mode (fast compile, slow binary) |
| `cargo build --release` | Compile in release mode (slow compile, fast binary) — use this for packaging |
| `cargo run -- <args>` | Build and run, passing `<args>` to your binary |
| `cargo test` | Run all tests |
| `cargo check` | Type-check without producing a binary — fastest way to catch errors |
| `cargo add serde_json` | Add a dependency (requires `cargo-edit`: `cargo install cargo-edit`) |

Dependencies live in `Cargo.toml`. When you add one, Cargo downloads it from [crates.io](https://crates.io) automatically on the next build.

---

## What you need to know for plugin development

Writing a plugin uses a small, stable subset of Rust. Here is what actually matters, in order of importance.

### 1. Reading command-line arguments

```rust
let args: Vec<String> = std::env::args().collect();
// args[0] is the binary name
// args[1] is the first argument, etc.
```

That's all you need. No argument-parsing crate is required for a plugin.

### 2. `Result` and the `?` operator

Almost everything that can fail in Rust returns a `Result<T, E>`. The `?` operator propagates errors up the call stack:

```rust
fn read_task_file(path: &str) -> Result<String, std::io::Error> {
    let content = std::fs::read_to_string(path)?;  // returns Err if file missing
    Ok(content)
}
```

For plugins, using `anyhow::Result` (from the `anyhow` crate) is the easiest approach — it lets you mix error types freely:

```rust
use anyhow::Result;

fn run(path: &str, task_id: &str) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let doc: serde_json::Value = serde_json::from_str(&content)?;
    // ...
    Ok(())
}

fn main() {
    if let Err(e) = run("task.json", "my-task") {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
```

### 3. JSON with `serde_json`

Your plugin reads a JSON task file and writes JSON events. `serde_json` handles both.

**Reading untyped JSON:**
```rust
use serde_json::Value;

let doc: Value = serde_json::from_str(&content)?;
let message = doc["tasks"][0]["params"]["message"].as_str().unwrap_or("");
```

**Serializing a struct to JSON:**
```rust
use serde::Serialize;

#[derive(Serialize)]
struct MyEvent {
    event: String,
    message: String,
}

let event = MyEvent { event: "started".to_string(), message: "hello".to_string() };
println!("{}", serde_json::to_string(&event)?);
```

The `ocp-types` crate already defines all the event structs — you rarely need to write your own.

### 4. Writing files

```rust
use std::fs;

fs::create_dir_all("./outputs")?;
fs::write("./outputs/result.txt", "hello")?;
```

That's it. For appending to a log file:
```rust
use std::fs::OpenOptions;
use std::io::Write;

let mut file = OpenOptions::new().create(true).append(true).open("run.log")?;
writeln!(file, "step completed")?;
```

### 5. Spawning external processes (wrapper plugins only)

If your plugin wraps an external tool like `julia` or `Rscript`:

```rust
use std::process::Command;

let output = Command::new("julia")
    .arg("analysis.jl")
    .arg("--verbose")
    .output()?;

let stdout = String::from_utf8_lossy(&output.stdout);
let stderr = String::from_utf8_lossy(&output.stderr);
let success = output.status.success();
```

For streaming output line by line (rather than waiting for the process to finish):
```rust
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

let mut child = Command::new("julia")
    .arg("analysis.jl")
    .stdout(Stdio::piped())
    .spawn()?;

let stdout = child.stdout.take().unwrap();
for line in BufReader::new(stdout).lines() {
    let line = line?;
    // emit a Progress or Warning event for each line
}

let status = child.wait()?;
```

### 6. Ownership — just enough to not get stuck

Rust's ownership system is its most distinctive feature. For plugin development you will encounter two common patterns:

**`.clone()` when you need to use a value twice:**
```rust
let run_id = uuid::Uuid::new_v4().to_string();
emit_event(&run_id);       // borrows run_id
emit_finish(run_id.clone()); // would move run_id, so clone it
```

**`.to_string()` to convert `&str` to `String`:**
```rust
let s: &str = "hello";
let owned: String = s.to_string();
```

If the compiler complains about "cannot move out of" or "borrow of moved value", adding `.clone()` is usually the right fix for plugin code. Don't worry about optimising clones until your plugin is working correctly.

---

## Resources

### For learning Rust

**[The Rust Programming Language](https://doc.rust-lang.org/book/)** ("The Book")
The official, free, well-written introduction to Rust. Read chapters 1–9 for a solid foundation. Chapter 6 (enums and pattern matching) and Chapter 9 (error handling) are the most directly relevant to plugin development.

**[Rust by Example](https://doc.rust-lang.org/rust-by-example/)**
A companion to The Book — same concepts, shown as runnable code examples. Useful for quickly looking up syntax.

**[Rustlings](https://github.com/rust-lang/rustlings)**
Small exercises that teach Rust interactively. A good complement to The Book if you prefer learning by doing. Install with:
```bash
cargo install rustlings
rustlings init
```

**[Exercism — Rust track](https://exercism.org/tracks/rust)**
Practice exercises with mentor feedback. Good for building fluency after you've read The Book.

### For reference while writing code

**[docs.rs](https://docs.rs)**
Automatically generated API documentation for every crate on crates.io. The canonical reference for `ocp-types`, `serde_json`, `anyhow`, and any other crate you use. Search for a crate name to find its docs.

**[The Rust Standard Library](https://doc.rust-lang.org/std/)**
Full documentation for `std::fs`, `std::process::Command`, `std::io`, and everything else in the standard library.

**[The Cargo Book](https://doc.rust-lang.org/cargo/)**
Reference for `Cargo.toml` fields, workspaces, build scripts, and the `cargo` command.

### For getting unstuck

**[The Rust users forum](https://users.rust-lang.org)**
Active community. Good for questions that are too specific for Stack Overflow.

**[Rust Discord](https://discord.gg/rust-lang)**
The `#beginners` channel has fast, helpful responses.

**[Stack Overflow — rust tag](https://stackoverflow.com/questions/tagged/rust)**
Good coverage of common patterns. Most ownership-related questions have been answered.

---

## Specific topics by plugin use case

| You want to... | Read about... | Resource |
|----------------|--------------|----------|
| Parse JSON task files | `serde_json::Value`, indexing | [serde_json docs](https://docs.rs/serde_json) |
| Deserialize into a struct | `#[derive(Deserialize)]`, `serde::Deserialize` | The Book ch. 10; [serde docs](https://docs.rs/serde) |
| Handle errors cleanly | `anyhow::Result`, `?` operator | [anyhow docs](https://docs.rs/anyhow); The Book ch. 9 |
| Write output files | `std::fs::write`, `std::fs::create_dir_all` | [std::fs docs](https://doc.rust-lang.org/std/fs/) |
| Spawn external processes | `std::process::Command`, `Stdio::piped` | [std::process docs](https://doc.rust-lang.org/std/process/) |
| Generate UUIDs | `uuid::Uuid::new_v4()` | [uuid docs](https://docs.rs/uuid) |
| Get current timestamp | `chrono::Utc::now()` | [chrono docs](https://docs.rs/chrono) |
| Compute SHA-256 | `sha2::Sha256`, `hex::encode` | [sha2 docs](https://docs.rs/sha2) |
| Cross-compile for other platforms | `rustup target add`, `cargo build --target` | [The Cargo Book: cross-compilation](https://doc.rust-lang.org/cargo/reference/config.html#targettriplelinker) |
