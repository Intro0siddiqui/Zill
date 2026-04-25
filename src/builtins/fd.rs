use crate::session::{ZillSession, CmdOutput};
use crate::error::ZillError;
use crate::fs::Node;
use clap::Parser;
use globset::{GlobBuilder, GlobSetBuilder};
use std::path::Path;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

#[derive(Parser, Debug)]
#[command(no_binary_name = true)]
struct FdArgs {
    pattern: Option<String>,
    path: Option<String>,
    #[arg(short = 'e')]
    extension: Vec<String>,
    #[arg(short = 't')]
    file_type: Option<String>, // f or d
    #[arg(short = 'd')]
    max_depth: Option<usize>,
    #[arg(short = 'H', long)]
    hidden: bool,
}

impl ZillSession {
    pub fn builtin_fd(&self, args: &[String]) -> Result<CmdOutput, ZillError> {
        let cli = match FdArgs::try_parse_from(args) {
            Ok(cli) => cli,
            Err(e) => return Ok(CmdOutput::error(e.to_string(), 1)),
        };

        let search_path = self.vfs.canonicalize(
            Path::new(cli.path.as_deref().unwrap_or(".")),
            &self.cwd
        );

        let mut globset = None;
        if let Some(ref pattern) = cli.pattern {
            let mut builder = GlobSetBuilder::new();
            // Use substring match if it's a simple pattern
            let is_simple = !pattern.contains('*') &&
                            !pattern.contains('?') &&
                            !pattern.contains('[') &&
                            !pattern.contains('{') &&
                            !pattern.contains('\\');

            let glob_pattern = if is_simple {
                format!("*{}*", pattern)
            } else {
                pattern.clone()
            };
            builder.add(GlobBuilder::new(&glob_pattern).literal_separator(false).build().map_err(|e| ZillError::Generic(e.to_string()))?);
            globset = Some(builder.build().map_err(|e| ZillError::Generic(e.to_string()))?);
        }

        let mut gitignore = None;
        if let Ok(content) = self.vfs.read(Path::new("/.gitignore")) {
            let mut builder = GitignoreBuilder::new("/");
            for line in String::from_utf8_lossy(content).lines() {
                builder.add_line(None, line).map_err(|e| ZillError::Generic(e.to_string()))?;
            }
            gitignore = Some(builder.build().map_err(|e| ZillError::Generic(e.to_string()))?);
        }

        let mut results = Vec::new();
        self.walk_vfs(&search_path, &search_path, 0, cli.max_depth, &mut results, &globset, &gitignore, &cli)?;

        results.sort();
        Ok(CmdOutput::success(results.join("\n") + if results.is_empty() { "" } else { "\n" }))
    }

    fn walk_vfs(
        &self,
        search_root: &Path,
        current: &Path,
        depth: usize,
        max_depth: Option<usize>,
        results: &mut Vec<String>,
        globset: &Option<globset::GlobSet>,
        gitignore: &Option<Gitignore>,
        cli: &FdArgs,
    ) -> Result<(), ZillError> {
        if let Some(max) = max_depth {
            if depth > max {
                return Ok(());
            }
        }

        let node = self.vfs.stat(current)?;

        let filename = current.file_name().and_then(|s| s.to_str()).unwrap_or("");

        // Skip hidden if not requested, UNLESS it's the search root
        if !cli.hidden && filename.starts_with('.') && filename != "." && filename != ".." && current != search_root {
            return Ok(());
        }

        // Check if ignored
        if let Some(gi) = gitignore {
            if gi.matched(current, node.is_dir()).is_ignore() {
                return Ok(());
            }
        }

        // Match against filters
        let mut matches = true;

        if let Some(ref gs) = globset {
            if !gs.is_match(current) {
                matches = false;
            }
        }

        if !cli.extension.is_empty() {
            if let Some(ext) = current.extension() {
                let ext_str = ext.to_string_lossy();
                if !cli.extension.iter().any(|e| e == ext_str.as_ref()) {
                    matches = false;
                }
            } else {
                matches = false;
            }
        }

        if let Some(ref t) = cli.file_type {
            match t.as_str() {
                "f" => if !node.is_file() { matches = false; },
                "d" => if !node.is_dir() { matches = false; },
                _ => {}
            }
        }

        if matches && current != Path::new("/") && current != Path::new(".") {
            results.push(current.display().to_string());
        }

        if let Node::Directory(meta) = node {
            let mut children: Vec<_> = meta.children.iter().collect();
            children.sort();
            for child in children {
                self.walk_vfs(search_root, &current.join(child), depth + 1, max_depth, results, globset, gitignore, cli)?;
            }
        }

        Ok(())
    }
}
