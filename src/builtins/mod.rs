pub mod rg;
pub mod fd;

use crate::session::ZillSession;
use crate::error::ZillError;
use crate::fs::Node;
use clap::Parser;
use std::path::Path;
use std::io;

#[derive(Parser)]
#[command(no_binary_name = true)]
struct LsArgs {
    #[arg(short = 'a')]
    all: bool,
    #[arg(short = 'l')]
    long: bool,
    #[arg(short = '1')]
    one_per_line: bool,
    paths: Vec<String>,
}

#[derive(Parser)]
#[command(no_binary_name = true)]
struct CatArgs {
    #[arg(short = 'n')]
    number: bool,
    paths: Vec<String>,
}

#[derive(Parser)]
#[command(no_binary_name = true)]
struct MkdirArgs {
    #[arg(short = 'p')]
    parents: bool,
    paths: Vec<String>,
}

impl ZillSession {
    /// Prints the current working directory to stdout.
    pub fn builtin_pwd(
        &self,
        _args: &[String],
        _stdin: &mut dyn io::Read,
        stdout: &mut dyn io::Write,
        _stderr: &mut dyn io::Write,
    ) -> Result<i32, ZillError> {
        writeln!(stdout, "{}", self.cwd.display()).map_err(|e| ZillError::Generic(e.to_string()))?;
        Ok(0)
    }

    /// Changes the current working directory.
    pub fn builtin_cd(
        &mut self,
        args: &[String],
        _stdin: &mut dyn io::Read,
        _stdout: &mut dyn io::Write,
        _stderr: &mut dyn io::Write,
    ) -> Result<i32, ZillError> {
        let target = args.get(0).map(|s| s.as_str()).unwrap_or("/");
        let new_path = self.vfs.canonicalize(Path::new(target), &self.cwd);

        let node = self.vfs.stat(&new_path)?;
        if node.is_dir() {
            self.cwd = new_path;
            Ok(0)
        } else {
            Err(ZillError::NotADirectory(target.to_string()))
        }
    }

    /// Lists directory contents.
    pub fn builtin_ls(
        &self,
        args: &[String],
        _stdin: &mut dyn io::Read,
        stdout: &mut dyn io::Write,
        stderr: &mut dyn io::Write,
    ) -> Result<i32, ZillError> {
        let cli = match LsArgs::try_parse_from(args) {
            Ok(cli) => cli,
            Err(e) => {
                writeln!(stderr, "{}", e).map_err(|e| ZillError::Generic(e.to_string()))?;
                return Ok(1);
            }
        };

        let paths = if cli.paths.is_empty() {
            vec![".".to_string()]
        } else {
            cli.paths
        };

        for (i, path_str) in paths.iter().enumerate() {
            if paths.len() > 1 {
                writeln!(stdout, "{}:", path_str).map_err(|e| ZillError::Generic(e.to_string()))?;
            }
            let canonical = self.vfs.canonicalize(Path::new(path_str), &self.cwd);
            let node = self.vfs.stat(&canonical)?;

            match node {
                Node::Directory(meta) => {
                    let mut entries: Vec<String> = meta.children.iter().cloned().collect();
                    if cli.all {
                        entries.push(".".into());
                        entries.push("..".into());
                    }
                    entries.sort();

                    for entry in entries {
                        if cli.long {
                            let entry_path = if entry == "." {
                                canonical.clone()
                            } else if entry == ".." {
                                canonical.parent().unwrap_or(Path::new("/")).to_path_buf()
                            } else {
                                canonical.join(&entry)
                            };
                            let entry_node = self.vfs.stat(&entry_path)?;
                            let type_char = if entry_node.is_dir() { 'd' } else { '-' };
                            let (size, date) = match entry_node {
                                Node::File(m) => (m.size, m.modified_at),
                                Node::Directory(m) => (4096, m.modified_at),
                            };
                            writeln!(
                                stdout,
                                "{}rwxr-xr-x 1 zill zill {:>8} {} {}",
                                type_char,
                                size,
                                date.format("%b %d %H:%M"),
                                entry
                            )
                            .map_err(|e| ZillError::Generic(e.to_string()))?;
                        } else if cli.one_per_line {
                            writeln!(stdout, "{}", entry).map_err(|e| ZillError::Generic(e.to_string()))?;
                        } else {
                            write!(stdout, "{}  ", entry).map_err(|e| ZillError::Generic(e.to_string()))?;
                        }
                    }
                    if !cli.long && !cli.one_per_line {
                        writeln!(stdout).map_err(|e| ZillError::Generic(e.to_string()))?;
                    }
                }
                Node::File(_) => {
                    writeln!(stdout, "{}", path_str).map_err(|e| ZillError::Generic(e.to_string()))?;
                }
            }
            if i < paths.len() - 1 {
                writeln!(stdout).map_err(|e| ZillError::Generic(e.to_string()))?;
            }
        }

        Ok(0)
    }

    /// Concatenates files and prints them to stdout.
    ///
    /// If no files are provided, it reads from stdin.
    pub fn builtin_cat(
        &self,
        args: &[String],
        stdin: &mut dyn io::Read,
        stdout: &mut dyn io::Write,
        stderr: &mut dyn io::Write,
    ) -> Result<i32, ZillError> {
        let cli = match CatArgs::try_parse_from(args) {
            Ok(cli) => cli,
            Err(e) => {
                writeln!(stderr, "{}", e).map_err(|e| ZillError::Generic(e.to_string()))?;
                return Ok(1);
            }
        };

        if cli.paths.is_empty() {
            io::copy(stdin, stdout).map_err(|e| ZillError::Generic(e.to_string()))?;
        } else {
            for path_str in cli.paths {
                let path = self.vfs.canonicalize(Path::new(&path_str), &self.cwd);
                let content = self.vfs.read(&path)?;
                let text = String::from_utf8_lossy(content);
                if cli.number {
                    for (i, line) in text.lines().enumerate() {
                        writeln!(stdout, "{:>6}  {}", i + 1, line).map_err(|e| ZillError::Generic(e.to_string()))?;
                    }
                } else {
                    write!(stdout, "{}", text).map_err(|e| ZillError::Generic(e.to_string()))?;
                }
            }
        }
        Ok(0)
    }

    /// Prints the given arguments to stdout.
    pub fn builtin_echo(
        &self,
        args: &[String],
        _stdin: &mut dyn io::Read,
        stdout: &mut dyn io::Write,
        _stderr: &mut dyn io::Write,
    ) -> Result<i32, ZillError> {
        let mut no_newline = false;
        let mut text_parts = Vec::new();
        let mut iter = args.iter();

        while let Some(arg) = iter.next() {
            if arg == "-n" {
                no_newline = true;
            } else {
                text_parts.push(arg.clone());
                text_parts.extend(iter.cloned());
                break;
            }
        }

        let output = text_parts.join(" ");
        if no_newline {
            write!(stdout, "{}", output).map_err(|e| ZillError::Generic(e.to_string()))?;
        } else {
            writeln!(stdout, "{}", output).map_err(|e| ZillError::Generic(e.to_string()))?;
        }
        Ok(0)
    }

    /// Creates directories.
    pub fn builtin_mkdir(
        &mut self,
        args: &[String],
        _stdin: &mut dyn io::Read,
        _stdout: &mut dyn io::Write,
        stderr: &mut dyn io::Write,
    ) -> Result<i32, ZillError> {
        let cli = match MkdirArgs::try_parse_from(args) {
            Ok(cli) => cli,
            Err(e) => {
                writeln!(stderr, "{}", e).map_err(|e| ZillError::Generic(e.to_string()))?;
                return Ok(1);
            }
        };

        for path_str in cli.paths {
            let path = self.vfs.canonicalize(Path::new(&path_str), &self.cwd);
            if cli.parents {
                self.vfs.mkdir_p(&path)?;
            } else {
                let parent = path.parent().ok_or_else(|| ZillError::InvalidPath("No parent".into()))?;
                let parent_node = self.vfs.stat(parent)?;
                if !parent_node.is_dir() {
                    return Err(ZillError::NotADirectory(parent.display().to_string()));
                }
                if self.vfs.stat(&path).is_ok() {
                    return Err(ZillError::FileExists(path_str.clone()));
                }
                self.vfs.mkdir_p(&path)?;
            }
        }
        Ok(0)
    }

    /// Updates the timestamp of a file or creates it if it doesn't exist.
    pub fn builtin_touch(
        &mut self,
        args: &[String],
        _stdin: &mut dyn io::Read,
        _stdout: &mut dyn io::Write,
        _stderr: &mut dyn io::Write,
    ) -> Result<i32, ZillError> {
        for arg in args {
            let path = self.vfs.canonicalize(Path::new(arg), &self.cwd);
            match self.vfs.stat(&path) {
                Ok(_) => {}
                Err(_) => self.vfs.create_file(&path, Vec::new())?,
            }
        }
        Ok(0)
    }

    /// Removes files.
    pub fn builtin_rm(
        &mut self,
        args: &[String],
        _stdin: &mut dyn io::Read,
        _stdout: &mut dyn io::Write,
        _stderr: &mut dyn io::Write,
    ) -> Result<i32, ZillError> {
        for arg in args {
            let path = self.vfs.canonicalize(Path::new(arg), &self.cwd);
            let node = self.vfs.stat(&path)?;
            if node.is_dir() {
                return Err(ZillError::RmIsDirectory(arg.clone()));
            }
            self.vfs.remove(&path)?;
        }
        Ok(0)
    }
}
