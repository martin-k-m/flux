//! A small hand-written lexer + recursive-descent parser for `.flux`.
//!
//! The grammar is intentionally small, so a dedicated parser generator (pest,
//! nom) would be more machinery than the language warrants. Keeping it
//! hand-rolled means zero grammar-build steps and precise error messages.
//!
//! ```text
//! config    := item*
//! item      := "project" STRING
//!            | "language" IDENT
//!            | "environment" "{" ("image" STRING)* "}"
//!            | "secret" IDENT
//!            | "import" name
//!            | "deployment" "{" dep_field* "}"
//!            | "runners" "{" pool* "}"
//!            | "policy" name "{" require* "}"
//!            | "pipeline" "{" (step | use)* "}"
//! dep_field := "target" IDENT | "replicas" NUM | "image" STRING
//! pool      := "pool" name "{" pool_field* "}"
//! pool_field := "os" name | "gpu" bool | "memory" name
//!            | "requirements" "{" pool_field* "}"
//! require   := "require" ("tests" | "security" | "approvals" NUM)
//! use       := "use" name
//! step      := "step" IDENT "{" field* "}"
//! field     := "command" STRING
//!            | "tool" IDENT
//!            | "description" STRING
//!            | "cache" IDENT              ; on/off/true/false/yes/no
//!            | "needs" ident_or_list
//!            | "env" ident_or_list
//!            | "inputs" ident_or_list     ; cache-scoping globs
//!            | "pool" name
//!            | "retries" NUM
//!            | "only_if" IDENT ("=="|"!=") STRING
//! ident_or_list := item_or_str | "[" (item_or_str ("," item_or_str)*)? "]"
//! item_or_str   := IDENT | STRING
//! name          := IDENT | STRING
//! bool          := "true" | "yes" | "on"  ; anything else is false
//! ```
//!
//! A `:` after a field keyword (e.g. `only_if:` or `env:`) is accepted and
//! ignored, so the spec's colon style parses too. Commas inside `[ … ]` lists
//! and inside `policy`/`runners`/`requirements` blocks are optional separators.

use std::fmt;

use super::ast::{
    CondOp, Condition, Deployment, Environment, FluxConfig, Policy, RunnerPool, Step,
};

/// A parse failure with 1-based line information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl ParseError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        ParseError {
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokKind {
    Ident,
    Str,
    Num,
    Op, // "==" or "!="
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokKind,
    text: String,
    line: usize,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')
}

fn lex(src: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    let mut line = 1usize;
    let mut chars = src.chars().peekable();

    while let Some(&c) = chars.peek() {
        match c {
            '\n' => {
                line += 1;
                chars.next();
            }
            // A colon is decorative (e.g. `only_if:`); skip it.
            ':' => {
                chars.next();
            }
            c if c.is_whitespace() => {
                chars.next();
            }
            // `#` line comment
            '#' => {
                while let Some(&c) = chars.peek() {
                    if c == '\n' {
                        break;
                    }
                    chars.next();
                }
            }
            // `//` line comment
            '/' => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    while let Some(&c) = chars.peek() {
                        if c == '\n' {
                            break;
                        }
                        chars.next();
                    }
                } else {
                    return Err(ParseError::new(
                        line,
                        "unexpected '/' (did you mean '//' ?)",
                    ));
                }
            }
            '{' => {
                push(&mut tokens, TokKind::LBrace, "{", line);
                chars.next();
            }
            '}' => {
                push(&mut tokens, TokKind::RBrace, "}", line);
                chars.next();
            }
            '[' => {
                push(&mut tokens, TokKind::LBracket, "[", line);
                chars.next();
            }
            ']' => {
                push(&mut tokens, TokKind::RBracket, "]", line);
                chars.next();
            }
            ',' => {
                push(&mut tokens, TokKind::Comma, ",", line);
                chars.next();
            }
            '=' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    push(&mut tokens, TokKind::Op, "==", line);
                } else {
                    return Err(ParseError::new(
                        line,
                        "unexpected '=' (did you mean '==' ?)",
                    ));
                }
            }
            '!' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    push(&mut tokens, TokKind::Op, "!=", line);
                } else {
                    return Err(ParseError::new(
                        line,
                        "unexpected '!' (did you mean '!=' ?)",
                    ));
                }
            }
            '"' => {
                chars.next(); // consume opening quote
                let start_line = line;
                let mut s = String::new();
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some('"') => s.push('"'),
                            Some('\\') => s.push('\\'),
                            Some(other) => s.push(other),
                            None => return Err(ParseError::new(start_line, "unterminated string")),
                        },
                        Some('\n') => {
                            return Err(ParseError::new(
                                start_line,
                                "newline inside string literal",
                            ))
                        }
                        Some(other) => s.push(other),
                        None => return Err(ParseError::new(start_line, "unterminated string")),
                    }
                }
                push(&mut tokens, TokKind::Str, &s, start_line);
            }
            c if c.is_ascii_digit() => {
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() {
                        s.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                push(&mut tokens, TokKind::Num, &s, line);
            }
            c if is_ident_start(c) => {
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if is_ident_char(c) {
                        s.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                push(&mut tokens, TokKind::Ident, &s, line);
            }
            other => {
                return Err(ParseError::new(
                    line,
                    format!("unexpected character '{other}'"),
                ))
            }
        }
    }

    Ok(tokens)
}

fn push(tokens: &mut Vec<Token>, kind: TokKind, text: &str, line: usize) {
    tokens.push(Token {
        kind,
        text: text.to_string(),
        line,
    });
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn last_line(&self) -> usize {
        self.toks
            .get(self.pos.saturating_sub(1))
            .map(|t| t.line)
            .unwrap_or(0)
    }

    fn expect(&mut self, kind: TokKind, what: &str) -> Result<Token, ParseError> {
        match self.next() {
            Some(t) if t.kind == kind => Ok(t),
            Some(t) => Err(ParseError::new(
                t.line,
                format!("expected {what}, found '{}'", t.text),
            )),
            None => Err(ParseError::new(
                self.last_line(),
                format!("expected {what}, found end of file"),
            )),
        }
    }

    fn expect_lbrace(&mut self) -> Result<(), ParseError> {
        self.expect(TokKind::LBrace, "'{'").map(|_| ())
    }

    fn expect_str(&mut self) -> Result<String, ParseError> {
        self.expect(TokKind::Str, "a quoted string").map(|t| t.text)
    }

    fn expect_ident(&mut self) -> Result<(String, usize), ParseError> {
        self.expect(TokKind::Ident, "an identifier")
            .map(|t| (t.text, t.line))
    }

    fn expect_number(&mut self) -> Result<(u32, usize), ParseError> {
        let t = self.expect(TokKind::Num, "a number")?;
        let n = t
            .text
            .parse::<u32>()
            .map_err(|_| ParseError::new(t.line, format!("'{}' is not a valid number", t.text)))?;
        Ok((n, t.line))
    }

    /// Accept either a quoted string or a bare identifier, returning its text.
    fn expect_str_or_ident(&mut self) -> Result<String, ParseError> {
        match self.next() {
            Some(t) if t.kind == TokKind::Str || t.kind == TokKind::Ident => Ok(t.text),
            Some(t) => Err(ParseError::new(
                t.line,
                format!("expected a name or string, found '{}'", t.text),
            )),
            None => Err(ParseError::new(
                self.last_line(),
                "expected a name or string, found end of file",
            )),
        }
    }
}

/// Parse `.flux` source text into a [`FluxConfig`].
pub fn parse(src: &str) -> Result<FluxConfig, ParseError> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0 };
    let mut cfg = FluxConfig::default();

    while let Some(tok) = p.peek().cloned() {
        if tok.kind != TokKind::Ident {
            return Err(ParseError::new(
                tok.line,
                format!("expected a top-level keyword, found '{}'", tok.text),
            ));
        }
        match tok.text.as_str() {
            "project" => {
                p.next();
                cfg.project = Some(p.expect_str()?);
            }
            "language" => {
                p.next();
                cfg.language = Some(p.expect_ident()?.0);
            }
            "environment" => {
                p.next();
                cfg.environment = Some(parse_environment(&mut p)?);
            }
            "secret" => {
                p.next();
                cfg.secrets.push(p.expect_ident()?.0);
            }
            "deployment" => {
                p.next();
                cfg.deployment = Some(parse_deployment(&mut p)?);
            }
            "import" => {
                p.next();
                cfg.imports.push(p.expect_str_or_ident()?);
            }
            "runners" => {
                p.next();
                cfg.runner_pools = parse_runners(&mut p)?;
            }
            "policy" => {
                p.next();
                cfg.policies.push(parse_policy(&mut p)?);
            }
            "pipeline" => {
                p.next();
                parse_pipeline(&mut p, &mut cfg)?;
            }
            other => {
                return Err(ParseError::new(
                    tok.line,
                    format!(
                        "unknown top-level keyword '{other}' (expected project, language, environment, secret, deployment, import, runners, policy, or pipeline)"
                    ),
                ))
            }
        }
    }

    Ok(cfg)
}

fn parse_environment(p: &mut Parser) -> Result<Environment, ParseError> {
    p.expect_lbrace()?;
    let mut env = Environment::default();
    loop {
        let tok = match p.peek().cloned() {
            Some(t) => t,
            None => {
                return Err(ParseError::new(
                    p.last_line(),
                    "unclosed 'environment' block",
                ))
            }
        };
        if tok.kind == TokKind::RBrace {
            p.next();
            break;
        }
        let (field, line) = p.expect_ident()?;
        match field.as_str() {
            "image" => env.image = Some(p.expect_str()?),
            other => {
                return Err(ParseError::new(
                    line,
                    format!("unknown environment field '{other}' (expected image)"),
                ))
            }
        }
    }
    Ok(env)
}

fn parse_deployment(p: &mut Parser) -> Result<Deployment, ParseError> {
    p.expect_lbrace()?;
    let mut dep = Deployment::default();
    loop {
        let tok = match p.peek().cloned() {
            Some(t) => t,
            None => {
                return Err(ParseError::new(
                    p.last_line(),
                    "unclosed 'deployment' block",
                ))
            }
        };
        if tok.kind == TokKind::RBrace {
            p.next();
            break;
        }
        let (field, line) = p.expect_ident()?;
        match field.as_str() {
            "target" => dep.target = Some(p.expect_ident()?.0),
            "replicas" => dep.replicas = Some(p.expect_number()?.0),
            "image" => dep.image = Some(p.expect_str()?),
            other => {
                return Err(ParseError::new(
                    line,
                    format!(
                        "unknown deployment field '{other}' (expected target, replicas, or image)"
                    ),
                ))
            }
        }
    }
    Ok(dep)
}

fn parse_pipeline(p: &mut Parser, cfg: &mut FluxConfig) -> Result<(), ParseError> {
    p.expect_lbrace()?;
    loop {
        let tok = match p.peek().cloned() {
            Some(t) => t,
            None => return Err(ParseError::new(p.last_line(), "unclosed 'pipeline' block")),
        };
        if tok.kind == TokKind::RBrace {
            p.next();
            break;
        }
        if tok.kind == TokKind::Ident && tok.text == "step" {
            p.next();
            let step = parse_step(p)?;
            cfg.steps.push(step);
        } else if tok.kind == TokKind::Ident && tok.text == "use" {
            // `use <module>` splices a reusable module's steps into this pipeline.
            p.next();
            cfg.uses.push(p.expect_str_or_ident()?);
        } else {
            return Err(ParseError::new(
                tok.line,
                format!("expected 'step', 'use', or '}}', found '{}'", tok.text),
            ));
        }
    }
    Ok(())
}

fn parse_step(p: &mut Parser) -> Result<Step, ParseError> {
    let (name, _) = p.expect_ident()?;
    let mut step = Step::new(name);
    p.expect_lbrace()?;

    loop {
        let tok = match p.peek().cloned() {
            Some(t) => t,
            None => return Err(ParseError::new(p.last_line(), "unclosed 'step' block")),
        };
        if tok.kind == TokKind::RBrace {
            p.next();
            break;
        }
        if tok.kind != TokKind::Ident {
            return Err(ParseError::new(
                tok.line,
                format!("expected a step field, found '{}'", tok.text),
            ));
        }
        p.next();
        match tok.text.as_str() {
            "command" => step.command = Some(p.expect_str()?),
            "tool" => step.tool = Some(p.expect_ident()?.0),
            "description" => step.description = Some(p.expect_str()?),
            "cache" => {
                let (v, line) = p.expect_ident()?;
                step.cache = match v.as_str() {
                    "on" | "true" | "yes" => true,
                    "off" | "false" | "no" => false,
                    other => {
                        return Err(ParseError::new(
                            line,
                            format!("invalid cache value '{other}' (expected on/off)"),
                        ))
                    }
                };
            }
            "needs" => step.needs = parse_ident_or_list(p)?,
            "env" => step.env = parse_ident_or_list(p)?,
            "inputs" => step.inputs = parse_ident_or_list(p)?,
            "pool" => step.pool = Some(p.expect_str_or_ident()?),
            "retries" => step.retries = p.expect_number()?.0,
            "only_if" => step.only_if = Some(parse_condition(p)?),
            other => {
                return Err(ParseError::new(
                    tok.line,
                    format!(
                        "unknown step field '{other}' (expected command, tool, description, cache, needs, env, inputs, pool, retries, or only_if)"
                    ),
                ))
            }
        }
    }

    if step.command.is_none() && step.tool.is_none() {
        return Err(ParseError::new(
            p.last_line(),
            format!("step '{}' has neither a command nor a tool", step.name),
        ));
    }

    Ok(step)
}

/// Parse either a bare item or a `[a, b, c]` list. Items may be identifiers
/// (e.g. step names) or quoted strings (e.g. glob patterns for `inputs`).
fn parse_ident_or_list(p: &mut Parser) -> Result<Vec<String>, ParseError> {
    match p.peek().cloned() {
        Some(t) if t.kind == TokKind::LBracket => {
            p.next();
            let mut items = Vec::new();
            loop {
                let tok = match p.peek().cloned() {
                    Some(t) => t,
                    None => return Err(ParseError::new(p.last_line(), "unclosed '[' list")),
                };
                if tok.kind == TokKind::RBracket {
                    p.next();
                    break;
                }
                if tok.kind == TokKind::Comma {
                    p.next();
                    continue;
                }
                items.push(p.expect_str_or_ident()?);
            }
            Ok(items)
        }
        Some(t) if t.kind == TokKind::Ident || t.kind == TokKind::Str => {
            Ok(vec![p.expect_str_or_ident()?])
        }
        Some(t) => Err(ParseError::new(
            t.line,
            format!("expected an item or '[', found '{}'", t.text),
        )),
        None => Err(ParseError::new(p.last_line(), "expected an item or '['")),
    }
}

/// Parse a `runners { pool "name" { requirements { ... } } ... }` block.
fn parse_runners(p: &mut Parser) -> Result<Vec<RunnerPool>, ParseError> {
    p.expect_lbrace()?;
    let mut pools = Vec::new();
    loop {
        let tok = match p.peek().cloned() {
            Some(t) => t,
            None => return Err(ParseError::new(p.last_line(), "unclosed 'runners' block")),
        };
        if tok.kind == TokKind::RBrace {
            p.next();
            break;
        }
        let (kw, line) = p.expect_ident()?;
        if kw != "pool" {
            return Err(ParseError::new(
                line,
                format!("expected 'pool' or '}}', found '{kw}'"),
            ));
        }
        let name = p.expect_str_or_ident()?;
        let mut pool = RunnerPool {
            name,
            ..RunnerPool::default()
        };
        p.expect_lbrace()?;
        loop {
            let tok = match p.peek().cloned() {
                Some(t) => t,
                None => return Err(ParseError::new(p.last_line(), "unclosed 'pool' block")),
            };
            if tok.kind == TokKind::RBrace {
                p.next();
                break;
            }
            if tok.kind == TokKind::Comma {
                p.next();
                continue;
            }
            let (field, fline) = p.expect_ident()?;
            match field.as_str() {
                "requirements" => parse_requirements(p, &mut pool)?,
                "os" => pool.os = Some(p.expect_str_or_ident()?),
                "gpu" => pool.gpu = Some(parse_bool(p)?),
                "memory" => pool.memory = Some(p.expect_str_or_ident()?),
                other => {
                    return Err(ParseError::new(
                        fline,
                        format!(
                        "unknown pool field '{other}' (expected requirements, os, gpu, or memory)"
                    ),
                    ))
                }
            }
        }
        pools.push(pool);
    }
    Ok(pools)
}

fn parse_requirements(p: &mut Parser, pool: &mut RunnerPool) -> Result<(), ParseError> {
    p.expect_lbrace()?;
    loop {
        let tok = match p.peek().cloned() {
            Some(t) => t,
            None => {
                return Err(ParseError::new(
                    p.last_line(),
                    "unclosed 'requirements' block",
                ))
            }
        };
        if tok.kind == TokKind::RBrace {
            p.next();
            break;
        }
        if tok.kind == TokKind::Comma {
            p.next();
            continue;
        }
        let (field, line) = p.expect_ident()?;
        match field.as_str() {
            "gpu" => pool.gpu = Some(parse_bool(p)?),
            "memory" => pool.memory = Some(p.expect_str_or_ident()?),
            "os" => pool.os = Some(p.expect_str_or_ident()?),
            other => {
                return Err(ParseError::new(
                    line,
                    format!("unknown requirement '{other}' (expected gpu, memory, or os)"),
                ))
            }
        }
    }
    Ok(())
}

fn parse_bool(p: &mut Parser) -> Result<bool, ParseError> {
    let v = p.expect_str_or_ident()?;
    Ok(matches!(v.as_str(), "true" | "yes" | "on"))
}

/// Parse `policy <name> { require tests, require security, require approvals N }`.
fn parse_policy(p: &mut Parser) -> Result<Policy, ParseError> {
    let name = p.expect_str_or_ident()?;
    let mut policy = Policy {
        name,
        ..Policy::default()
    };
    p.expect_lbrace()?;
    loop {
        let tok = match p.peek().cloned() {
            Some(t) => t,
            None => return Err(ParseError::new(p.last_line(), "unclosed 'policy' block")),
        };
        if tok.kind == TokKind::RBrace {
            p.next();
            break;
        }
        if tok.kind == TokKind::Comma {
            p.next();
            continue;
        }
        let (kw, line) = p.expect_ident()?;
        if kw != "require" {
            return Err(ParseError::new(
                line,
                format!("expected 'require' in policy, found '{kw}'"),
            ));
        }
        let (what, wline) = p.expect_ident()?;
        match what.as_str() {
            "tests" => policy.require_tests = true,
            "security" => policy.require_security = true,
            "approvals" => policy.require_approvals = p.expect_number()?.0,
            other => {
                return Err(ParseError::new(
                    wline,
                    format!(
                    "unknown policy requirement '{other}' (expected tests, security, or approvals)"
                ),
                ))
            }
        }
    }
    Ok(policy)
}

/// Parse a condition: `IDENT ("==" | "!=") STRING`.
fn parse_condition(p: &mut Parser) -> Result<Condition, ParseError> {
    let (var, _) = p.expect_ident()?;
    let op_tok = p.expect(TokKind::Op, "'==' or '!='")?;
    let op = match op_tok.text.as_str() {
        "==" => CondOp::Eq,
        "!=" => CondOp::Ne,
        _ => unreachable!("lexer only emits == or !="),
    };
    let value = p.expect_str()?;
    Ok(Condition { var, op, value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_reference_example() {
        let src = r#"
            project "my-app"
            language rust

            pipeline {
                step dependencies { command "cargo fetch" }
                step build        { command "cargo build --release" }
                step test         { command "cargo test" }
            }
        "#;
        let cfg = parse(src).expect("should parse");
        assert_eq!(cfg.project.as_deref(), Some("my-app"));
        assert_eq!(cfg.language.as_deref(), Some("rust"));
        assert_eq!(cfg.steps.len(), 3);
        assert_eq!(cfg.steps[1].name, "build");
        assert_eq!(
            cfg.steps[1].command.as_deref(),
            Some("cargo build --release")
        );
        assert!(cfg.steps[1].cache);
    }

    #[test]
    fn parses_tool_hooks_and_cache_flag() {
        let src = r#"
            project "svc"
            language node
            pipeline {
                step build { command "npm run build" cache off }
                step security { tool scanner }
            }
        "#;
        let cfg = parse(src).unwrap();
        assert!(!cfg.steps[0].cache);
        assert_eq!(cfg.steps[1].tool.as_deref(), Some("scanner"));
        assert!(cfg.steps[1].is_hook());
    }

    #[test]
    fn supports_comments() {
        let src = "# a comment\nproject \"x\" // trailing\nlanguage python\n";
        let cfg = parse(src).unwrap();
        assert_eq!(cfg.project.as_deref(), Some("x"));
        assert_eq!(cfg.language.as_deref(), Some("python"));
    }

    #[test]
    fn reports_unknown_keyword_with_line() {
        let err = parse("\nbogus \"x\"\n").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn rejects_empty_step() {
        let err = parse("pipeline { step build { } }").unwrap_err();
        assert!(err.message.contains("neither a command nor a tool"));
    }

    #[test]
    fn parses_needs_list_and_single() {
        let src = r#"
            pipeline {
                step frontend { command "npm build" }
                step backend  { command "cargo build" }
                step tests {
                    needs [ frontend, backend ]
                    command "./run-tests"
                }
                step package {
                    needs tests
                    command "docker build ."
                }
            }
        "#;
        let cfg = parse(src).unwrap();
        let tests = cfg.steps.iter().find(|s| s.name == "tests").unwrap();
        assert_eq!(tests.needs, vec!["frontend", "backend"]);
        let package = cfg.steps.iter().find(|s| s.name == "package").unwrap();
        assert_eq!(package.needs, vec!["tests"]);
    }

    #[test]
    fn parses_only_if_retries_and_env() {
        let src = r#"
            secret DATABASE_URL
            pipeline {
                step deploy {
                    command "./deploy"
                    only_if: branch == "main"
                    retries 3
                    env: [ DATABASE_URL ]
                }
            }
        "#;
        let cfg = parse(src).unwrap();
        assert_eq!(cfg.secrets, vec!["DATABASE_URL"]);
        let deploy = &cfg.steps[0];
        assert_eq!(deploy.retries, 3);
        assert_eq!(deploy.env, vec!["DATABASE_URL"]);
        let cond = deploy.only_if.as_ref().unwrap();
        assert_eq!(cond.var, "branch");
        assert_eq!(cond.op, CondOp::Eq);
        assert_eq!(cond.value, "main");
    }

    /// The top-level `import`/`runners`/`policy` items and the `inputs`/`pool`
    /// step fields are part of the documented grammar but were only exercised
    /// end-to-end; this pins them at the parser level.
    #[test]
    fn parses_imports_runner_pools_policies_and_step_scoping() {
        let src = r#"
            import shared-ci
            runners {
                pool "gpu-builders" {
                    requirements { gpu true, memory "32gb" }
                }
                pool linux { os linux }
            }
            policy production {
                require tests
                require security
                require approvals 2
            }
            pipeline {
                use rust-library
                step build {
                    command "cargo build --release"
                    inputs [ "src/**", "Cargo.toml" ]
                    pool "gpu-builders"
                }
            }
        "#;
        let cfg = parse(src).unwrap();
        assert_eq!(cfg.imports, vec!["shared-ci"]);
        assert_eq!(cfg.uses, vec!["rust-library"]);

        assert_eq!(cfg.runner_pools.len(), 2);
        let gpu = &cfg.runner_pools[0];
        assert_eq!(gpu.name, "gpu-builders");
        assert_eq!(gpu.gpu, Some(true));
        assert_eq!(gpu.memory.as_deref(), Some("32gb"));
        assert_eq!(cfg.runner_pools[1].os.as_deref(), Some("linux"));

        assert_eq!(cfg.policies.len(), 1);
        let policy = &cfg.policies[0];
        assert_eq!(policy.name, "production");
        assert!(policy.require_tests);
        assert!(policy.require_security);
        assert_eq!(policy.require_approvals, 2);

        let build = &cfg.steps[0];
        assert_eq!(build.inputs, vec!["src/**", "Cargo.toml"]);
        assert_eq!(build.pool.as_deref(), Some("gpu-builders"));
    }

    #[test]
    fn parses_environment_and_deployment() {
        let src = r#"
            environment { image "rust:latest" }
            deployment { target kubernetes replicas 3 }
            pipeline { step build { command "cargo build" } }
        "#;
        let cfg = parse(src).unwrap();
        assert_eq!(
            cfg.environment.unwrap().image.as_deref(),
            Some("rust:latest")
        );
        let dep = cfg.deployment.unwrap();
        assert_eq!(dep.target.as_deref(), Some("kubernetes"));
        assert_eq!(dep.replicas, Some(3));
    }
}
