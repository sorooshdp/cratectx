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
        }
    }
}

impl ImportVisitor {
    /// Directly walks `syn::UseTree` without invoking `quote!`, avoiding token stream
    /// formatting and supporting nested group imports (`use crate::{a::b, c};`).
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

/// Pre-indexed lookup structure that turns O(Files) per-import resolution into O(1) lookups.
struct ModuleIndex {
    /// Exact normalized path to original file path
    exact_paths: HashMap<String, String>,
    /// Suffix without `.rs` (e.g. `api/handler`, `src/api/handler`) to original file path
    suffix_to_path: HashMap<String, String>,
    /// File stem (e.g. `handler`) to original file path
    stem_to_path: HashMap<String, String>,
}

impl ModuleIndex {
    fn build<F: SourceFile>(files: &[F]) -> Self {
        let mut exact_paths = HashMap::with_capacity(files.len());
        let mut suffix_to_path = HashMap::with_capacity(files.len() * 2);
        let mut stem_to_path = HashMap::with_capacity(files.len());

        for file in files {
            let orig = file.file_path();
            let normalized = orig.replace('\\', "/");
            exact_paths.insert(normalized.clone(), orig.to_string());

            let trimmed = strip_rust_extension(&normalized);
            // Index all trailing slash suffixes so `ends_with("/{suffix}")` is O(1)
            suffix_to_path
                .entry(trimmed.to_string())
                .or_insert_with(|| orig.to_string());
            for (idx, ch) in trimmed.char_indices() {
                if ch == '/' {
                    let sub = &trimmed[idx + 1..];
                    if !sub.is_empty() {
                        suffix_to_path
                            .entry(sub.to_string())
                            .or_insert_with(|| orig.to_string());
                    }
                }
            }

            if let Some(stem) = Path::new(&normalized).file_stem().and_then(|s| s.to_str()) {
                stem_to_path
                    .entry(stem.to_string())
                    .or_insert_with(|| orig.to_string());
            }
        }

        Self {
            exact_paths,
            suffix_to_path,
            stem_to_path,
        }
    }

    fn resolve(&self, import: &str, current_file: &str) -> Option<String> {
        let normalized = import.trim();
        if normalized.is_empty() {
            return None;
        }

        let segments: Vec<&str> = normalized
            .trim_start_matches("crate::")
            .trim_start_matches("self::")
            .trim_start_matches("super::")
            .split("::")
            .filter(|part| !part.trim().is_empty())
            .collect();

        if segments.is_empty() {
            return None;
        }

        // 1. Check full or partial module path suffix (e.g. `crate::api::routes::handler` -> `api/routes` or `api/routes/handler`)
        for len in (1..=segments.len()).rev() {
            let suffix = segments[..len].join("/");
            if let Some(target) = self.suffix_to_path.get(&suffix) {
                return Some(target.clone());
            }
        }

        // 2. Check file stem match for any segment
        for &seg in &segments {
            if let Some(target) = self.stem_to_path.get(seg) {
                return Some(target.clone());
            }
        }

        // 3. Relative `super::` fallback
        if normalized.starts_with("super::") {
            let current = Path::new(current_file);
            let parent = current.parent().unwrap_or_else(|| Path::new("."));
            let mut candidate = parent.to_path_buf();
            for &segment in &segments {
                candidate.push(segment);
            }
            let candidate_str = candidate
                .with_extension("rs")
                .to_string_lossy()
                .replace('\\', "/");

            if let Some(target) = self.exact_paths.get(&candidate_str) {
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
}
