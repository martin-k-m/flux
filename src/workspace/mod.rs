//! Flux Workspaces (Phase 4, 4.1 & 4.2).
//!
//! A workspace manages several projects (repositories/services) as one unit,
//! with dependencies between them. A `flux.workspace` file declares members:
//!
//! ```text
//! workspace "backend"
//!
//! member shared  { path "shared" }
//! member auth    { path "services/auth"    needs [ shared ] }
//! member gateway { path "services/gateway" needs [ auth, shared ] }
//! ```
//!
//! Flux understands the cross-project dependency graph: when `shared` changes,
//! only `shared` and the members that depend on it rebuild — everything else is
//! skipped (4.2).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::cache::Cache;
use crate::core::graph::Graph;

/// The conventional workspace filename.
pub const WORKSPACE_FILE: &str = "flux.workspace";

/// A workspace member (one project).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// Path to the member, relative to the workspace root.
    pub path: String,
    /// Other members this one depends on.
    pub needs: Vec<String>,
}

/// A parsed workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub name: String,
    pub members: Vec<Member>,
}

impl Workspace {
    /// Load `flux.workspace` from `root`, or `None` if absent.
    pub fn load(root: &Path) -> anyhow::Result<Option<Workspace>> {
        let path = root.join(WORKSPACE_FILE);
        match std::fs::read_to_string(&path) {
            Ok(src) => Ok(Some(parse(&src)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Members in dependency order (roots first). Validates the graph (cycles,
    /// unknown members) by reusing the pipeline graph engine.
    pub fn ordered(&self) -> anyhow::Result<Vec<Member>> {
        let steps: Vec<crate::core::config::Step> = self
            .members
            .iter()
            .map(|m| {
                let mut s = crate::core::config::Step::command(&m.name, "noop");
                s.needs = m.needs.clone();
                s
            })
            .collect();
        let graph = Graph::build(&steps).map_err(|e| anyhow::anyhow!("workspace: {e}"))?;
        let order = graph.topo_order();
        let mut ordered = Vec::new();
        for name in order {
            if let Some(m) = self.members.iter().find(|m| m.name == name) {
                ordered.push(m.clone());
            }
        }
        Ok(ordered)
    }

    /// Compute which members are affected since the last workspace build: a
    /// member whose own files changed, plus everything transitively downstream.
    pub fn affected(&self, root: &Path) -> anyhow::Result<HashSet<String>> {
        let cache = Cache::new(root);
        let store_dir = root.join(".flux-cache").join("workspace");
        std::fs::create_dir_all(&store_dir)?;

        let ordered = self.ordered()?;
        let mut affected: HashSet<String> = HashSet::new();

        for member in &ordered {
            let pattern = vec![format!("{}/**", member.path.trim_end_matches('/'))];
            let hash = cache.source_hash_scoped(&pattern);
            let hash_file = store_dir.join(format!("{}.hash", safe(&member.name)));
            let previous = std::fs::read_to_string(&hash_file).ok();

            let changed = previous.as_deref() != Some(hash.as_str());
            let dep_affected = member.needs.iter().any(|n| affected.contains(n));
            if changed || dep_affected {
                affected.insert(member.name.clone());
            }
        }
        Ok(affected)
    }

    /// Record the current hash for each member (call after a successful build).
    pub fn record_hashes(&self, root: &Path) -> anyhow::Result<()> {
        let cache = Cache::new(root);
        let store_dir = root.join(".flux-cache").join("workspace");
        std::fs::create_dir_all(&store_dir)?;
        for member in &self.members {
            let pattern = vec![format!("{}/**", member.path.trim_end_matches('/'))];
            let hash = cache.source_hash_scoped(&pattern);
            std::fs::write(store_dir.join(format!("{}.hash", safe(&member.name))), hash)?;
        }
        Ok(())
    }

    /// Resolve a member's absolute path.
    pub fn member_path(&self, root: &Path, member: &Member) -> PathBuf {
        root.join(&member.path)
    }
}

fn safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Parser (small, hand-written)
// ---------------------------------------------------------------------------

fn parse(src: &str) -> anyhow::Result<Workspace> {
    let toks = tokenize(src);
    let mut i = 0;
    let next = |i: &mut usize| -> Option<&String> {
        let t = toks.get(*i);
        if t.is_some() {
            *i += 1;
        }
        t
    };

    if next(&mut i).map(String::as_str) != Some("workspace") {
        anyhow::bail!("workspace file must start with `workspace \"<name>\"`");
    }
    let name = next(&mut i)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("workspace name expected"))?;

    let mut members = Vec::new();
    while i < toks.len() {
        let kw = toks[i].clone();
        i += 1;
        if kw != "member" {
            anyhow::bail!("expected `member`, found `{kw}`");
        }
        let mname = toks
            .get(i)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("member name expected"))?;
        i += 1;
        if toks.get(i).map(String::as_str) != Some("{") {
            anyhow::bail!("expected `{{` after member `{mname}`");
        }
        i += 1;

        let mut path = String::new();
        let mut needs = Vec::new();
        while toks.get(i).map(String::as_str) != Some("}") {
            let field = toks
                .get(i)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("unclosed member `{mname}`"))?;
            i += 1;
            match field.as_str() {
                "path" => {
                    path = toks
                        .get(i)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("path value expected"))?;
                    i += 1;
                }
                "needs" => {
                    if toks.get(i).map(String::as_str) != Some("[") {
                        anyhow::bail!("expected `[` after needs");
                    }
                    i += 1;
                    while toks.get(i).map(String::as_str) != Some("]") {
                        let t = toks
                            .get(i)
                            .cloned()
                            .ok_or_else(|| anyhow::anyhow!("unclosed needs list"))?;
                        i += 1;
                        if t == "," {
                            continue;
                        }
                        needs.push(t);
                    }
                    i += 1; // consume ]
                }
                other => anyhow::bail!("unknown member field `{other}`"),
            }
        }
        i += 1; // consume }

        if path.is_empty() {
            anyhow::bail!("member `{mname}` has no path");
        }
        members.push(Member {
            name: mname,
            path,
            needs,
        });
    }

    Ok(Workspace { name, members })
}

/// Tokenize into words, quoted strings, and the structural chars `{}[],`.
fn tokenize(src: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut chars = src.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            '#' => {
                while let Some(&c) = chars.peek() {
                    if c == '\n' {
                        break;
                    }
                    chars.next();
                }
            }
            '{' | '}' | '[' | ']' | ',' => {
                toks.push(c.to_string());
                chars.next();
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                    s.push(c);
                }
                toks.push(s);
            }
            _ => {
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || "{}[],\"#".contains(c) {
                        break;
                    }
                    s.push(c);
                    chars.next();
                }
                toks.push(s);
            }
        }
    }
    toks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_members_and_needs() {
        let src = r#"
            workspace "backend"
            member shared  { path "shared" }
            member auth    { path "services/auth"    needs [ shared ] }
            member gateway { path "services/gateway" needs [ auth, shared ] }
        "#;
        let ws = parse(src).unwrap();
        assert_eq!(ws.name, "backend");
        assert_eq!(ws.members.len(), 3);
        let gw = ws.members.iter().find(|m| m.name == "gateway").unwrap();
        assert_eq!(gw.path, "services/gateway");
        assert_eq!(gw.needs, vec!["auth", "shared"]);
    }

    #[test]
    fn orders_by_dependencies() {
        let src = r#"
            workspace "w"
            member gateway { path "g" needs [ auth ] }
            member auth    { path "a" needs [ shared ] }
            member shared  { path "s" }
        "#;
        let ws = parse(src).unwrap();
        let order: Vec<String> = ws.ordered().unwrap().into_iter().map(|m| m.name).collect();
        assert!(order.iter().position(|n| n == "shared") < order.iter().position(|n| n == "auth"));
        assert_eq!(order.last().unwrap(), "gateway");
    }

    #[test]
    fn rejects_cyclic_workspace() {
        let src = r#"
            workspace "w"
            member a { path "a" needs [ b ] }
            member b { path "b" needs [ a ] }
        "#;
        let ws = parse(src).unwrap();
        assert!(ws.ordered().is_err());
    }
}
