use anyhow::{Context, Result};
use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use quote::quote;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};
use syn::visit::Visit;
use syn::{File, ItemMod, ItemUse};

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
        if !self.graph.contains_edge(from, to) {
            self.graph.add_edge(from, to, ());
        }
    }

    pub fn get_dependencies(&self, file_path: &str, max_depth: usize) -> Vec<String> {
        let Some(&start_node) = self.path_to_node.get(file_path) else {
            return vec![];
        };

        let mut visited = HashSet::new();
        let mut result = Vec::new();
        self.traverse_dependencies(start_node, 0, max_depth, &mut visited, &mut result);
        result
    }

    fn traverse_dependencies(
        &self,
        node: NodeIndex,
        depth: usize,
        max_depth: usize,
        visited: &mut HashSet<NodeIndex>,
        result: &mut Vec<String>,
    ) {
        if depth > max_depth || visited.contains(&node) {
            return;
        }

        visited.insert(node);
        if depth > 0 {
            if let Some(node_data) = self.graph.node_weight(node) {
                result.push(node_data.path.clone());
            }
        }

        for neighbor in self.graph.neighbors_directed(node, Direction::Outgoing) {
            self.traverse_dependencies(neighbor, depth + 1, max_depth, visited, result);
        }
    }

    pub fn get_dependents(&self, file_path: &str, max_depth: usize) -> Vec<String> {
        let Some(&start_node) = self.path_to_node.get(file_path) else {
            return vec![];
        };

        let mut visited = HashSet::new();
        let mut result = Vec::new();
        self.traverse_dependents(start_node, 0, max_depth, &mut visited, &mut result);
        result
    }

    fn traverse_dependents(
        &self,
        node: NodeIndex,
        depth: usize,
        max_depth: usize,
        visited: &mut HashSet<NodeIndex>,
        result: &mut Vec<String>,
    ) {
        if depth > max_depth || visited.contains(&node) {
            return;
        }

        visited.insert(node);
        if depth > 0 {
            if let Some(node_data) = self.graph.node_weight(node) {
                result.push(node_data.path.clone());
            }
        }

        for neighbor in self.graph.neighbors_directed(node, Direction::Incoming) {
            self.traverse_dependents(neighbor, depth + 1, max_depth, visited, result);
        }
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

    let mut visitor = ImportVisitor { imports: Vec::new() };
    visitor.visit_file(&syntax_tree);
    Ok(visitor.imports)
}

struct ImportVisitor {
    imports: Vec<String>,
}

impl<'ast> Visit<'ast> for ImportVisitor {
    fn visit_item_use(&mut self, node: &'ast ItemUse) {
        let use_tree_string = quote!(#node).to_string();
        if let Some(path) = extract_use_path(&use_tree_string) {
            self.imports.push(path);
        }
    }

    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        if let Some((_, items)) = &node.content {
            for item in items {
                self.visit_item(item);
            }
        }
    }
}

fn extract_use_path(use_statement: &str) -> Option<String> {
    let cleaned = use_statement
        .trim()
        .replace(" :: ", "::")
        .replace(":: ", "::")
        .replace(" ::", "::")
        .replace(' ', "");

    let trimmed = cleaned
        .trim_start_matches("use")
        .trim_end_matches(';')
        .trim();

    if trimmed.is_empty() {
        return None;
    }

    if trimmed.starts_with("std::")
        || trimmed.starts_with("core::")
        || trimmed.starts_with("alloc::")
        || trimmed.starts_with("extern::")
    {
        return None;
    }

    if trimmed.starts_with("crate::")
        || trimmed.starts_with("self::")
        || trimmed.starts_with("super::")
    {
        return Some(trimmed.to_string());
    }

    let first = trimmed.split("::").next()?;
    if first.is_empty() || first.contains('.') || first.contains('-') {
        return None;
    }

    Some(trimmed.to_string())
}

pub fn build_dependency_graph(
    files: &[(String, String)],
    base_path: &Path,
) -> Result<DependencyGraph> {
    let mut graph = DependencyGraph::new();
    let mut file_to_module = HashMap::new();

    for (file_path, _) in files {
        let module_path = path_to_module_path(file_path, base_path);
        file_to_module.insert(file_path.clone(), module_path.clone());
        graph.add_file(file_path.clone(), module_path);
    }

    for (file_path, content) in files {
        if !file_path.ends_with(".rs") {
            continue;
        }

        let imports = parse_rust_imports(content).unwrap_or_default();
        let Some(&from_idx) = graph.path_to_node.get(file_path) else {
            continue;
        };

        for import in imports {
            if let Some(target_path) = resolve_import(&import, file_path, &file_to_module) {
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

fn resolve_import(
    import: &str,
    current_file: &str,
    file_to_module: &HashMap<String, Vec<String>>,
) -> Option<String> {
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

    for (file_path, _) in file_to_module {
        let file_path_normalized = file_path.replace('\\', "/");
        let trimmed = strip_rust_extension(&file_path_normalized);
        let suffix = segments.join("/");

        if trimmed.ends_with(&format!("/{suffix}")) || trimmed == suffix {
            return Some(file_path.clone());
        }

        if let Some(file_stem) = Path::new(&file_path_normalized)
            .file_stem()
            .and_then(|stem| stem.to_str())
        {
            if segments.iter().any(|segment| *segment == file_stem) {
                return Some(file_path.clone());
            }
        }
    }

    if normalized.starts_with("super::") {
        let current = Path::new(current_file);
        let parent = current.parent().unwrap_or_else(|| Path::new("."));
        let mut candidate = parent.to_path_buf();
        for segment in segments.iter().copied() {
            candidate.push(segment);
        }
        let candidate_str = candidate.with_extension("rs")
            .to_string_lossy()
            .replace('\\', "/");

        if file_to_module.contains_key(&candidate_str) {
            return Some(candidate_str);
        }
    }

    None
}

fn strip_rust_extension(path: &str) -> String {
    path.strip_suffix(".rs")
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string())
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
    }

    #[test]
    fn test_build_dependency_graph_integration() {
        let files = vec![
            ("src/main.rs".to_string(), "use crate::lib;\nfn main() { lib::foo(); }".to_string()),
            ("src/lib.rs".to_string(), "pub fn foo() {}".to_string()),
            ("src/unrelated.rs".to_string(), "pub fn noop() {}".to_string()),
        ];

        let graph = build_dependency_graph(&files, Path::new(".")).unwrap();
        let deps = graph.get_dependencies("src/main.rs", 2);
        assert!(deps.iter().any(|path| path == "src/lib.rs"));
        assert!(!deps.iter().any(|path| path == "src/unrelated.rs"));
    }

    #[test]
    fn test_path_to_module_path() {
        let base = Path::new("/project");
        let result = path_to_module_path("/project/src/api/handler.rs", base);
        assert_eq!(result, vec!["src", "api", "handler"]);
    }
}
