use crate::formatter::Format;
use clap::Parser;
use std::path::PathBuf;

const AFTER_HELP: &str = "\
Examples:
  # Pack current repo into XML within a 5,000-token budget
  cratectx . --format xml --max-tokens 5000

  # Pack only Rust & TOML files with a directory tree, stripping tests
  cratectx src -i *.rs,*.toml --tree --strip-tests --format md

  # Traverse 2 hops of dependencies starting from src/lib.rs
  cratectx . --entry-point src/lib.rs --dep-depth 2 --with-dependents

  # Collapse function bodies to signatures and copy to clipboard
  cratectx src --signatures-only --clipboard";

#[derive(Parser, Debug, Clone)]
#[command(
    name = "cratectx",
    version = crate::version::app_version(),
    about = "Compress codebases into LLM-ready context with token-aware packing",
    long_about = "Scans a codebase and packages source files into compact, LLM-friendly context.\n\
                  Supports token budgets, AST-aware Rust dependency traversal, signature collapsing,\n\
                  test stripping, and automatic secret redaction.",
    after_help = AFTER_HELP
)]
pub struct Cli {
    /// Directory to scan (defaults to current directory)
    #[arg(default_value = ".", value_name = "PATH")]
    pub path: PathBuf,

    /// Include files/dirs matching glob pattern(s) (e.g. -i *.rs *.toml or -i *.rs,*.toml)
    #[arg(short = 'i', long, num_args = 1.., value_delimiter = ',', value_name = "GLOB")]
    pub include: Vec<String>,

    /// Exclude files/dirs matching glob pattern(s) (e.g. -e target *.md or -e target,*.md)
    #[arg(short = 'e', long, num_args = 1.., value_delimiter = ',', value_name = "GLOB")]
    pub exclude: Vec<String>,

    /// Maximum estimated token budget for output
    #[arg(short = 'm', long, value_name = "TOKENS")]
    pub max_tokens: Option<usize>,

    /// Priority strategy when token budget is exceeded [possible values: smallest-first, shallow-first, deep-first, first-come]
    #[arg(short = 'p', long, default_value = "smallest-first", value_name = "STRATEGY")]
    pub priority: String,

    /// Output format
    #[arg(short = 'f', long, default_value = "xml", value_name = "FORMAT")]
    pub format: Format,

    /// Write context to a file instead of stdout
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Copy generated context to the system clipboard
    #[arg(short = 'c', long)]
    pub clipboard: bool,

    /// Disable automatic respect for .gitignore, global gitignore, and .git/info/exclude
    #[arg(long)]
    pub no_gitignore: bool,

    /// Prepend an ASCII directory tree of included files
    #[arg(short = 't', long)]
    pub tree: bool,

    /// Read newline-delimited file paths from stdin instead of walking PATH
    #[arg(long)]
    pub stdin_list: bool,

    /// Print paths of files excluded due to the token budget to stderr
    #[arg(long)]
    pub show_excluded: bool,

    /// Anchor selection to an entry-point Rust file and its dependency graph
    #[arg(long, value_name = "FILE")]
    pub entry_point: Option<PathBuf>,

    /// Maximum hop depth for dependency traversal from --entry-point
    #[arg(long, default_value = "3", value_name = "DEPTH")]
    pub dep_depth: usize,

    /// Include reverse dependencies (files that import the entry point)
    #[arg(long)]
    pub with_dependents: bool,

    /// Strip #[cfg(test)] modules and #[test] functions from Rust files
    #[arg(long)]
    pub strip_tests: bool,

    /// Collapse Rust function bodies to `{ /* ... */ }`, keeping only signatures and types
    #[arg(long)]
    pub signatures_only: bool,

    /// Disable automatic redaction of API keys, tokens, and PEM private keys
    #[arg(long)]
    pub no_redact: bool,

    /// Include lockfiles and minified assets that are excluded by default
    #[arg(long)]
    pub no_default_excludes: bool,
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
            "--include",
            "*.rs",
            "--exclude",
            "tests",
            "--max-tokens",
            "1000",
            "--priority",
            "shallow-first",
            "--clipboard",
        ])
        .unwrap();
        assert_eq!(cli.path, PathBuf::from("src"));
        assert_eq!(cli.include, vec!["*.rs"]);
        assert_eq!(cli.exclude, vec!["tests"]);
        assert_eq!(cli.max_tokens, Some(1000));
        assert_eq!(cli.priority, "shallow-first");
        assert!(cli.clipboard);
    }

    #[test]
    fn test_show_excluded_flag() {
        let cli = Cli::try_parse_from(&["cratectx", "--show-excluded"]).unwrap();
        assert!(cli.show_excluded);
    }

    #[test]
    fn test_dependency_options() {
        let cli = Cli::try_parse_from(&[
            "cratectx",
            "--entry-point",
            "src/main.rs",
            "--dep-depth",
            "5",
            "--with-dependents",
        ])
        .unwrap();
        assert_eq!(cli.entry_point, Some(PathBuf::from("src/main.rs")));
        assert_eq!(cli.dep_depth, 5);
        assert!(cli.with_dependents);
    }

    #[test]
    fn test_stdin_list_flag() {
        let cli = Cli::try_parse_from(&["cratectx", "--stdin-list"]).unwrap();
        assert!(cli.stdin_list);
    }

    #[test]
    fn test_security_and_transform_flags() {
        let cli = Cli::try_parse_from(&[
            "cratectx",
            "--strip-tests",
            "--signatures-only",
            "--no-redact",
        ])
        .unwrap();
        assert!(cli.strip_tests);
        assert!(cli.signatures_only);
        assert!(cli.no_redact);
    }

    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }
    #[test]
    fn test_multi_value_and_comma_excludes() {
        let cli = Cli::try_parse_from(&[
            "cratectx",
            "src",
            "-e",
            "./target",
            "*.yaml",
            "*.md,*.exe",
            "-i",
            "*.rs",
            "*.toml",
        ])
        .unwrap();
        assert_eq!(cli.path, PathBuf::from("src"));
        assert_eq!(cli.exclude, vec!["./target", "*.yaml", "*.md", "*.exe"]);
        assert_eq!(cli.include, vec!["*.rs", "*.toml"]);
    }
}
