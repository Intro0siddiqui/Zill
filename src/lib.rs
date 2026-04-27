//! Zill is a deterministic Bash-like environment for LLMs that operates 100% in-memory.
//!
//! It provides core POSIX builtins along with high-performance `ripgrep` and `fd`
//! integration, ensuring no host disk or process syscalls are made.

#![forbid(unsafe_code)]

pub mod fs;
pub mod session;
pub mod builtins;
pub mod error;
pub mod limits;

pub use session::ZillSession;
pub use limits::ZillLimits;
pub use session::CmdOutput;
