use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::{Component, Path};

/// Generate a tree-style directory structure from file paths.
pub fn generate_tree(file_paths: &[String]) -> String {
    if file_paths.is_empty() {
        return String::new();
    }

    let mut root = Node::default();
    for path in file_paths {
        root.insert(path);
    }

    let mut out = String::from("Directory structure:\n");
    render(&root, "", &mut out);
    out.push('\n');
    out
}

#[derive(Debug, Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: BTreeSet<String>,
}

impl Node {
    fn insert(&mut self, path: &str) {
        // Only keep normal components: drops `.`, `..`, and any root prefix.
        let parts: Vec<String> = Path::new(path)
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();

        let Some((file, dirs)) = parts.split_last() else {
            return;
        };

        let mut node = self;
        for dir in dirs {
            node = node.dirs.entry(dir.clone()).or_default();
        }
        node.files.insert(file.clone());
    }
}

/// Renders directories first, then files, each group sorted by name.
fn render(node: &Node, prefix: &str, out: &mut String) {
    let total = node.dirs.len() + node.files.len();
    let mut index = 0;

    let connector = |is_last: bool| if is_last { "└──" } else { "├──" };

    for (name, child) in &node.dirs {
        index += 1;
        let is_last = index == total;
        let _ = writeln!(out, "{prefix}{} {name}/", connector(is_last));
        let child_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
        render(child, &child_prefix, out);
    }

    for name in &node.files {
        index += 1;
        let _ = writeln!(out, "{prefix}{} {name}", connector(index == total));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(paths: &[&str]) -> String {
        generate_tree(&paths.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn empty_input_produces_empty_output() {
        assert_eq!(tree(&[]), "");
    }

    /// Regression: intermediate directories that contain no files of their own
    /// used to be invisible to the renderer, so this path produced no output at all.
    #[test]
    fn renders_files_nested_under_empty_intermediate_dirs() {
        let expected = "\
Directory structure:
└── src/
    └── components/
        └── Button/
            └── index.tsx

";
        assert_eq!(tree(&["src/components/Button/index.tsx"]), expected);
    }

    #[test]
    fn renders_correct_connectors_and_continuation_lines() {
        let paths = [
            "Cargo.toml",
            "README.md",
            "src/main.rs",
            "src/lib.rs",
            "src/api/routes.rs",
            "tests/it.rs",
        ];
        let expected = "\
Directory structure:
├── src/
│   ├── api/
│   │   └── routes.rs
│   ├── lib.rs
│   └── main.rs
├── tests/
│   └── it.rs
├── Cargo.toml
└── README.md

";
        assert_eq!(tree(&paths), expected);
    }

    #[test]
    fn sorts_dedups_and_strips_leading_dot_slash() {
        let expected = "\
Directory structure:
├── a.rs
├── m.rs
└── z.rs

";
        assert_eq!(tree(&["z.rs", "a.rs", "a.rs", "./m.rs"]), expected);
    }
}