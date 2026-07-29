pub mod cli;
pub mod walker;
pub mod formatter;
pub mod token;
pub mod tree;
pub mod budget;

use anyhow::Result;
use cli::Cli;
use walker::WalkOptions;
use budget::PriorityStrategy;

/// Main entry point for the library
pub fn run(cli: Cli) -> Result<()> {
    let strategy = PriorityStrategy::from_str(&cli.priority)
        .ok_or_else(|| anyhow::anyhow!(
            "Invalid priority strategy: '{}'. Valid options: first-come, smallest-first, shallow-first, deep-first",
            cli.priority
        ))?;

    let walk_opts = WalkOptions::from_cli(&cli);
    let files = walker::collect_files(&walk_opts)?;
    
    let budget_result = budget::apply_budget(
        files,
        cli.max_tokens,
        strategy,
        token::estimate_tokens,
    );

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
    
    // Add tree if requested (only show included files)
    if cli.tree {
        let file_paths: Vec<String> = budget_result
            .included
            .iter()
            .map(|(f, _)| f.path.clone())
            .collect();
        output.push_str(&tree::generate_tree(&file_paths));
    }
    
    // Add formatted file contents
    output.push_str(&formatter::format_files(&budget_result.included, cli.format)?);
    
    if cli.clipboard {
        #[cfg(not(test))]
        {
            use arboard::Clipboard;
            let mut clipboard = Clipboard::new()?;
            clipboard.set_text(&output)?;
            eprintln!("Copied to clipboard ({} tokens)", budget_result.total_tokens);
        }
    }
    
    if let Some(path) = cli.output {
        std::fs::write(&path, &output)?;
        eprintln!("Written to {} ({} tokens)", path.display(), budget_result.total_tokens);
    } else if !cli.clipboard {
        print!("{}", output);
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

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
        };
        
        let result = run(cli);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid priority strategy"));
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
        };
        
        run(cli).unwrap();
        
        let output_content = fs::read_to_string(&output_file).unwrap();
        let tree_section = output_content.split("===").next().unwrap();
        assert!(tree_section.contains("included.rs"));
        assert!(!tree_section.contains("excluded.rs"));
    }
}
