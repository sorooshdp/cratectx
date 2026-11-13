pub mod budget;
pub mod cli;
pub mod dependency;
pub mod formatter;
pub mod token;
pub mod tree;
pub mod version;
pub mod walker;

use anyhow::Result;
use budget::PriorityStrategy;
use cli::Cli;
use std::collections::HashSet;
use std::io::{self, Write};
use walker::WalkOptions;

fn normalize_entry_point(entry_point: &std::path::Path, scan_root: &std::path::Path) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();

    let base = if scan_root.is_absolute() {
        scan_root.to_path_buf()
    } else {
        cwd.join(scan_root)
    };

    let absolute_entry = if entry_point.is_absolute() {
        entry_point.to_path_buf()
    } else {
        cwd.join(entry_point)
    };

    absolute_entry
        .strip_prefix(&base)
        .unwrap_or(entry_point)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Main entry point for the library
pub fn run(cli: Cli) -> Result<()> {
    eprintln!("{}", version::app_banner());

    let strategy = PriorityStrategy::from_str(&cli.priority).ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid priority strategy: '{}'. Valid options: first-come, smallest-first, shallow-first, deep-first",
            cli.priority
        )
    })?;

    let walk_opts = WalkOptions::from_cli(&cli);
    let mut files = if cli.stdin_list {
        walker::collect_files_from_stdin(&walk_opts)?
    } else {
        walker::collect_files(&walk_opts)?
    };

    if let Some(entry_point) = &cli.entry_point {
        let entry_point_rel = normalize_entry_point(entry_point, &cli.path);

        if !files.iter().any(|f| f.path == entry_point_rel) {
            return Err(anyhow::anyhow!(
                "Entry point '{}' was not found in the scanned files.",
                entry_point.display()
            ));
        }

        // Pass `&files` directly without cloning file paths or contents
        let graph = dependency::build_dependency_graph(&files, &cli.path)?;
        let mut relevant = HashSet::new();
        relevant.insert(entry_point_rel.clone());
        relevant.extend(graph.get_dependencies(&entry_point_rel, cli.dep_depth));

        if cli.with_dependents {
            relevant.extend(graph.get_dependents(&entry_point_rel, cli.dep_depth));
        }

        files.retain(|f| relevant.contains(&f.path));
    }

    let budget_result =
        budget::apply_budget(files, cli.max_tokens, strategy, token::estimate_tokens);

    if cli.show_excluded && !budget_result.excluded.is_empty() {
        eprintln!("\nExcluded files (token budget exceeded):");
        for file in &budget_result.excluded {
            eprintln!("  - {}", file.path);
        }
        eprintln!();
    } else if !budget_result.excluded.is_empty() {
        eprintln!(
            "Excluded {} file(s) due to token budget. Use --show-excluded to see the list.",
            budget_result.excluded.len()
        );
    }

    let mut output = String::new();

    // Add tree if requested using borrowed &str references (zero String clones)
    if cli.tree {
        let file_paths: Vec<&str> = budget_result
            .included
            .iter()
            .map(|(f, _)| f.path.as_str())
            .collect();
        output.push_str(&tree::generate_tree(&file_paths));
    }

    // Stream formatted file contents directly into `output`
    formatter::format_files_into(&budget_result.included, cli.format, &mut output)?;

    if cli.clipboard {
        #[cfg(not(test))]
        {
            use arboard::Clipboard;
            let mut clipboard = Clipboard::new()?;
            clipboard.set_text(&output)?;
            eprintln!(
                "Copied to clipboard ({} tokens)",
                budget_result.total_tokens
            );
        }
    }

    if let Some(path) = cli.output {
        std::fs::write(&path, &output)?;
        eprintln!(
            "Written to {} ({} tokens)",
            path.display(),
            budget_result.total_tokens
        );
    } else if !cli.clipboard {
        let mut stdout = io::stdout().lock();
        stdout.write_all(output.as_bytes())?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn test_entry_point_normalization_for_scan_root() {
        let scan_root = Path::new("src");
        let entry = Path::new("src/lib.rs");
        assert_eq!(normalize_entry_point(entry, scan_root), "lib.rs");
    }

    #[test]
    fn test_basic_run_with_temp_files() {
        let dir = TempDir::new().unwrap();
        let file1 = dir.path().join("test.rs");
        fs::write(&file1, "fn main() {}").unwrap();

        let cli = Cli {
            path: dir.path().to_path_buf(),
            include: vec![],
            exclude: vec![],
            max_tokens: None,
            priority: "smallest-first".to_string(),
            format: formatter::Format::Xml,
            output: None,
            clipboard: false,
            no_gitignore: false,
            tree: false,
            stdin_list: false,
            show_excluded: false,
            entry_point: None,
            dep_depth: 3,
            with_dependents: false,
        };

        let result = run(cli);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_priority_strategy() {
        let dir = TempDir::new().unwrap();

        let cli = Cli {
            path: dir.path().to_path_buf(),
            include: vec![],
            exclude: vec![],
            max_tokens: None,
            priority: "invalid-strategy".to_string(),
            format: formatter::Format::Xml,
            output: None,
            clipboard: false,
            no_gitignore: false,
            tree: false,
            stdin_list: false,
            show_excluded: false,
            entry_point: None,
            dep_depth: 3,
            with_dependents: false,
        };

        let result = run(cli);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid priority strategy")
        );
    }

    #[test]
    fn test_run_with_tree_output() {
        let dir = TempDir::new().unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();
        fs::write(src_dir.join("lib.rs"), "pub fn foo() {}").unwrap();

        let tests_dir = dir.path().join("tests");
        fs::create_dir(&tests_dir).unwrap();
        fs::write(tests_dir.join("test.rs"), "#[test] fn it_works() {}").unwrap();

        let output_file = dir.path().join("output.txt");

        let cli = Cli {
            path: dir.path().to_path_buf(),
            include: vec![],
            exclude: vec![],
            max_tokens: None,
            priority: "smallest-first".to_string(),
            format: formatter::Format::Xml,
            output: Some(output_file.clone()),
            clipboard: false,
            no_gitignore: false,
            tree: true,
            stdin_list: false,
            show_excluded: false,
            entry_point: None,
            dep_depth: 3,
            with_dependents: false,
        };

        let result = run(cli);
        assert!(result.is_ok());

        let output_content = fs::read_to_string(&output_file).unwrap();
        assert!(output_content.contains("Directory structure:"));
        assert!(output_content.contains("src/"));
        assert!(output_content.contains("tests/"));
        assert!(output_content.contains("main.rs"));
        assert!(output_content.contains("lib.rs"));
        assert!(output_content.contains("test.rs"));
        assert!(output_content.contains("<context>"));
    }

    #[test]
    fn test_smallest_first_priority() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("large.rs"), "x".repeat(1000)).unwrap();
        fs::write(dir.path().join("small.rs"), "x").unwrap();

        let output_file = dir.path().join("output.txt");

        let cli = Cli {
            path: dir.path().to_path_buf(),
            include: vec![],
            exclude: vec![],
            max_tokens: Some(10),
            priority: "smallest-first".to_string(),
            format: formatter::Format::Plain,
            output: Some(output_file.clone()),
            clipboard: false,
            no_gitignore: false,
            tree: false,
            stdin_list: false,
            show_excluded: false,
            entry_point: None,
            dep_depth: 3,
            with_dependents: false,
        };

        run(cli).unwrap();

        let output_content = fs::read_to_string(&output_file).unwrap();
        assert!(output_content.contains("small.rs"));
        assert!(!output_content.contains("large.rs"));
    }

    #[test]
    fn test_tree_only_shows_included_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("included.rs"), "x".repeat(10)).unwrap();
        fs::write(dir.path().join("excluded.rs"), "x".repeat(1000)).unwrap();

        let output_file = dir.path().join("output.txt");

        let cli = Cli {
            path: dir.path().to_path_buf(),
            include: vec![],
            exclude: vec![],
            max_tokens: Some(10),
            priority: "smallest-first".to_string(),
            format: formatter::Format::Plain,
            output: Some(output_file.clone()),
            clipboard: false,
            no_gitignore: false,
            tree: true,
            stdin_list: false,
            show_excluded: false,
            entry_point: None,
            dep_depth: 3,
            with_dependents: false,
        };

        run(cli).unwrap();

        let output_content = fs::read_to_string(&output_file).unwrap();
        let tree_section = output_content.split("===").next().unwrap();
        assert!(tree_section.contains("included.rs"));
        assert!(!tree_section.contains("excluded.rs"));
    }
}
