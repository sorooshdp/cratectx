use crate::walker::FileEntry;
use anyhow::{Context, Result};
use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Component, Path};
use syn::visit::Visit;
use syn::{File, ItemMod, ItemUse, UseTree};

/// Trait allowing `build_dependency_graph` to accept both `FileEntry` slices (zero-copy)
/// and `(String, String)` tuples.
pub trait SourceFile {
    fn file_path(&self) -> &str;
    fn file_content(&self) -> &str;
}

impl SourceFile for FileEntry {
    #[inline]
    fn file_path(&self) -> &str {
        &self.path
    }
    #[inline]
    fn file_content(&self) -> &str {
        &self.content
    }
}

impl SourceFile for (String, String) {
    #[inline]
    fn file_path(&self) -> &str {
        &self.0
    }
    #[inline]
    fn file_content(&self) -> &str {
        &self.1
    }
}

impl SourceFile for (&str, &str) {
    #[inline]
    fn file_path(&self) -> &str {
        self.0
    }
    #[inline]
    fn file_content(&self) -> &str {
        self.1
    }
}

#[derive(Debug, Clone)]
pub struct DependencyNode {
    pub path: String,
    pub module_path: Vec<String>,
}

#[derive(Debug, Default)]
pub struct DependencyGraph {
    graph: DiGraph<DependencyNode, ()>,
    pub path_to_node: HashMap<String, NodeIndex>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_file(&mut self, file_path: String, module_path: Vec<String>) -> NodeIndex {
        if let Some(&idx) = self.path_to_node.get(&file_path) {
            return idx;
        }

        let node = DependencyNode {
            path: file_path.clone(),
            module_path,
        };
        let idx = self.graph.add_node(node);
        self.path_to_node.insert(file_path, idx);
        idx
    }

    pub fn add_dependency(&mut self, from: NodeIndex, to: NodeIndex) {
        if from != to && !self.graph.contains_edge(from, to) {
            self.graph.add_edge(from, to, ());
        }
    }

    pub fn get_dependencies(&self, file_path: &str, max_depth: usize) -> Vec<String> {
        self.bfs_traverse(file_path, max_depth, Direction::Outgoing)
    }

    pub fn get_dependents(&self, file_path: &str, max_depth: usize) -> Vec<String> {
        self.bfs_traverse(file_path, max_depth, Direction::Incoming)
    }

    /// Breadth-first traversal guarantees shortest-path discovery up to `max_depth`,
    /// fixing the DFS bug where visiting a node via a deep branch prevented exploring
    /// its neighbors when later reached via a shallower branch.
    fn bfs_traverse(&self, file_path: &str, max_depth: usize, direction: Direction) -> Vec<String> {
        let Some(&start_node) = self.path_to_node.get(file_path) else {
            return vec![];
        };

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        visited.insert(start_node);
        queue.push_back((start_node, 0usize));

        while let Some((node, depth)) = queue.pop_front() {
            if depth > 0 {
                if let Some(node_data) = self.graph.node_weight(node) {
                    result.push(node_data.path.clone());
                }
            }

            if depth >= max_depth {
                continue;
            }

            for neighbor in self.graph.neighbors_directed(node, direction) {
                if visited.insert(neighbor) {
                    queue.push_back((neighbor, depth + 1));
                }
            }
        }

        result
    }

    pub fn all_files(&self) -> Vec<String> {
        self.graph
            .node_weights()
            .map(|node| node.path.clone())
            .collect()
    }
}

pub fn parse_rust_imports(content: &str) -> Result<Vec<String>> {
    let syntax_tree: File = syn::parse_file(content).context("Failed to parse Rust file")?;

    let mut visitor = ImportVisitor {
        imports: Vec::new(),
    };
    visitor.visit_file(&syntax_tree);
    Ok(visitor.imports)
}

struct ImportVisitor {
    imports: Vec<String>,
}

impl<'ast> Visit<'ast> for ImportVisitor {
    fn visit_item_use(&mut self, node: &'ast ItemUse) {
        let mut prefix = Vec::new();
        self.collect_use_tree(&node.tree, &mut prefix);
    }

    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        if let Some((_, items)) = &node.content {
            for item in items {
                self.visit_item(item);
            }
        } else {
            // External module declaration: `mod foo;` or `pub mod foo;`
            self.imports.push(format!("self::{}", node.ident));
        }
    }
}

impl ImportVisitor {
    fn collect_use_tree(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.collect_use_tree(&path.tree, prefix);
                prefix.pop();
            }
            UseTree::Name(name) => {
                prefix.push(name.ident.to_string());
                self.record_path(prefix);
                prefix.pop();
            }
            UseTree::Rename(rename) => {
                prefix.push(rename.ident.to_string());
                self.record_path(prefix);
                prefix.pop();
            }
            UseTree::Glob(_) => {
                self.record_path(prefix);
            }
            UseTree::Group(group) => {
                for item in &group.items {
                    self.collect_use_tree(item, prefix);
                }
            }
        }
    }

    fn record_path(&mut self, segments: &[String]) {
        let Some(first) = segments.first().map(String::as_str) else {
            return;
        };
        if matches!(first, "std" | "core" | "alloc" | "extern") {
            return;
        }
        if first.is_empty() || first.contains('.') || first.contains('-') {
            return;
        }
        self.imports.push(segments.join("::"));
    }
}

/// Pre-indexed lookup structure that accurately maps Rust module paths (`foo/bar`, `foo/bar/mod`)
/// without false-positive stem collisions on external crates.
struct ModuleIndex {
    exact_paths: HashMap<String, String>,
    suffix_to_path: HashMap<String, String>,
    local_roots: HashSet<String>,
}

impl ModuleIndex {
    fn build<F: SourceFile>(files: &[F]) -> Self {
        let mut exact_paths = HashMap::with_capacity(files.len());
        let mut suffix_to_path = HashMap::with_capacity(files.len() * 3);
        let mut local_roots = HashSet::new();

        for file in files {
            let orig = file.file_path();
            let normalized = orig.replace('\\', "/");
            exact_paths.insert(normalized.clone(), orig.to_string());

            let trimmed = strip_rust_extension(&normalized);
            // If this is `src/network/mod.rs`, also index it under `src/network` and `network`
            let mod_trimmed = trimmed.strip_suffix("/mod").unwrap_or(trimmed);

            for candidate in [trimmed, mod_trimmed] {
                suffix_to_path
                    .entry(candidate.to_string())
                    .or_insert_with(|| orig.to_string());

                for (idx, ch) in candidate.char_indices() {
                    if ch == '/' {
                        let sub = &candidate[idx + 1..];
                        if !sub.is_empty() {
                            suffix_to_path
                                .entry(sub.to_string())
                                .or_insert_with(|| orig.to_string());
                        }
                    }
                }
            }

            // Record known local module names so bare imports only resolve to local modules
            let rel_src = mod_trimmed.strip_prefix("src/").unwrap_or(mod_trimmed);
            if let Some(first_seg) = rel_src.split('/').next() {
                if !first_seg.is_empty() {
                    local_roots.insert(first_seg.to_string());
                }
            }
        }

        Self {
            exact_paths,
            suffix_to_path,
            local_roots,
        }
    }

    fn resolve(&self, import: &str, current_file: &str) -> Option<String> {
        let normalized = import.trim();
        if normalized.is_empty() {
            return None;
        }

        let current_norm = current_file.replace('\\', "/");
        let current_dir = Path::new(&current_norm)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("");

        // 1. Handle relative `self::` or `super::` imports
        if normalized.starts_with("self::") || normalized.starts_with("super::") {
            let mut base_dir = Path::new(&current_norm)
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_path_buf();

            // In Rust, `mod foo;` inside `src/bar.rs` (not `mod.rs`/`lib.rs`/`main.rs`)
            // looks in `src/bar/foo.rs` or `src/bar.rs`'s directory.
            let file_stem = Path::new(&current_norm)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            let mut rest = normalized;
            if let Some(after_self) = rest.strip_prefix("self::") {
                rest = after_self;
                if !matches!(file_stem, "lib" | "main" | "mod") {
                    let sub_dir = base_dir.join(file_stem);
                    let first_seg = rest.split("::").next().unwrap_or("");
                    let sub_candidate = sub_dir
                        .join(first_seg)
                        .with_extension("rs")
                        .to_string_lossy()
                        .replace('\\', "/");
                    if let Some(target) = self.exact_paths.get(&sub_candidate) {
                        return Some(target.clone());
                    }
                }
            } else {
                while let Some(after_super) = rest.strip_prefix("super::") {
                    rest = after_super;
                    if !matches!(file_stem, "lib" | "main" | "mod") {
                        // First `super::` from `src/a/b.rs` goes to `src/a/` (which is `base_dir`)
                    } else {
                        base_dir.pop();
                    }
                }
            }

            let segments: Vec<&str> = rest.split("::").filter(|s| !s.is_empty()).collect();
            for len in (1..=segments.len()).rev() {
                let rel_suffix = segments[..len].join("/");
                let candidate = if base_dir.as_os_str().is_empty() {
                    rel_suffix.clone()
                } else {
                    format!("{}/{}", base_dir.to_string_lossy().replace('\\', "/"), rel_suffix)
                };

                for probe in [
                    format!("{candidate}.rs"),
                    format!("{candidate}/mod.rs"),
                ] {
                    if let Some(target) = self.exact_paths.get(&probe) {
                        return Some(target.clone());
                    }
                }
                if let Some(target) = self.suffix_to_path.get(&candidate) {
                    return Some(target.clone());
                }
            }
            return None;
        }

        // 2. Handle `crate::` or bare local module imports
        let is_crate_prefixed = normalized.starts_with("crate::");
        let stripped = normalized.trim_start_matches("crate::");
        let segments: Vec<&str> = stripped
            .split("::")
            .filter(|part| !part.trim().is_empty())
            .collect();

        if segments.is_empty() {
            return None;
        }

        // Reject external crate imports (e.g., `serde::de::Error` or `anyhow::Context`)
        // unless prefixed with `crate::` or starting with a known local root module
        // (or crate name `cratectx`).
        let effective_segments = if !is_crate_prefixed {
            if segments[0] == "cratectx" && segments.len() > 1 {
                &segments[1..]
            } else if self.local_roots.contains(segments[0]) {
                &segments[..]
            } else {
                return None;
            }
        } else {
            &segments[..]
        };

        // Match from longest module path prefix to shortest (e.g. `api::routes::Handler` -> `api/routes`)
        for len in (1..=effective_segments.len()).rev() {
            let suffix = effective_segments[..len].join("/");
            if !current_dir.is_empty() {
                let scoped = format!("{current_dir}/{suffix}");
                if let Some(target) = self.suffix_to_path.get(&scoped) {
                    return Some(target.clone());
                }
            }
            if let Some(target) = self.suffix_to_path.get(&suffix) {
                return Some(target.clone());
            }
        }

        None
    }
}

pub fn build_dependency_graph<F: SourceFile>(
    files: &[F],
    base_path: &Path,
) -> Result<DependencyGraph> {
    let mut graph = DependencyGraph::new();
    let index = ModuleIndex::build(files);

    for file in files {
        let path_str = file.file_path();
        let module_path = path_to_module_path(path_str, base_path);
        graph.add_file(path_str.to_string(), module_path);
    }

    for file in files {
        let file_path = file.file_path();
        if !file_path.ends_with(".rs") {
            continue;
        }

        let imports = parse_rust_imports(file.file_content()).unwrap_or_default();
        let Some(&from_idx) = graph.path_to_node.get(file_path) else {
            continue;
        };

        for import in imports {
            if let Some(target_path) = index.resolve(&import, file_path) {
                if let Some(&to_idx) = graph.path_to_node.get(&target_path) {
                    graph.add_dependency(from_idx, to_idx);
                }
            }
        }
    }

    Ok(graph)
}

fn path_to_module_path(file_path: &str, base_path: &Path) -> Vec<String> {
    let path = Path::new(file_path);
    let relative = path.strip_prefix(base_path).unwrap_or(path);
    let without_ext = relative.with_extension("");

    without_ext
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

#[inline]
fn strip_rust_extension(path: &str) -> &str {
    path.strip_suffix(".rs").unwrap_or(path)
}

/// Strips `#[cfg(test)]` modules/items and `#[test]` functions from Rust source code
/// while preserving original formatting and comments.
pub fn strip_rust_tests(source: &str) -> String {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    let mut copy_from = 0;

    while i < len {
        // Skip comments and string literals so we never match inside them
        if let Some(next_i) = skip_non_code(bytes, i) {
            i = next_i;
            continue;
        }

        if bytes[i] == b'#' && is_test_attribute(&source[i..]) {
            // Backtrack horizontal whitespace on the current line
            let mut attr_start = i;
            while attr_start > copy_from
                && matches!(bytes[attr_start - 1], b' ' | b'\t')
            {
                attr_start -= 1;
            }
            out.push_str(&source[copy_from..attr_start]);

            // Advance past all consecutive attributes and the annotated item (; or { ... })
            i = skip_annotated_item(bytes, i);
            // Also consume a single trailing newline if present
            if i < len && bytes[i] == b'\r' {
                i += 1;
            }
            if i < len && bytes[i] == b'\n' {
                i += 1;
            }
            copy_from = i;
            continue;
        }

        i += 1;
    }

    out.push_str(&source[copy_from..]);
    out
}

/// Replaces Rust function and method bodies `{ ... }` with `{ /* ... */ }`,
/// keeping signatures, types, traits, structs, and comments intact.
pub fn collapse_rust_bodies(source: &str) -> String {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    let mut copy_from = 0;

    while i < len {
        if let Some(next_i) = skip_non_code(bytes, i) {
            i = next_i;
            continue;
        }

        // Detect `fn` keyword at identifier boundary
        if i + 2 <= len
            && &bytes[i..i + 2] == b"fn"
            && (i == 0 || !is_ident_char(bytes[i - 1]))
            && (i + 2 == len || !is_ident_char(bytes[i + 2]))
        {
            i += 2;
            let mut seen_params = false;
            let mut paren_depth = 0usize;
            let mut bracket_depth = 0usize;

            while i < len {
                if let Some(next_i) = skip_non_code(bytes, i) {
                    i = next_i;
                    continue;
                }
                match bytes[i] {
                    b'(' => {
                        paren_depth += 1;
                        seen_params = true;
                    }
                    b')' => paren_depth = paren_depth.saturating_sub(1),
                    b'[' => bracket_depth += 1,
                    b']' => bracket_depth = bracket_depth.saturating_sub(1),
                    b';' if seen_params && paren_depth == 0 && bracket_depth == 0 => {
                        // Trait method declaration without body
                        i += 1;
                        break;
                    }
                    b'{' if seen_params && paren_depth == 0 && bracket_depth == 0 => {
                        // Found start of function body block
                        out.push_str(&source[copy_from..=i]);
                        out.push_str(" /* ... */ ");
                        let close_brace = skip_balanced_braces(bytes, i);
                        copy_from = close_brace.saturating_sub(1); // keep closing `}`
                        i = close_brace;
                        break;
                    }
                    _ => {}
                }
                i += 1;
            }
            continue;
        }

        i += 1;
    }

    out.push_str(&source[copy_from..]);
    out
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_test_attribute(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('#') else {
        return false;
    };
    let rest = rest.trim_start();
    let Some(inside) = rest.strip_prefix('[') else {
        return false;
    };
    let inside = inside.trim_start();
    inside.starts_with("cfg(test)")
        || inside.starts_with("test]")
        || inside.starts_with("tokio::test")
}

fn skip_annotated_item(bytes: &[u8], mut i: usize) -> usize {
    let len = bytes.len();
    while i < len {
        if let Some(next_i) = skip_non_code(bytes, i) {
            i = next_i;
            continue;
        }
        match bytes[i] {
            b';' => return i + 1,
            b'{' => return skip_balanced_braces(bytes, i),
            _ => i += 1,
        }
    }
    len
}

fn skip_balanced_braces(bytes: &[u8], mut i: usize) -> usize {
    let len = bytes.len();
    let mut depth = 0usize;
    while i < len {
        if let Some(next_i) = skip_non_code(bytes, i) {
            i = next_i;
            continue;
        }
        if bytes[i] == b'{' {
            depth += 1;
        } else if bytes[i] == b'}' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return i + 1;
            }
        }
        i += 1;
    }
    len
}

/// Skips `//` line comments, `/* */` block comments, `"..."` strings, `r#"..."#` raw strings,
/// and `'c'` character literals. Returns `Some(new_index)` if skipped.
fn skip_non_code(bytes: &[u8], i: usize) -> Option<usize> {
    let len = bytes.len();
    // Line comment
    if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
        let mut j = i + 2;
        while j < len && bytes[j] != b'\n' {
            j += 1;
        }
        return Some(j);
    }
    // Block comment (supports nesting)
    if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
        let mut j = i + 2;
        let mut depth = 1usize;
        while j + 1 < len && depth > 0 {
            if bytes[j] == b'/' && bytes[j + 1] == b'*' {
                depth += 1;
                j += 2;
            } else if bytes[j] == b'*' && bytes[j + 1] == b'/' {
                depth -= 1;
                j += 2;
            } else {
                j += 1;
            }
        }
        return Some(j);
    }
    // Raw string literal r"..." or r#"..."#
    if bytes[i] == b'r' && i + 1 < len && (bytes[i + 1] == b'"' || bytes[i + 1] == b'#') {
        let mut j = i + 1;
        let mut hashes = 0usize;
        while j < len && bytes[j] == b'#' {
            hashes += 1;
            j += 1;
        }
        if j < len && bytes[j] == b'"' {
            j += 1;
            while j < len {
                if bytes[j] == b'"' && j + hashes < len {
                    if bytes[j + 1..=j + hashes].iter().all(|&b| b == b'#') {
                        return Some(j + 1 + hashes);
                    }
                }
                j += 1;
            }
            return Some(len);
        }
    }
    // Normal string literal
    if bytes[i] == b'"' {
        let mut j = i + 1;
        while j < len {
            if bytes[j] == b'\\' {
                j += 2;
            } else if bytes[j] == b'"' {
                return Some(j + 1);
            } else {
                j += 1;
            }
        }
        return Some(len);
    }
    // Character literal (e.g. `'{'`, `'\\'`) vs lifetime (`'a`)
    if bytes[i] == b'\'' && i + 2 < len {
        if bytes[i + 1] == b'\\' && i + 3 < len && bytes[i + 3] == b'\'' {
            return Some(i + 4);
        }
        if bytes[i + 2] == b'\'' {
            return Some(i + 3);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_parse_rust_imports_simple() {
        let code = r#"
            use crate::walker::FileEntry;
            use super::token;
            use std::collections::HashMap;
        "#;

        let imports = parse_rust_imports(code).unwrap();
        assert!(imports.iter().any(|import| import.starts_with("crate::")));
        assert!(imports.iter().any(|import| import.starts_with("super::")));
        assert!(!imports.iter().any(|import| import.starts_with("std::")));
    }

    #[test]
    fn test_parse_rust_imports_grouped() {
        let code = r#"
            use crate::{cli::Cli, walker::{FileEntry, WalkOptions}};
        "#;

        let imports = parse_rust_imports(code).unwrap();
        assert!(imports.contains(&"crate::cli::Cli".to_string()));
        assert!(imports.contains(&"crate::walker::FileEntry".to_string()));
        assert!(imports.contains(&"crate::walker::WalkOptions".to_string()));
    }

    #[test]
    fn test_build_dependency_graph_integration() {
        let files = vec![
            (
                "src/main.rs".to_string(),
                "use crate::lib;\nfn main() { lib::foo(); }".to_string(),
            ),
            ("src/lib.rs".to_string(), "pub fn foo() {}".to_string()),
            (
                "src/unrelated.rs".to_string(),
                "pub fn noop() {}".to_string(),
            ),
        ];

        let graph = build_dependency_graph(&files, Path::new(".")).unwrap();
        let deps = graph.get_dependencies("src/main.rs", 2);
        assert!(deps.iter().any(|path| path == "src/lib.rs"));
        assert!(!deps.iter().any(|path| path == "src/unrelated.rs"));
    }

    #[test]
    fn test_bfs_diamond_graph_respects_shortest_depth() {
        // A -> B -> C -> D (depth 3) AND A -> C (depth 1), so D is at shortest depth 2 from A.
        let mut graph = DependencyGraph::new();
        let a = graph.add_file("a.rs".into(), vec!["a".into()]);
        let b = graph.add_file("b.rs".into(), vec!["b".into()]);
        let c = graph.add_file("c.rs".into(), vec!["c".into()]);
        let d = graph.add_file("d.rs".into(), vec!["d".into()]);

        graph.add_dependency(a, b);
        graph.add_dependency(b, c);
        graph.add_dependency(c, d);
        graph.add_dependency(a, c); // Shortcut: A -> C -> D has depth 2!

        let deps = graph.get_dependencies("a.rs", 2);
        assert!(deps.contains(&"d.rs".to_string()));
    }

    #[test]
    fn test_path_to_module_path() {
        let base = Path::new("/project");
        let result = path_to_module_path("/project/src/api/handler.rs", base);
        assert_eq!(result, vec!["src", "api", "handler"]);
    }

    #[test]
    fn test_no_false_positive_stem_collision_on_external_crates() {
        let files = vec![
            (
                "src/main.rs".to_string(),
                "use serde::de::Error;\nuse anyhow::Context;\nfn main() {}".to_string(),
            ),
            ("src/error.rs".to_string(), "pub struct Error;".to_string()),
            ("src/context.rs".to_string(), "pub struct Context;".to_string()),
        ];

        let graph = build_dependency_graph(&files, Path::new(".")).unwrap();
        let deps = graph.get_dependencies("src/main.rs", 2);
        assert!(deps.is_empty(), "External crates must not match local stems: {deps:?}");
    }

    #[test]
    fn test_resolves_mod_declarations_and_mod_rs() {
        let files = vec![
            ("src/lib.rs".to_string(), "pub mod network;\npub mod cli;".to_string()),
            ("src/network/mod.rs".to_string(), "pub fn connect() {}".to_string()),
            ("src/cli.rs".to_string(), "pub struct Cli;".to_string()),
        ];

        let graph = build_dependency_graph(&files, Path::new(".")).unwrap();
        let deps = graph.get_dependencies("src/lib.rs", 1);
        assert!(deps.contains(&"src/network/mod.rs".to_string()));
        assert!(deps.contains(&"src/cli.rs".to_string()));
    }

    #[test]
    fn test_strip_rust_tests_removes_cfg_test_and_preserves_code() {
        let src = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn it_works() {\n        let s = \"}\";\n        assert_eq!(2 + 2, 4);\n    }\n}\n";
        let stripped = strip_rust_tests(src);
        assert!(stripped.contains("pub fn add"));
        assert!(!stripped.contains("mod tests"));
        assert!(!stripped.contains("it_works"));
    }

    #[test]
    fn test_collapse_rust_bodies_keeps_signatures() {
        let src = "/// Doc comment\npub fn compute(x: usize) -> usize {\n    let y = x * 2;\n    y + 1\n}\n";
        let collapsed = collapse_rust_bodies(src);
        assert!(collapsed.contains("/// Doc comment"));
        assert!(collapsed.contains("pub fn compute(x: usize) -> usize { /* ... */ }"));
        assert!(!collapsed.contains("let y = x * 2"));
    }
}
