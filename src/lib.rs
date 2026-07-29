pub mod cli;
pub mod walker;
pub mod formatter;
pub mod token;

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
    
    let output = formatter::format_files(&selected_files, cli.format)?;
    
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
}
