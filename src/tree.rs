use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::{Component, Path};

/// Generate a tree-style directory structure from file paths.
pub fn generate_tree<S: AsRef<str>>(file_paths: &[S]) -> String {
    if file_paths.is_empty() {
        return String::new();
    }

    let mut root = Node::default();
    for path in file_paths {
        root.insert(path.as_ref());
    }

    // Pre-allocate approximately 32 bytes per path entry
    let mut out = String::with_capacity(24 + file_paths.len() * 32);
    out.push_str("Directory structure:\n");

    let mut prefix = String::with_capacity(64);
    render(&root, &mut prefix, &mut out);
    out.push('\n');
    out
}

#[derive(Debug, Default)]
struct Node<'a> {
    dirs: BTreeMap<&'a str, Node<'a>>,
    files: BTreeSet<&'a str>,
}

impl<'a> Node<'a> {
    fn insert(&mut self, path: &'a str) {
        let mut components = Path::new(path)
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .peekable();

        let mut node = self;
        while let Some(part) = components.next() {
            if components.peek().is_some() {
                node = node.dirs.entry(part).or_default();
            } else {
                node.files.insert(part);
            }
        }
    }
}

/// Renders directories first, then files, each group sorted by name.
/// Reuses `prefix` buffer across recursive calls to avoid per-level String allocations.
fn render(node: &Node<'_>, prefix: &mut String, out: &mut String) {
    let total = node.dirs.len() + node.files.len();
    let mut index = 0;

    let connector = |is_last: bool| if is_last { "└──" } else { "├──" };

    for (name, child) in &node.dirs {
        index += 1;
        let is_last = index == total;
        let _ = writeln!(out, "{prefix}{} {name}/", connector(is_last));

        let prev_len = prefix.len();
        prefix.push_str(if is_last { "    " } else { "│   " });
        render(child, prefix, out);
        prefix.truncate(prev_len);
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
        generate_tree(paths)
    }

    #[test]
    fn empty_input_produces_empty_output() {
        assert_eq!(tree(&[]), "");
    }

    #[test]
    fn works_with_owned_strings_and_str_slices() {
        let owned = vec!["src/main.rs".to_string(), "src/lib.rs".to_string()];
        let borrowed = ["src/main.rs", "src/lib.rs"];
        assert_eq!(generate_tree(&owned), generate_tree(&borrowed));
    }

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