use clap::Parser;
use std::path::PathBuf;
use crate::formatter::Format;

#[derive(Parser, Debug, Clone)]
#[command(name = "cratectx")]
#[command(about = "Compress codebases into LLM-ready context with token-aware packing", long_about = None)]
pub struct Cli {
    /// Directory to scan (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Include pattern (glob, repeatable)
    #[arg(short = 'i', long)]
    pub include: Vec<String>,

    /// Exclude pattern (glob, repeatable)
    #[arg(short = 'e', long)]
    pub exclude: Vec<String>,

    /// Maximum token budget
    #[arg(short = 'm', long)]
    pub max_tokens: Option<usize>,

    /// Priority strategy when budget is exceeded
    #[arg(short = 'p', long, default_value = "smallest-first")]
    pub priority: String,

    /// Output format
    #[arg(short = 'f', long, default_value = "xml")]
    pub format: Format,

    /// Output file path (defaults to stdout)
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,

    /// Copy to clipboard
    #[arg(short = 'c', long)]
    pub clipboard: bool,

    /// Disable automatic .gitignore respect
    #[arg(long)]
    pub no_gitignore: bool,

    /// Prepend directory tree summary
    #[arg(short = 't', long)]
    pub tree: bool,

    /// Read file paths from stdin instead of walking
    #[arg(long)]
    pub stdin_list: bool,

    /// Show summary of excluded files
    #[arg(long)]
    pub show_excluded: bool,

    /// Entry point file for dependency traversal
    #[arg(long)]
    pub entry_point: Option<PathBuf>,

    /// Maximum depth for dependency traversal
    #[arg(long, default_value = "3")]
    pub dep_depth: usize,

    /// Include reverse dependencies (files that depend on the entry point)
    #[arg(long)]
    pub with_dependents: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_defaults() {
        let cli = Cli::try_parse_from(&["cratectx"]).unwrap();
        assert_eq!(cli.path, PathBuf::from("."));
        assert_eq!(cli.include.len(), 0);
        assert_eq!(cli.exclude.len(), 0);
        assert_eq!(cli.max_tokens, None);
        assert!(!cli.clipboard);
        assert!(!cli.no_gitignore);
        assert_eq!(cli.priority, "smallest-first");
        assert_eq!(cli.dep_depth, 3);
    }

    #[test]
    fn test_cli_with_options() {
        let cli = Cli::try_parse_from(&[
            "cratectx",
            "src",
            "--include", "*.rs",
            "--exclude", "tests",
            "--max-tokens", "1000",
            "--priority", "shallow-first",
            "--clipboard"
        ]).unwrap();
        assert_eq!(cli.path, PathBuf::from("src"));
        assert_eq!(cli.include, vec!["*.rs"]);
        assert_eq!(cli.exclude, vec!["tests"]);
        assert_eq!(cli.max_tokens, Some(1000));
        assert_eq!(cli.priority, "shallow-first");
        assert!(cli.clipboard);
    }

    #[test]
    fn test_show_excluded_flag() {
        let cli = Cli::try_parse_from(&[
            "cratectx",
            "--show-excluded"
        ]).unwrap();
        assert!(cli.show_excluded);
    }

    #[test]
    fn test_dependency_options() {
        let cli = Cli::try_parse_from(&[
            "cratectx",
            "--entry-point", "src/main.rs",
            "--dep-depth", "5",
            "--with-dependents"
        ]).unwrap();
        assert_eq!(cli.entry_point, Some(PathBuf::from("src/main.rs")));
        assert_eq!(cli.dep_depth, 5);
        assert!(cli.with_dependents);
    }

    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }
}
