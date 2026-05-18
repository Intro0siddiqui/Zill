use serde::{Serialize, Deserialize};

/// Represents a node in the Abstract Syntax Tree (AST) of a parsed shell command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AstNode {
    /// A simple command with arguments and redirections.
    Command {
        /// The argument vector, where argv[0] is the command name.
        argv: Vec<String>,
        /// List of I/O redirections for this command.
        redirects: Vec<Redirection>,
    },
    /// A pipeline of commands (e.g., `cmd1 | cmd2`).
    Pipeline {
        /// The nodes in the pipeline.
        nodes: Vec<AstNode>,
    },
    /// A sequence of commands executed one after another (e.g., `cmd1; cmd2`).
    Sequence {
        /// The nodes in the sequence.
        nodes: Vec<AstNode>,
    },
    /// A logical operation between two commands (e.g., `cmd1 && cmd2` or `cmd1 || cmd2`).
    Logical {
        /// The left-hand side operand.
        left: Box<AstNode>,
        /// The right-hand side operand.
        right: Box<AstNode>,
        /// The logical operator (And/Or).
        operator: LogicalOperator,
    },
    /// A subshell execution grouping (e.g., `(cmd1; cmd2)`).
    Subshell {
        /// The node to execute in the subshell.
        node: Box<AstNode>,
    },
    /// An if-then-else control flow construct.
    If {
        /// The condition command to execute.
        condition: Box<AstNode>,
        /// The branch to execute if the condition succeeds (exit code 0).
        then_part: Box<AstNode>,
        /// The optional branch to execute if the condition fails.
        else_part: Option<Box<AstNode>>,
    },
    /// A for-loop construct (e.g., `for i in a b c; do cmd; done`).
    For {
        /// The loop variable name.
        variable: String,
        /// The list of items to iterate over.
        items: Vec<String>,
        /// The body of the loop.
        body: Box<AstNode>,
    },
}

/// Logical operators used in shell commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogicalOperator {
    /// The `&&` operator.
    And,
    /// The `||` operator.
    Or,
}

/// Represents I/O redirections for a command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Redirection {
    /// Redirection of stdout to a file, overwriting its content (`>`).
    StdoutOverwrite(String),
    /// Redirection of stdout to a file, appending to its content (`>>`).
    StdoutAppend(String),
    /// Redirection of stdin from a file (`<`).
    Stdin(String),
}

/// A shell command parser that generates an AST.
pub struct Parser<'a> {
    words: Vec<String>,
    pos: usize,
    _marker: std::marker::PhantomData<&'a str>,
}

impl<'a> Parser<'a> {
    /// Creates a new parser from the given input string.
    ///
    /// This performs initial lexing, handling quotes and escape characters.
    pub fn new(input: &str) -> Result<Self, String> {
        let mut words = Vec::new();
        let mut current_word = String::new();
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut escaped = false;

        let mut it = input.chars().peekable();
        while let Some(c) = it.next() {
            if escaped {
                current_word.push(c);
                escaped = false;
                continue;
            }

            match c {
                '\\' if !in_single_quote => {
                    escaped = true;
                }
                '\'' if !in_double_quote => {
                    in_single_quote = !in_single_quote;
                }
                '"' if !in_single_quote => {
                    in_double_quote = !in_double_quote;
                }
                _ if in_single_quote || in_double_quote => {
                    current_word.push(c);
                }
                ' ' | '\t' | '\n' | '\r' => {
                    if !current_word.is_empty() {
                        words.push(current_word);
                        current_word = String::new();
                    }
                }
                '|' | '&' | ';' | '(' | ')' | '<' | '>' => {
                    if !current_word.is_empty() {
                        words.push(current_word);
                        current_word = String::new();
                    }
                    let mut meta = c.to_string();
                    if (c == '|' && it.peek() == Some(&'|'))
                        || (c == '&' && it.peek() == Some(&'&'))
                        || (c == '>' && it.peek() == Some(&'>'))
                    {
                        meta.push(it.next().unwrap());
                    }
                    words.push(meta);
                }
                _ => {
                    current_word.push(c);
                }
            }
        }

        if !current_word.is_empty() {
            words.push(current_word);
        }

        if in_single_quote || in_double_quote {
            return Err("unclosed quote".into());
        }

        Ok(Parser {
            words,
            pos: 0,
            _marker: std::marker::PhantomData,
        })
    }

    /// Parses the input words into an AST.
    pub fn parse(&mut self) -> Result<AstNode, String> {
        self.parse_sequence()
    }

    fn parse_sequence(&mut self) -> Result<AstNode, String> {
        let mut nodes = Vec::new();
        while self.pos < self.words.len() {
            if let Some(word) = self.peek() {
                if ["then", "else", "fi", "do", "done", ")"].contains(&word) {
                    break;
                }
            }
            let node = self.parse_logical()?;
            nodes.push(node);
            if self.peek() == Some(";") {
                self.consume();
            } else {
                break;
            }
        }
        if nodes.is_empty() {
            return Err("empty command".into());
        }
        if nodes.len() == 1 {
            Ok(nodes.remove(0))
        } else {
            Ok(AstNode::Sequence { nodes })
        }
    }

    fn parse_logical(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_pipeline()?;
        while let Some(op) = self.peek() {
            match op {
                "&&" => {
                    self.consume();
                    let right = self.parse_pipeline()?;
                    left = AstNode::Logical {
                        left: Box::new(left),
                        right: Box::new(right),
                        operator: LogicalOperator::And,
                    };
                }
                "||" => {
                    self.consume();
                    let right = self.parse_pipeline()?;
                    left = AstNode::Logical {
                        left: Box::new(left),
                        right: Box::new(right),
                        operator: LogicalOperator::Or,
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_pipeline(&mut self) -> Result<AstNode, String> {
        let mut nodes = Vec::new();
        nodes.push(self.parse_primary()?);
        while self.peek() == Some("|") {
            self.consume();
            nodes.push(self.parse_primary()?);
        }
        if nodes.len() == 1 {
            Ok(nodes.remove(0))
        } else {
            Ok(AstNode::Pipeline { nodes })
        }
    }

    fn parse_primary(&mut self) -> Result<AstNode, String> {
        match self.peek() {
            Some("if") => self.parse_if(),
            Some("for") => self.parse_for(),
            Some("then") | Some("else") | Some("fi") | Some("do") | Some("done") => {
                Err(format!("unexpected token '{}'", self.peek().unwrap()))
            }
            Some("(") => {
                self.consume();
                let node = self.parse_sequence()?;
                if self.peek() == Some(")") {
                    self.consume();
                    Ok(AstNode::Subshell { node: Box::new(node) })
                } else {
                    Err("expected ')'".into())
                }
            }
            _ => self.parse_command(),
        }
    }

    fn parse_if(&mut self) -> Result<AstNode, String> {
        self.consume(); // if
        let condition = self.parse_sequence()?;
        if self.peek() != Some("then") {
            return Err("expected 'then'".into());
        }
        self.consume(); // then
        let then_part = self.parse_sequence()?;
        let mut else_part = None;
        if self.peek() == Some("else") {
            self.consume();
            else_part = Some(Box::new(self.parse_sequence()?));
        }
        if self.peek() != Some("fi") {
            return Err("expected 'fi'".into());
        }
        self.consume(); // fi
        Ok(AstNode::If {
            condition: Box::new(condition),
            then_part: Box::new(then_part),
            else_part,
        })
    }

    fn parse_for(&mut self) -> Result<AstNode, String> {
        self.consume(); // for
        let variable = self.consume().ok_or("expected variable name after 'for'")?.to_string();
        if self.peek() != Some("in") {
            return Err("expected 'in' after variable name in 'for'".into());
        }
        self.consume(); // in
        let mut items = Vec::new();
        while let Some(word) = self.peek() {
            if word == "do" || word == ";" {
                break;
            }
            items.push(self.consume().unwrap().to_string());
        }
        if self.peek() == Some(";") {
            self.consume();
        }
        if self.peek() != Some("do") {
            return Err("expected 'do'".into());
        }
        self.consume(); // do
        let body = self.parse_sequence()?;
        if self.peek() != Some("done") {
            return Err("expected 'done'".into());
        }
        self.consume(); // done
        Ok(AstNode::For {
            variable,
            items,
            body: Box::new(body),
        })
    }

    fn parse_command(&mut self) -> Result<AstNode, String> {
        let mut argv = Vec::new();
        let mut redirects = Vec::new();

        while let Some(word) = self.peek() {
            match word {
                "|" | "&&" | "||" | ";" | ")" | "then" | "else" | "fi" | "do" | "done" => break,
                ">" => {
                    self.consume();
                    let path = self.consume().ok_or("expected file after '>'")?.to_string();
                    redirects.push(Redirection::StdoutOverwrite(path));
                }
                ">>" => {
                    self.consume();
                    let path = self.consume().ok_or("expected file after '>>'")?.to_string();
                    redirects.push(Redirection::StdoutAppend(path));
                }
                "<" => {
                    self.consume();
                    let path = self.consume().ok_or("expected file after '<'")?.to_string();
                    redirects.push(Redirection::Stdin(path));
                }
                _ => {
                    argv.push(self.consume().unwrap().to_string());
                }
            }
        }

        if argv.is_empty() && redirects.is_empty() {
             return Err("empty command".into());
        }

        Ok(AstNode::Command { argv, redirects })
    }

    fn peek(&self) -> Option<&str> {
        self.words.get(self.pos).map(|s| s.as_str())
    }

    /// Consumes and returns the next word, if any.
    fn consume(&mut self) -> Option<&str> {
        let word = self.words.get(self.pos).map(|s| s.as_str());
        if word.is_some() {
            self.pos += 1;
        }
        word
    }
}
