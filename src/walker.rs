use anyhow::{Context, Result};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use crate::cli::Cli;

const MAX_FILE_SIZE: u64 = 500 * 1024; // 500KB

#[derive(Debug, Clone)]
pub struct WalkOptions {
    pub path: PathBuf,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub respect_gitignore: bool,
}

impl WalkOptions {
    pub fn from_cli(cli: &Cli) -> Self {
        Self {
            path: cli.path.clone(),
            include: cli.include.clone(),
            exclude: cli.exclude.clone(),
            respect_gitignore: !cli.no_gitignore,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub content: String,
}

pub fn collect_files(opts: &WalkOptions) -> Result<Vec<FileEntry>> {
    let mut files = Vec::new();
    let mut builder = WalkBuilder::new(&opts.path);
    builder.git_ignore(opts.respect_gitignore);
    
    for exclude in &opts.exclude {
        builder.add_custom_ignore_filename(exclude);
    }

    for result in builder.build() {
        let entry = result.context("Failed to read directory entry")?;
        let path = entry.path();
        
        if !path.is_file() {
            continue;
        }
        
        if is_binary(path)? {
            continue;
        }
        
        let metadata = path.metadata()?;
        if metadata.len() > MAX_FILE_SIZE {
            eprintln!("Skipping large file: {}", path.display());
            continue;
        }
        
        if !opts.include.is_empty() && !matches_patterns(path, &opts.include) {
            continue;
        }
        
        if matches_patterns(path, &opts.exclude) {
            continue;
        }
        
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        
        let relative_path = path.strip_prefix(&opts.path)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        
        files.push(FileEntry {
            path: relative_path,
            content,
        });
    }
    
    Ok(files)
}

fn is_binary(path: &Path) -> Result<bool> {
    let content = std::fs::read(path)?;
    // Simple heuristic: if first 8KB contains null bytes, it's likely binary
    let sample_size = content.len().min(8192);
    Ok(content[..sample_size].contains(&0))
}

fn matches_patterns(path: &Path, patterns: &[String]) -> bool {
    use ignore::gitignore::GitignoreBuilder;
    
    if patterns.is_empty() {
        return false;
    }
    
    let mut builder = GitignoreBuilder::new("");
    for pattern in patterns {
        builder.add_line(None, pattern).ok();
    }
    
    if let Ok(gi) = builder.build() {
        gi.matched(path, false).is_ignore()
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[test]
    fn test_collect_files_empty_dir() {
        let dir = TempDir::new().unwrap();
        let opts = WalkOptions {
            path: dir.path().to_path_buf(),
            include: vec![],
            exclude: vec![],
            respect_gitignore: true,
        };
        
        let files = collect_files(&opts).unwrap();
        assert_eq!(files.len(), 0);
    }

    #[test]
    fn test_collect_files_with_content() {
        let dir = TempDir::new().unwrap();
        let file1 = dir.path().join("test1.rs");
        let file2 = dir.path().join("test2.rs");
        fs::write(&file1, "fn foo() {}").unwrap();
        fs::write(&file2, "fn bar() {}").unwrap();
        
        let opts = WalkOptions {
            path: dir.path().to_path_buf(),
            include: vec![],
            exclude: vec![],
            respect_gitignore: true,
        };
        
        let files = collect_files(&opts).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_exclude_pattern() {
        let dir = TempDir::new().unwrap();
        let file1 = dir.path().join("test.rs");
        let file2 = dir.path().join("test.txt");
        fs::write(&file1, "fn foo() {}").unwrap();
        fs::write(&file2, "hello").unwrap();
        
        let opts = WalkOptions {
            path: dir.path().to_path_buf(),
            include: vec![],
            exclude: vec!["*.txt".to_string()],
            respect_gitignore: true,
        };
        
        let files = collect_files(&opts).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].path.ends_with(".rs"));
    }

    #[test]
    fn test_include_pattern() {
        let dir = TempDir::new().unwrap();
        let file1 = dir.path().join("test.rs");
        let file2 = dir.path().join("test.txt");
        fs::write(&file1, "fn foo() {}").unwrap();
        fs::write(&file2, "hello").unwrap();
        
        let opts = WalkOptions {
            path: dir.path().to_path_buf(),
            include: vec!["*.rs".to_string()],
            exclude: vec![],
            respect_gitignore: true,
        };
        
        let files = collect_files(&opts).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].path.ends_with(".rs"));
    }

    #[test]
    fn test_is_binary_detects_null_bytes() {
        let dir = TempDir::new().unwrap();
        let binary_file = dir.path().join("binary");
        fs::write(&binary_file, vec![0u8, 1, 2, 0, 255]).unwrap();
        
        assert!(is_binary(&binary_file).unwrap());
    }

    #[test]
    fn test_is_binary_rejects_text() {
        let dir = TempDir::new().unwrap();
        let text_file = dir.path().join("text.txt");
        fs::write(&text_file, "hello world").unwrap();
        
        assert!(!is_binary(&text_file).unwrap());
    }
}
