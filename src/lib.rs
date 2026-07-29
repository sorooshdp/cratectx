pub mod cli;
pub mod walker;
pub mod formatter;
pub mod token;
pub mod tree;

use anyhow::Result;
use cli::Cli;
use walker::WalkOptions;

/// Main entry point for the library
pub fn run(cli: Cli) -> Result<()> {
    let walk_opts = WalkOptions::from_cli(&cli);
    let files = walker::collect_files(&walk_opts)?;
    
    let mut total_tokens = 0;
    let mut selected_files = Vec::new();
    
    for file in files {
        let tokens = token::estimate_tokens(&file.content);
        if let Some(max) = cli.max_tokens {
            if total_tokens + tokens > max {
                eprintln!("Skipping {} (would exceed token budget)", file.path);
                continue;
            }
        }
        total_tokens += tokens;
        selected_files.push((file, tokens));
    }
    
    let mut output = String::new();
    
    // Add tree if requested
    if cli.tree {
        let file_paths: Vec<String> = selected_files
            .iter()
            .map(|(f, _)| f.path.clone())
            .collect();
        output.push_str(&tree::generate_tree(&file_paths));
    }
    
    // Add formatted file contents
    output.push_str(&formatter::format_files(&selected_files, cli.format)?);
    
    if cli.clipboard {
        #[cfg(not(test))]
        {
            use arboard::Clipboard;
            let mut clipboard = Clipboard::new()?;
            clipboard.set_text(&output)?;
            eprintln!("Copied to clipboard ({} tokens)", total_tokens);
        }
    }
    
    if let Some(path) = cli.output {
        std::fs::write(&path, &output)?;
        eprintln!("Written to {} ({} tokens)", path.display(), total_tokens);
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
            format: formatter::Format::Xml,
            output: None,
            clipboard: false,
            no_gitignore: false,
            tree: false,
            stdin_list: false,
        };
        
        let result = run(cli);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_with_tree_output() {
        let dir = TempDir::new().unwrap();
        // Create a nested structure
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
            format: formatter::Format::Xml,
            output: Some(output_file.clone()),
            clipboard: false,
            no_gitignore: false,
            tree: true,
            stdin_list: false,
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
    fn test_tree_with_max_tokens() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("small.rs"), "fn a() {}").unwrap();
        fs::write(dir.path().join("large.rs"), "fn b() {}\n".repeat(1000)).unwrap();
        
        let output_file = dir.path().join("output.txt");
        
        let cli = Cli {
            path: dir.path().to_path_buf(),
            include: vec![],
            exclude: vec![],
            max_tokens: Some(20),
            format: formatter::Format::Plain,
            output: Some(output_file.clone()),
            clipboard: false,
            no_gitignore: false,
            tree: true,
            stdin_list: false,
        };
        
        run(cli).unwrap();
        
        let output_content = fs::read_to_string(&output_file).unwrap();
        assert!(output_content.contains("Directory structure:"));
        assert!(output_content.contains("small.rs"));
        // large.rs should be in tree but not in content due to token limit
        assert!(output_content.contains("small.rs"));
    }

    #[test]
    fn test_tree_markdown_format() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("file1.rs"), "code1").unwrap();
        fs::write(dir.path().join("file2.rs"), "code2").unwrap();
        
        let output_file = dir.path().join("output.md");
        
        let cli = Cli {
            path: dir.path().to_path_buf(),
            include: vec![],
            exclude: vec![],
            max_tokens: None,
            format: formatter::Format::Md,
            output: Some(output_file.clone()),
            clipboard: false,
            no_gitignore: false,
            tree: true,
            stdin_list: false,
        };
        
        run(cli).unwrap();
        
        let output_content = fs::read_to_string(&output_file).unwrap();
        assert!(output_content.contains("Directory structure:"));
        assert!(output_content.contains("##"));
        assert!(output_content.contains("```"));
    }
}
