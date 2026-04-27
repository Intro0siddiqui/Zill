# Zill

Zill is a high-performance, deterministic, and 100% in-memory Bash-like environment designed for LLM agents. It provides the power of `ripgrep` and `fd` in a strictly isolated sandbox.

## Features

- **Isolated Virtual FS**: No `std::fs` or process syscalls. Fully safe and isolated.
- **High Performance**: Memory-backed storage and search.
- **ripgrep & fd Power**: Built-in support for recursive search and file finding with `.gitignore` awareness.
- **Deterministic**: Guaranteed byte-for-byte identical output across sessions and serialization roundtrips.
- **Resource Controlled**: Configurable limits on file sizes, node counts, and search outputs.
- **Agent Friendly**: Human-readable nested JSON serialization of the session state.

## Architecture

Zill is built in three layers:
1. **VirtualFS**: A flat `HashMap<PathBuf, Node>` for O(1) access, with metadata for directories maintaining children sets for fast listing.
2. **Session**: Manages current working directory, environment variables, and resource limits.
3. **Builtins**: POSIX-compliant implementations of common shell utilities.

## Usage

### Library

```rust
use zill::{ZillSession, ZillLimits};

fn main() {
    let mut session = ZillSession::new();

    session.run("mkdir -p /src");
    session.run("echo 'fn main() {}' > /src/main.rs");

    let output = session.run("rg main /src");
    println!("{}", output.stdout);
}
```

### REPL

You can run an interactive shell for manual testing:

```bash
cargo run
```

### Examples

- `cargo run --example agent_loop`: Demonstrates a typical agent command sequence.
- `cargo run --example eval`: Evaluates search accuracy and performance on a synthetic dataset.

## Safety

Zill enforces `#![forbid(unsafe_code)]` at the crate level to guarantee memory safety and ensure no syscalls slip in through dependencies.
