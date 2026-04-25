use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};
use crate::fs::VirtualFs;
use crate::error::ZillError;
use crate::limits::ZillLimits;
use std::collections::HashMap;
use std::marker::PhantomData;

/// Result of a command execution in the Zill shell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmdOutput {
    /// Text written to standard output.
    pub stdout: String,
    /// Text written to standard error.
    pub stderr: String,
    /// Process exit code (0 for success).
    pub exit_code: i32,
}

impl CmdOutput {
    /// Creates a successful command output with the given stdout.
    pub fn success(stdout: String) -> Self {
        CmdOutput {
            stdout,
            stderr: String::new(),
            exit_code: 0,
        }
    }

    /// Creates an error command output with the given stderr and exit code.
    pub fn error(stderr: String, exit_code: i32) -> Self {
        CmdOutput {
            stdout: String::new(),
            stderr,
            exit_code,
        }
    }
}

/// A shell session that manages working directory, environment, and the virtual file system.
///
/// `ZillSession` is !Sync to optimize for single-threaded usage.
#[derive(Serialize, Deserialize)]
pub struct ZillSession {
    #[serde(serialize_with = "VirtualFs::serialize_nested", deserialize_with = "VirtualFs::deserialize_nested")]
    pub vfs: VirtualFs,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub limits: ZillLimits,
    #[serde(skip)]
    _not_sync: PhantomData<*const ()>, // Mark !Sync
}

pub enum Redirection {
    Overwrite(PathBuf),
    Append(PathBuf),
}

pub struct ParsedCommand {
    pub argv: Vec<String>,
    pub redirect: Option<Redirection>,
}

impl ZillSession {
    /// Creates a new session with default limits.
    pub fn new() -> Self {
        Self::with_limits(ZillLimits::default())
    }

    /// Creates a new session with custom resource limits.
    pub fn with_limits(limits: ZillLimits) -> Self {
        ZillSession {
            vfs: VirtualFs::new(limits.max_nodes, limits.max_file_size),
            cwd: PathBuf::from("/"),
            env: HashMap::new(),
            limits,
            _not_sync: PhantomData,
        }
    }

    /// Runs a command string and returns its output.
    pub fn run(&mut self, input: &str) -> CmdOutput {
        let parsed = match self.parse_input(input) {
            Ok(p) => p,
            Err(e) => return CmdOutput::error(format!("zill: parse error: {}", e), 1),
        };

        if parsed.argv.is_empty() {
            return CmdOutput::success(String::new());
        }

        let cmd_name = &parsed.argv[0];
        let result = self.execute_builtin(cmd_name, &parsed.argv[1..]);

        match result {
            Ok(mut output) => {
                if let Some(redirection) = parsed.redirect {
                    if let Err(e) = self.handle_redirection(&mut output, redirection) {
                        return CmdOutput::error(e.to_string(), 1);
                    }
                }
                output
            }
            Err(e) => CmdOutput::error(e.to_string(), 1),
        }
    }

    fn parse_input(&self, input: &str) -> Result<ParsedCommand, String> {
        let words = shell_words::split(input).map_err(|e| e.to_string())?;
        let mut argv = Vec::new();
        let mut redirect = None;
        let mut iter = words.into_iter();

        while let Some(word) = iter.next() {
            match word.as_str() {
                ">" => {
                    if let Some(path) = iter.next() {
                        redirect = Some(Redirection::Overwrite(self.vfs.canonicalize(Path::new(&path), &self.cwd)));
                    } else {
                        return Err("syntax error near unexpected token `newline'".into());
                    }
                }
                ">>" => {
                    if let Some(path) = iter.next() {
                        redirect = Some(Redirection::Append(self.vfs.canonicalize(Path::new(&path), &self.cwd)));
                    } else {
                        return Err("syntax error near unexpected token `newline'".into());
                    }
                }
                _ => {
                    argv.push(word);
                }
            }
        }

        Ok(ParsedCommand { argv, redirect })
    }

    fn handle_redirection(&mut self, output: &mut CmdOutput, redirection: Redirection) -> Result<(), ZillError> {
        let content = output.stdout.as_bytes().to_vec();
        match redirection {
            Redirection::Overwrite(path) => {
                self.vfs.write(&path, content)?;
            }
            Redirection::Append(path) => {
                let mut existing = match self.vfs.read(&path) {
                    Ok(data) => data.to_vec(),
                    Err(ZillError::NotFound(_)) => Vec::new(),
                    Err(e) => return Err(e),
                };
                existing.extend_from_slice(&content);
                self.vfs.write(&path, existing)?;
            }
        }
        output.stdout.clear();
        Ok(())
    }

    /// Serializes the session to a human-readable nested JSON format.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self)
    }

    /// Deserializes a session from its JSON representation.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let mut session: Self = serde_json::from_str(json)?;
        session._not_sync = PhantomData;
        Ok(session)
    }

    fn execute_builtin(&mut self, name: &str, args: &[String]) -> Result<CmdOutput, ZillError> {
        match name {
            "pwd" => self.builtin_pwd(args),
            "cd" => self.builtin_cd(args),
            "ls" => self.builtin_ls(args),
            "cat" => self.builtin_cat(args),
            "echo" => self.builtin_echo(args),
            "mkdir" => self.builtin_mkdir(args),
            "touch" => self.builtin_touch(args),
            "rm" => self.builtin_rm(args),
            "rg" => self.builtin_rg(args),
            "fd" => self.builtin_fd(args),
            _ => Err(ZillError::Generic(format!("{}: command not found", name))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_basic() {
        let mut session = ZillSession::new();
        let out = session.run("pwd");
        assert_eq!(out.stdout, "/\n");

        session.run("mkdir /test");
        let out = session.run("ls /");
        assert!(out.stdout.contains("test"));
    }

    #[test]
    fn test_redirection() {
        let mut session = ZillSession::new();
        session.run("echo hello > /file.txt");
        let out = session.run("cat /file.txt");
        assert_eq!(out.stdout, "hello\n");

        session.run("echo world >> /file.txt");
        let out = session.run("cat /file.txt");
        assert_eq!(out.stdout, "hello\nworld\n");
    }

    #[test]
    fn test_serialization() {
        let mut session = ZillSession::new();
        session.run("mkdir /test");
        session.run("echo 'hello' > /test/hi.txt");
        session.cwd = PathBuf::from("/test");

        let json = session.to_json().unwrap();
        let mut session2 = ZillSession::from_json(&json).unwrap();

        assert_eq!(session2.cwd, PathBuf::from("/test"));
        let out = session2.run("cat hi.txt");
        assert_eq!(out.stdout, "hello\n");
    }

    #[test]
    fn test_rg() {
        let mut session = ZillSession::new();
        session.run("echo 'hello world' > f1.txt");
        session.run("echo 'goodbye world' > f2.txt");
        session.run("echo 'hello again' > f3.txt");

        let out = session.run("rg hello");
        assert!(out.stdout.contains("f1.txt:hello world"));
        assert!(out.stdout.contains("f3.txt:hello again"));
        assert!(!out.stdout.contains("f2.txt"));

        let out = session.run("rg -c world");
        assert!(out.stdout.contains("f1.txt:1"));
        assert!(out.stdout.contains("f2.txt:1"));
    }

    #[test]
    fn test_fd() {
        let mut session = ZillSession::new();
        session.run("mkdir -p /a/b/c");
        session.run("touch /a/b/f1.txt");
        session.run("touch /a/b/c/f2.rs");
        session.run("touch /.hidden");

        let out = session.run("fd");
        assert!(out.stdout.contains("/a"));
        assert!(out.stdout.contains("/a/b"));
        assert!(out.stdout.contains("/a/b/f1.txt"));
        assert!(out.stdout.contains("/a/b/c/f2.rs"));
        assert!(!out.stdout.contains(".hidden"));

        let out = session.run("fd -e rs");
        assert!(!out.stdout.contains("f1.txt"));
        assert!(out.stdout.contains("f2.rs"));

        let out = session.run("fd f1");
        assert!(out.stdout.contains("f1.txt"));
        assert!(!out.stdout.contains("f2.rs"));
    }
}
