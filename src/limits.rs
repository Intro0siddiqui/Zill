use serde::{Serialize, Deserialize};

/// Resource limits for a Zill session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZillLimits {
    /// Maximum number of nodes (files/dirs) in the VFS.
    pub max_nodes: usize,
    /// Maximum size of a single file in bytes.
    pub max_file_size: usize,
    /// Maximum number of matches for a single `rg` command.
    pub max_match_count: u64,
    /// Maximum size of the output for a single command in bytes.
    pub max_output_size: usize,
}

impl Default for ZillLimits {
    fn default() -> Self {
        ZillLimits {
            max_nodes: 10000,
            max_file_size: 1024 * 1024, // 1MB
            max_match_count: 1000,
            max_output_size: 100 * 1024, // 100KB
        }
    }
}
