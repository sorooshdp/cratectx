use std::collections::BTreeMap;
use std::path::Path;

/// Generate a tree-style directory structure from file paths
pub fn generate_tree(file_paths: &[String]) -> String {
    if file_paths.is_empty() {
        return String::new();
    }

    let mut tree = String::from("Directory structure:\n");
    let mut dir_structure: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();

    // Build directory structure
    for path in file_paths {
        let parts: Vec<String> = Path::new(path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();

        if parts.is_empty() {
            continue;
        }

        let file_name = parts.last().unwrap().clone();
        let dir_path = parts[..parts.len().saturating_sub(1)].to_vec();

        dir_structure
            .entry(dir_path)
            .or_insert_with(Vec::new)
            .push(file_name);
    }

    // Sort files within each directory
    for files in dir_structure.values_mut() {
        files.sort();
    }

    // Render tree
    render_tree(&mut tree, &dir_structure, &[], 0);
    tree.push('\n');
    tree
}

fn render_tree(
    output: &mut String,
    structure: &BTreeMap<Vec<String>, Vec<String>>,
    current_path: &[String],
    depth: usize,
) {
    let indent = "  ".repeat(depth);

    // Get files in current directory
    if let Some(files) = structure.get(current_path) {
        for file in files {
            output.push_str(&format!("{}├── {}\n", indent, file));
        }
    }

    // Get subdirectories
    let mut subdirs: Vec<Vec<String>> = structure
        .keys()
        .filter(|path| {
            path.len() == current_path.len() + 1
                && path.starts_with(current_path)
        })
        .cloned()
        .collect();

    subdirs.sort();

    for (i, subdir) in subdirs.iter().enumerate() {
        let is_last = i == subdirs.len() - 1;
        let dir_name = subdir.last().unwrap();
        let prefix = if is_last { "└──" } else { "├──" };

        output.push_str(&format!("{}{} {}/\n", indent, prefix, dir_name));
        render_tree(output, structure, subdir, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_tree_empty() {
        let paths: Vec<String> = vec![];
        let tree = generate_tree(&paths);
        assert_eq!(tree, "");
    }

    #[test]
    fn test_generate_tree_single_file() {
        let paths = vec!["main.rs".to_string()];
        let tree = generate_tree(&paths);
        assert!(tree.contains("Directory structure:"));
        assert!(tree.contains("├── main.rs"));
    }

    #[test]
    fn test_generate_tree_multiple_files_same_dir() {
        let paths = vec![
            "lib.rs".to_string(),
            "main.rs".to_string(),
            "utils.rs".to_string(),
        ];
        let tree = generate_tree(&paths);
        assert!(tree.contains("Directory structure:"));
        assert!(tree.contains("├── lib.rs"));
        assert!(tree.contains("├── main.rs"));
        assert!(tree.contains("├── utils.rs"));
    }

    #[test]
    fn test_generate_tree_nested_directories() {
        let paths = vec![
            "src/main.rs".to_string(),
            "src/lib.rs".to_string(),
            "tests/integration.rs".to_string(),
        ];
        let tree = generate_tree(&paths);
        assert!(tree.contains("Directory structure:"));
        assert!(tree.contains("src/"));
        assert!(tree.contains("tests/"));
        assert!(tree.contains("main.rs"));
        assert!(tree.contains("lib.rs"));
        assert!(tree.contains("integration.rs"));
    }

    #[test]
    fn test_generate_tree_deep_nesting() {
        let paths = vec![
            "src/api/routes/user.rs".to_string(),
            "src/api/routes/auth.rs".to_string(),
            "src/api/middleware.rs".to_string(),
            "src/main.rs".to_string(),
        ];
        let tree = generate_tree(&paths);
        assert!(tree.contains("Directory structure:"));
        assert!(tree.contains("src/"));
        assert!(tree.contains("api/"));
        assert!(tree.contains("routes/"));
        assert!(tree.contains("user.rs"));
        assert!(tree.contains("auth.rs"));
        assert!(tree.contains("middleware.rs"));
        assert!(tree.contains("main.rs"));
    }

    #[test]
    fn test_generate_tree_mixed_depths() {
        let paths = vec![
            "README.md".to_string(),
            "src/main.rs".to_string(),
            "src/utils/helper.rs".to_string(),
            "docs/guide.md".to_string(),
        ];
        let tree = generate_tree(&paths);
        assert!(tree.contains("Directory structure:"));
        assert!(tree.contains("README.md"));
        assert!(tree.contains("src/"));
        assert!(tree.contains("main.rs"));
        assert!(tree.contains("utils/"));
        assert!(tree.contains("helper.rs"));
        assert!(tree.contains("docs/"));
        assert!(tree.contains("guide.md"));
    }

    #[test]
    fn test_generate_tree_sorting() {
        let paths = vec![
            "z.rs".to_string(),
            "a.rs".to_string(),
            "m.rs".to_string(),
        ];
        let tree = generate_tree(&paths);
        let a_pos = tree.find("a.rs").unwrap();
        let m_pos = tree.find("m.rs").unwrap();
        let z_pos = tree.find("z.rs").unwrap();
        assert!(a_pos < m_pos, "a.rs should come before m.rs");
        assert!(m_pos < z_pos, "m.rs should come before z.rs");
    }

    #[test]
    fn test_generate_tree_with_multiple_nested_levels() {
        let paths = vec![
            "a/b/c/deep.rs".to_string(),
            "a/b/mid.rs".to_string(),
            "a/shallow.rs".to_string(),
            "root.rs".to_string(),
        ];
        let tree = generate_tree(&paths);
        assert!(tree.contains("root.rs"));
        assert!(tree.contains("a/"));
        assert!(tree.contains("shallow.rs"));
        assert!(tree.contains("b/"));
        assert!(tree.contains("mid.rs"));
        assert!(tree.contains("c/"));
        assert!(tree.contains("deep.rs"));
    }

    #[test]
    fn test_generate_tree_special_chars_in_names() {
        let paths = vec![
            "file-with-dash.rs".to_string(),
            "file_with_underscore.rs".to_string(),
            "file.multiple.dots.rs".to_string(),
        ];
        let tree = generate_tree(&paths);
        assert!(tree.contains("file-with-dash.rs"));
        assert!(tree.contains("file_with_underscore.rs"));
        assert!(tree.contains("file.multiple.dots.rs"));
    }

    #[test]
    fn test_generate_tree_realistic_project() {
        let paths = vec![
            "Cargo.toml".to_string(),
            "README.md".to_string(),
            "src/main.rs".to_string(),
            "src/lib.rs".to_string(),
            "src/cli.rs".to_string(),
            "src/walker.rs".to_string(),
            "src/formatter.rs".to_string(),
            "tests/integration_test.rs".to_string(),
        ];
        let tree = generate_tree(&paths);
        
        // Verify structure
        assert!(tree.contains("Directory structure:"));
        assert!(tree.contains("Cargo.toml"));
        assert!(tree.contains("README.md"));
        assert!(tree.contains("src/"));
        assert!(tree.contains("tests/"));
        
        // Verify all files are present
        for path in &paths {
            let filename = Path::new(path).file_name().unwrap().to_str().unwrap();
            assert!(tree.contains(filename), "Tree should contain {}", filename);
        }
    }
}
