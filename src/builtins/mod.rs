pub mod rg;
pub mod fd;

use crate::session::{ZillSession, CmdOutput};
use crate::error::ZillError;
use crate::fs::Node;
use clap::Parser;
use std::path::Path;

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
struct EchoArgs {
    #[arg(short = 'n')]
    no_newline: bool,
    #[arg(trailing_var_arg = true)]
    text: Vec<String>,
}

#[derive(Parser)]
#[command(no_binary_name = true)]
struct MkdirArgs {
    #[arg(short = 'p')]
    parents: bool,
    paths: Vec<String>,
}

impl ZillSession {
    pub fn builtin_pwd(&self, _args: &[String]) -> Result<CmdOutput, ZillError> {
        Ok(CmdOutput::success(format!("{}\n", self.cwd.display())))
    }

    pub fn builtin_cd(&mut self, args: &[String]) -> Result<CmdOutput, ZillError> {
        let target = args.get(0).map(|s| s.as_str()).unwrap_or("/");
        let new_path = self.vfs.canonicalize(Path::new(target), &self.cwd);

        let node = self.vfs.stat(&new_path)?;
        if node.is_dir() {
            self.cwd = new_path;
            Ok(CmdOutput::success(String::new()))
        } else {
            Err(ZillError::NotADirectory(target.to_string()))
        }
    }

    pub fn builtin_ls(&self, args: &[String]) -> Result<CmdOutput, ZillError> {
        let cli = match LsArgs::try_parse_from(args) {
            Ok(cli) => cli,
            Err(e) => return Ok(CmdOutput::error(e.to_string(), 1)),
        };

        let paths = if cli.paths.is_empty() {
            vec![".".to_string()]
        } else {
            cli.paths
        };

        let mut output = String::new();
        for (i, path_str) in paths.iter().enumerate() {
            if paths.len() > 1 {
                output.push_str(&format!("{}:\n", path_str));
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
                            output.push_str(&format!("{}rwxr-xr-x 1 zill zill {:>8} {} {}\n",
                                type_char, size, date.format("%b %d %H:%M"), entry));
                        } else if cli.one_per_line {
                            output.push_str(&format!("{}\n", entry));
                        } else {
                            output.push_str(&format!("{}  ", entry));
                        }
                    }
                    if !cli.long && !cli.one_per_line {
                        output.push('\n');
                    }
                }
                Node::File(_) => {
                    output.push_str(&format!("{}\n", path_str));
                }
            }
            if i < paths.len() - 1 {
                output.push('\n');
            }
        }

        Ok(CmdOutput::success(output))
    }

    pub fn builtin_cat(&self, args: &[String]) -> Result<CmdOutput, ZillError> {
        let cli = match CatArgs::try_parse_from(args) {
            Ok(cli) => cli,
            Err(e) => return Ok(CmdOutput::error(e.to_string(), 1)),
        };

        let mut output = String::new();
        for path_str in cli.paths {
            let path = self.vfs.canonicalize(Path::new(&path_str), &self.cwd);
            let content = self.vfs.read(&path)?;
            let text = String::from_utf8_lossy(content);
            if cli.number {
                for (i, line) in text.lines().enumerate() {
                    output.push_str(&format!("{:>6}  {}\n", i + 1, line));
                }
            } else {
                output.push_str(&text);
            }
        }
        Ok(CmdOutput::success(output))
    }

    pub fn builtin_echo(&self, args: &[String]) -> Result<CmdOutput, ZillError> {
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

        let mut output = text_parts.join(" ");
        if !no_newline {
            output.push('\n');
        }
        Ok(CmdOutput::success(output))
    }

    pub fn builtin_mkdir(&mut self, args: &[String]) -> Result<CmdOutput, ZillError> {
        let cli = match MkdirArgs::try_parse_from(args) {
            Ok(cli) => cli,
            Err(e) => return Ok(CmdOutput::error(e.to_string(), 1)),
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
        Ok(CmdOutput::success(String::new()))
    }

    pub fn builtin_touch(&mut self, args: &[String]) -> Result<CmdOutput, ZillError> {
        for arg in args {
            let path = self.vfs.canonicalize(Path::new(arg), &self.cwd);
            match self.vfs.stat(&path) {
                Ok(_) => {},
                Err(_) => self.vfs.create_file(&path, Vec::new())?,
            }
        }
        Ok(CmdOutput::success(String::new()))
    }

    pub fn builtin_rm(&mut self, args: &[String]) -> Result<CmdOutput, ZillError> {
        for arg in args {
            let path = self.vfs.canonicalize(Path::new(arg), &self.cwd);
            self.vfs.remove(&path)?;
        }
        Ok(CmdOutput::success(String::new()))
    }
}
