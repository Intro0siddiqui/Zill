use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};
use crate::fs::VirtualFs;
use crate::error::ZillError;
use crate::limits::ZillLimits;
use crate::parser::{AstNode, Parser, Redirection as AstRedirection, LogicalOperator};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::io;

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
    pub variables: HashMap<String, String>,
    pub limits: ZillLimits,
    #[serde(skip)]
    _not_sync: PhantomData<*const ()>, // Mark !Sync
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
            variables: HashMap::new(),
            limits,
            _not_sync: PhantomData,
        }
    }

    /// Runs a command string and returns its output.
    pub fn run(&mut self, input: &str) -> CmdOutput {
        let mut parser = match Parser::new(input) {
            Ok(p) => p,
            Err(e) => return CmdOutput::error(format!("zill: parse error: {}", e), 1),
        };
        let ast = match parser.parse() {
            Ok(ast) => ast,
            Err(e) => return CmdOutput::error(format!("zill: parse error: {}", e), 1),
        };

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut stdin = io::empty();

        let exit_code = match self.execute_node(&ast, &mut stdin, &mut stdout, &mut stderr) {
            Ok(code) => code,
            Err(e) => {
                return CmdOutput::error(e.to_string(), 1);
            }
        };

        CmdOutput {
            stdout: String::from_utf8_lossy(&stdout).to_string(),
            stderr: String::from_utf8_lossy(&stderr).to_string(),
            exit_code,
        }
    }

    fn execute_node(
        &mut self,
        node: &AstNode,
        stdin: &mut dyn io::Read,
        stdout: &mut dyn io::Write,
        stderr: &mut dyn io::Write,
    ) -> Result<i32, ZillError> {
        match node {
            AstNode::Command { argv, redirects } => {
                let mut expanded_argv = Vec::new();
                for arg in argv {
                    expanded_argv.push(self.expand_variables(arg));
                }

                if expanded_argv.is_empty() {
                    return Ok(0);
                }

                // Handle variable assignment if it's like VAR=VAL
                if expanded_argv.len() == 1 && expanded_argv[0].contains('=') {
                    let parts: Vec<&str> = expanded_argv[0].splitn(2, '=').collect();
                    self.variables.insert(parts[0].to_string(), parts[1].to_string());
                    return Ok(0);
                }

                let cmd_name = expanded_argv[0].clone();
                let cmd_args = expanded_argv[1..].to_vec();

                let mut current_stdin_buf: Vec<u8> = Vec::new();
                stdin.read_to_end(&mut current_stdin_buf).map_err(|e| ZillError::Generic(e.to_string()))?;
                let mut current_stdin: Box<dyn io::Read> = Box::new(io::Cursor::new(current_stdin_buf));

                let mut out_redirect = None;

                for redirect in redirects {
                    match redirect {
                        AstRedirection::Stdin(path) => {
                            let canonical = self.vfs.canonicalize(Path::new(path), &self.cwd);
                            let content = self.vfs.read(&canonical)?;
                            current_stdin = Box::new(io::Cursor::new(content.to_vec()));
                        }
                        AstRedirection::StdoutOverwrite(path) => {
                            let canonical = self.vfs.canonicalize(Path::new(path), &self.cwd);
                            out_redirect = Some((canonical, false));
                        }
                        AstRedirection::StdoutAppend(path) => {
                            let canonical = self.vfs.canonicalize(Path::new(path), &self.cwd);
                            out_redirect = Some((canonical, true));
                        }
                    }
                }

                let result = if let Some((path, append)) = out_redirect {
                    let mut writer = Vec::new();
                    let res = self.execute_builtin(&cmd_name, &cmd_args, &mut *current_stdin, &mut writer, stderr)?;
                    if append {
                        let mut existing = match self.vfs.read(&path) {
                            Ok(d) => d.to_vec(),
                            Err(ZillError::NotFound(_)) => Vec::new(),
                            Err(e) => return Err(e),
                        };
                        existing.extend_from_slice(&writer);
                        self.vfs.write(&path, existing)?;
                    } else {
                        self.vfs.write(&path, writer)?;
                    }
                    res
                } else {
                    self.execute_builtin(&cmd_name, &cmd_args, &mut *current_stdin, stdout, stderr)?
                };

                Ok(result)
            }
            AstNode::Pipeline { nodes } => {
                let mut last_exit_code = 0;
                let mut current_input: Vec<u8> = Vec::new();

                for (i, node) in nodes.iter().enumerate() {
                    let mut next_stdout = Vec::new();
                    if i == 0 {
                        last_exit_code = self.execute_node(node, stdin, &mut next_stdout, stderr)?;
                    } else if i == nodes.len() - 1 {
                        let mut reader = io::Cursor::new(current_input.clone());
                        last_exit_code = self.execute_node(node, &mut reader, stdout, stderr)?;
                    } else {
                        let mut reader = io::Cursor::new(current_input.clone());
                        last_exit_code = self.execute_node(node, &mut reader, &mut next_stdout, stderr)?;
                    }
                    current_input = next_stdout;
                }
                Ok(last_exit_code)
            }
            AstNode::Sequence { nodes } => {
                let mut last_exit_code = 0;
                for node in nodes {
                    last_exit_code = self.execute_node(node, stdin, stdout, stderr)?;
                }
                Ok(last_exit_code)
            }
            AstNode::Logical { left, right, operator } => {
                let left_exit = self.execute_node(left, stdin, stdout, stderr)?;
                match operator {
                    LogicalOperator::And => {
                        if left_exit == 0 {
                            self.execute_node(right, stdin, stdout, stderr)
                        } else {
                            Ok(left_exit)
                        }
                    }
                    LogicalOperator::Or => {
                        if left_exit != 0 {
                            self.execute_node(right, stdin, stdout, stderr)
                        } else {
                            Ok(left_exit)
                        }
                    }
                }
            }
            AstNode::Subshell { node } => {
                let old_cwd = self.cwd.clone();
                let old_variables = self.variables.clone();
                let result = self.execute_node(node, stdin, stdout, stderr);
                self.cwd = old_cwd;
                self.variables = old_variables;
                result
            }
            AstNode::If { condition, then_part, else_part } => {
                let mut cond_stdout = Vec::new();
                let mut cond_stderr = Vec::new();
                let cond_exit = self.execute_node(condition, &mut io::empty(), &mut cond_stdout, &mut cond_stderr)?;
                if cond_exit == 0 {
                    self.execute_node(then_part, stdin, stdout, stderr)
                } else if let Some(else_node) = else_part {
                    self.execute_node(else_node, stdin, stdout, stderr)
                } else {
                    Ok(0)
                }
            }
            AstNode::For { variable, items, body } => {
                let mut last_exit_code = 0;
                for item in items {
                    self.variables.insert(variable.clone(), item.clone());
                    last_exit_code = self.execute_node(body, stdin, stdout, stderr)?;
                }
                Ok(last_exit_code)
            }
        }
    }

    fn expand_variables(&self, input: &str) -> String {
        let mut result = String::new();
        let mut chars = input.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '$' {
                let mut name = String::new();
                let mut braced = false;
                if let Some(&'{') = chars.peek() {
                    chars.next();
                    braced = true;
                }

                while let Some(&nc) = chars.peek() {
                    if nc.is_alphanumeric() || nc == '_' {
                        name.push(nc);
                        chars.next();
                    } else {
                        break;
                    }
                }

                if braced && chars.peek() == Some(&'}') {
                    chars.next();
                }

                if let Some(val) = self.variables.get(&name) {
                    result.push_str(val);
                } else if let Some(val) = self.env.get(&name) {
                    result.push_str(val);
                } else if name.is_empty() {
                    result.push('$');
                }
            } else {
                result.push(c);
            }
        }
        result
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

    fn execute_builtin(
        &mut self,
        name: &str,
        args: &[String],
        stdin: &mut dyn io::Read,
        stdout: &mut dyn io::Write,
        stderr: &mut dyn io::Write,
    ) -> Result<i32, ZillError> {
        match name {
            "pwd" => self.builtin_pwd(args, stdin, stdout, stderr),
            "cd" => self.builtin_cd(args, stdin, stdout, stderr),
            "ls" => self.builtin_ls(args, stdin, stdout, stderr),
            "cat" => self.builtin_cat(args, stdin, stdout, stderr),
            "echo" => self.builtin_echo(args, stdin, stdout, stderr),
            "mkdir" => self.builtin_mkdir(args, stdin, stdout, stderr),
            "touch" => self.builtin_touch(args, stdin, stdout, stderr),
            "rm" => self.builtin_rm(args, stdin, stdout, stderr),
            "rg" => self.builtin_rg(args, stdin, stdout, stderr),
            "fd" => self.builtin_fd(args, stdin, stdout, stderr),
            "true" => Ok(0),
            "false" => Ok(1),
            ":" => Ok(0),
            _ => {
                writeln!(stderr, "{}: command not found", name).map_err(|e| ZillError::Generic(e.to_string()))?;
                Ok(127)
            }
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
    fn test_pipeline() {
        let mut session = ZillSession::new();
        session.run("echo hello | cat > /file.txt");
        let out = session.run("cat /file.txt");
        assert_eq!(out.stdout, "hello\n");
    }

    #[test]
    fn test_variables() {
        let mut session = ZillSession::new();
        session.run("GREETING=hello");
        let out = session.run("echo $GREETING world");
        assert_eq!(out.stdout, "hello world\n");
    }

    #[test]
    fn test_logical() {
        let mut session = ZillSession::new();
        let out = session.run("echo a && echo b");
        assert_eq!(out.stdout, "a\nb\n");

        let out = session.run("false || echo c");
        assert!(out.stdout.contains("c"));

        let out = session.run("nonexistent || echo d");
        assert!(out.stderr.contains("command not found"));
        assert!(out.stdout.contains("d"));
    }

    #[test]
    fn test_if() {
        let mut session = ZillSession::new();
        let out = session.run("if true; then echo yes; fi");
        assert_eq!(out.stdout, "yes\n");

        let out = session.run("if false; then echo yes; else echo no; fi");
        assert_eq!(out.stdout, "no\n");
    }

    #[test]
    fn test_for() {
        let mut session = ZillSession::new();
        let out = session.run("for i in a b c; do echo $i; done");
        assert_eq!(out.stdout, "a\nb\nc\n");
    }

    #[test]
    fn test_subshell_isolation() {
        let mut session = ZillSession::new();
        session.run("(FOO=bar; cd /test)");
        let out = session.run("echo $FOO");
        assert_eq!(out.stdout, "\n");
        assert_eq!(session.cwd, PathBuf::from("/"));
    }

    #[test]
    fn test_variable_expansion_refined() {
        let mut session = ZillSession::new();
        session.run("V=1; VAR=2");
        let out = session.run("echo $V $VAR");
        assert_eq!(out.stdout, "1 2\n");
    }

    #[test]
    fn test_quoted_meta() {
        let mut session = ZillSession::new();
        let out = session.run("echo \"a|b\"");
        assert_eq!(out.stdout, "a|b\n");
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

        let out = session.run("fd -H .hidden");
        assert!(out.stdout.contains(".hidden"));
    }
}
