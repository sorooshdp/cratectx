use anyhow::{Context, Result};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::overrides::OverrideBuilder;
use ignore::{Walk, WalkBuilder};
use std::path::{Path, PathBuf};
use crate::cli::Cli;

const MAX_FILE_SIZE: u64 = 500 * 1024; // 500KB
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

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
    let include = IncludeFilter::new(&opts.path, &opts.include)?;
    let walker = build_walker(opts)?;

    let mut files = Vec::new();
    for result in walker {
        let entry = result.context("Failed to read directory entry")?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let relative = path.strip_prefix(&opts.path).unwrap_or(path);
        if !include.matches(relative) {
            continue;
        }

        if entry.metadata()?.len() > MAX_FILE_SIZE {
            eprintln!("Skipping large file: {}", path.display());
            continue;
        }

        let bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if is_binary(&bytes) {
            continue;
        }
        let content = String::from_utf8(bytes)
            .with_context(|| format!("{} is not valid UTF-8", path.display()))?;

        files.push(FileEntry {
            path: relative.to_string_lossy().into_owned(),
            content,
        });
    }

    Ok(files)
}

/// Excludes are installed as override rules so the walker prunes matching
/// directories during traversal instead of visiting (and reading) their contents.
/// In override syntax a leading `!` means "ignore".
fn validate_glob_pattern(pattern: &str, label: &str) -> Result<()> {
    let open = pattern.matches('[').count();
    let close = pattern.matches(']').count();
    if open != close {
        return Err(anyhow::anyhow!("Invalid {label} pattern: {pattern:?}"));
    }
    Ok(())
}

fn build_walker(opts: &WalkOptions) -> Result<Walk> {
    let mut overrides = OverrideBuilder::new(&opts.path);
    for pattern in &opts.exclude {
        validate_glob_pattern(pattern, "exclude")?;
        overrides
            .add(&format!("!{pattern}"))
            .with_context(|| format!("Invalid exclude pattern: {pattern:?}"))?;
    }
    let overrides = overrides.build().context("Failed to compile exclude patterns")?;

    let mut builder = WalkBuilder::new(&opts.path);
    builder
        .overrides(overrides)
        .git_ignore(opts.respect_gitignore)
        .git_global(opts.respect_gitignore)
        .git_exclude(opts.respect_gitignore)
        .require_git(false);

    Ok(builder.build())
}

/// Include patterns are applied as a post-filter on files, not as override
/// whitelists. A whitelist override takes precedence over `.gitignore` and
/// would resurrect ignored files whenever `--include` is used; that is not
/// what `--include "*.rs"` should mean. Since an include pattern can never
/// prune a directory, filtering after the walk costs nothing extra.
struct IncludeFilter(Option<Gitignore>);

impl IncludeFilter {
    fn new(root: &Path, patterns: &[String]) -> Result<Self> {
        if patterns.is_empty() {
            return Ok(Self(None));
        }
        let mut builder = GitignoreBuilder::new(root);
        for pattern in patterns {
            validate_glob_pattern(pattern, "include")?;
            builder
                .add_line(None, pattern)
                .with_context(|| format!("Invalid include pattern: {pattern:?}"))?;
        }
        let matcher = builder.build().context("Failed to compile include patterns")?;
        Ok(Self(Some(matcher)))
    }

    /// `relative` must be relative to the scan root. A file matches if it or
    /// any parent directory matches a pattern, so `--include src` selects
    /// everything under `src/`.
    fn matches(&self, relative: &Path) -> bool {
        match &self.0 {
            None => true,
            Some(gi) => gi.matched_path_or_any_parents(relative, false).is_ignore(),
        }
    }
}

/// Heuristic: a NUL byte in the first 8KB means binary.
fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_SNIFF_BYTES)].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn opts(dir: &TempDir) -> WalkOptions {
        WalkOptions {
            path: dir.path().to_path_buf(),
            include: vec![],
            exclude: vec![],
            respect_gitignore: true,
        }
    }

    fn write(dir: &TempDir, rel: &str, content: impl AsRef<[u8]>) {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn paths(files: &[FileEntry]) -> Vec<String> {
        let mut v: Vec<_> = files.iter().map(|f| f.path.replace('\\', "/")).collect();
        v.sort();
        v
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn collects_text_files_and_skips_binary() {
        let dir = TempDir::new().unwrap();
        write(&dir, "a.rs", "fn a() {}");
        write(&dir, "b.txt", "hello");
        write(&dir, "blob.bin", [0u8, 1, 2, 0, 255]);

        let files = collect_files(&opts(&dir)).unwrap();
        assert_eq!(paths(&files), ["a.rs", "b.txt"]);
    }

    /// Regression: a bare directory name never matched the old per-file matcher,
    /// so `--exclude target` still included everything under `target/`.
    #[test]
    fn exclude_prunes_directories_and_files() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/main.rs", "fn main() {}");
        write(&dir, "target/debug/build.rs", "// generated");
        write(&dir, "notes.txt", "todo");

        let mut o = opts(&dir);
        o.exclude = strings(&["target", "*.txt"]);

        let files = collect_files(&o).unwrap();
        assert_eq!(paths(&files), ["src/main.rs"]);
    }

    #[test]
    fn include_matches_files_and_whole_directories() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/main.rs", "fn main() {}");
        write(&dir, "src/util/helper.rs", "pub fn h() {}");
        write(&dir, "docs/guide.md", "# guide");
        write(&dir, "README.md", "# readme");
        write(&dir, "build.py", "print()");

        let mut o = opts(&dir);
        o.include = strings(&["src", "*.md"]);

        let files = collect_files(&o).unwrap();
        assert_eq!(
            paths(&files),
            ["README.md", "docs/guide.md", "src/main.rs", "src/util/helper.rs"]
        );
    }

    /// Documents the design choice: includes narrow the selection but do not
    /// override `.gitignore`. Also covers `require_git(false)` (no repo here)
    /// and the `respect_gitignore` switch.
    #[test]
    fn include_does_not_override_gitignore() {
        let dir = TempDir::new().unwrap();
        write(&dir, ".gitignore", "generated.rs\n");
        write(&dir, "main.rs", "fn main() {}");
        write(&dir, "generated.rs", "// generated");

        let mut o = opts(&dir);
        o.include = strings(&["*.rs"]);

        let files = collect_files(&o).unwrap();
        assert_eq!(paths(&files), ["main.rs"]);

        o.respect_gitignore = false;
        let files = collect_files(&o).unwrap();
        assert_eq!(paths(&files), ["generated.rs", "main.rs"]);
    }

    #[test]
    fn invalid_glob_is_an_error() {
        let dir = TempDir::new().unwrap();
        write(&dir, "a.rs", "");

        let mut o = opts(&dir);
        o.exclude = strings(&["[unclosed"]);
        let err = collect_files(&o).unwrap_err().to_string();
        assert!(err.contains("Invalid exclude pattern"), "{err}");

        let mut o = opts(&dir);
        o.include = strings(&["[unclosed"]);
        let err = collect_files(&o).unwrap_err().to_string();
        assert!(err.contains("Invalid include pattern"), "{err}");
    }
}