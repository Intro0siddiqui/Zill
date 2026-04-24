use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZillLimits {
    pub max_nodes: usize,
    pub max_file_size: usize,
    pub max_match_count: u64,
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
