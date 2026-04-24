use crate::session::{ZillSession, CmdOutput};
use crate::error::ZillError;
use clap::Parser;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{Searcher, Sink, SinkMatch, SinkContext};
use std::path::{Path, PathBuf};
use std::io;

#[derive(Parser, Debug)]
#[command(no_binary_name = true)]
struct RgArgs {
    pattern: String,
    paths: Vec<String>,
    #[arg(short = 'n')]
    line_number: bool,
    #[arg(short = 'i')]
    ignore_case: bool,
    #[arg(short = 'c')]
    count: bool,
    #[arg(short = 'l')]
    files_with_matches: bool,
    #[arg(long = "max-count")]
    max_count: Option<u64>,
}

struct ZillSink<'a> {
    output: &'a mut String,
    path_display: String,
    line_number: bool,
    count_only: bool,
    match_count: u64,
    max_count: Option<u64>,
    files_with_matches: bool,
    found: bool,
    max_output_size: usize,
}

impl<'a> Sink for ZillSink<'a> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        self.found = true;
        self.match_count += 1;

        if self.files_with_matches {
            self.output.push_str(&format!("{}\n", self.path_display));
            return Ok(false); // Stop searching this file
        }

        if !self.count_only {
            if self.line_number {
                self.output.push_str(&format!("{}:{}:", self.path_display, mat.line_number().unwrap_or(0)));
            } else {
                self.output.push_str(&format!("{}:", self.path_display));
            }
            self.output.push_str(&String::from_utf8_lossy(mat.bytes()));
            if !mat.bytes().ends_with(b"\n") {
                self.output.push('\n');
            }
        }

        if let Some(max) = self.max_count {
            if self.match_count >= max {
                return Ok(false);
            }
        }

        if self.output.len() > self.max_output_size {
            return Ok(false);
        }

        Ok(true)
    }

    fn context(&mut self, _searcher: &Searcher, _context: &SinkContext<'_>) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

impl ZillSession {
    pub fn builtin_rg(&self, args: &[String]) -> Result<CmdOutput, ZillError> {
        let cli = match RgArgs::try_parse_from(args) {
            Ok(cli) => cli,
            Err(e) => return Ok(CmdOutput::error(e.to_string(), 1)),
        };

        let matcher = RegexMatcherBuilder::new()
            .case_insensitive(cli.ignore_case)
            .build(&cli.pattern)
            .map_err(|e| ZillError::Generic(format!("invalid regex: {}", e)))?;

        let mut searcher = Searcher::new();
        let mut total_output = String::new();
        let mut total_match_count = 0;

        let paths = if cli.paths.is_empty() {
            vec![".".to_string()]
        } else {
            cli.paths
        };

        for path_str in paths {
            let canonical = self.vfs.canonicalize(Path::new(&path_str), &self.cwd);
            let mut files_to_search = Vec::new();
            self.collect_files_recursive(&canonical, &mut files_to_search)?;

            for file_path in files_to_search {
                if let Ok(content) = self.vfs.read(&file_path) {
                    let path_display = file_path.display().to_string();
                    let (match_count, found) = {
                        let mut sink = ZillSink {
                            output: &mut total_output,
                            path_display: path_display.clone(),
                            line_number: cli.line_number,
                            count_only: cli.count,
                            match_count: 0,
                            max_count: cli.max_count,
                            files_with_matches: cli.files_with_matches,
                            found: false,
                            max_output_size: self.limits.max_output_size,
                        };

                        let _ = searcher.search_slice(&matcher, content, &mut sink);
                        (sink.match_count, sink.found)
                    };

                    if cli.count && found {
                        total_output.push_str(&format!("{}:{}\n", path_display, match_count));
                    }

                    total_match_count += match_count;
                    if total_match_count >= self.limits.max_match_count || total_output.len() > self.limits.max_output_size {
                        break;
                    }
                }
            }
            if total_match_count >= self.limits.max_match_count || total_output.len() > self.limits.max_output_size {
                break;
            }
        }

        Ok(CmdOutput::success(total_output))
    }

    fn collect_files_recursive(&self, path: &Path, files: &mut Vec<PathBuf>) -> Result<(), ZillError> {
        let node = match self.vfs.stat(path) {
            Ok(n) => n,
            Err(_) => return Ok(()),
        };

        match node {
            crate::fs::Node::File(_) => {
                files.push(path.to_path_buf());
            }
            crate::fs::Node::Directory(meta) => {
                let mut children: Vec<_> = meta.children.iter().collect();
                children.sort();
                for child in children {
                    self.collect_files_recursive(&path.join(child), files)?;
                }
            }
        }
        Ok(())
    }
}
