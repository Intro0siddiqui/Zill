use thiserror::Error;

/// Error types returned by Zill operations.
///
/// Most error messages match GNU coreutils wording.
#[derive(Error, Debug, Clone)]
pub enum ZillError {
    #[error("ls: {0}: No such file or directory")]
    NotFound(String),
    #[error("ls: {0}: Not a directory")]
    NotADirectory(String),
    #[error("cat: {0}: Is a directory")]
    IsADirectory(String),
    #[error("mkdir: cannot create directory ‘{0}’: File exists")]
    FileExists(String),
    #[error("rm: cannot remove ‘{0}’: No such file or directory")]
    RmNotFound(String),
    #[error("rm: cannot remove ‘{0}’: Is a directory")]
    RmIsDirectory(String),
    #[error("rm: cannot remove ‘{0}’: Directory not empty")]
    DirectoryNotEmpty(String),
    #[error("zill: {0}: permission denied")]
    PermissionDenied(String),
    #[error("zill: file too large")]
    FileTooLarge,
    #[error("zill: disk full")]
    DiskFull,
    #[error("zill: invalid path: {0}")]
    InvalidPath(String),
    #[error("zill: {0}")]
    Generic(String),
}
