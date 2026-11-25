use crate::cli::Cli;
use anyhow::{Context, Result};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::overrides::OverrideBuilder;
use ignore::{WalkBuilder, WalkParallel, WalkState};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

const MAX_FILE_SIZE: u64 = 500 * 1024; // 500KB
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

const DEFAULT_EXCLUDES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "poetry.lock",
    "uv.lock",
    "Gemfile.lock",
    "composer.lock",
    "*.min.js",
    "*.min.css",
    "*.map",
];

/// Normalizes user glob patterns so `./target`, `.\target\`, and `.\src\*.rs`
/// work consistently across Windows PowerShell and Unix shells.
pub fn normalize_glob_pattern(pattern: &str) -> String {
    let mut normalized = pattern.trim().replace('\\', "/");
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped.to_string();
    }
    normalized
}

#[derive(Debug, Clone)]
pub struct WalkOptions {
    pub path: PathBuf,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub respect_gitignore: bool,
    pub redact_secrets: bool,
    pub default_excludes: bool,
}

impl WalkOptions {
    pub fn from_cli(cli: &Cli) -> Self {
        Self {
            path: cli.path.clone(),
            include: cli.include.clone(),
            exclude: cli.exclude.clone(),
            respect_gitignore: !cli.no_gitignore,
            redact_secrets: !cli.no_redact,
            default_excludes: !cli.no_default_excludes,
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
    let walker = build_parallel_walker(opts)?;

    let files: Mutex<Vec<FileEntry>> = Mutex::new(Vec::new());
    let first_err: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let redact = opts.redact_secrets;

    walker.run(|| {
        let files = &files;
        let first_err = &first_err;
        let include = &include;
        let root = &opts.path;

        Box::new(move |result| {
            let entry = match result.context("Failed to read directory entry") {
                Ok(e) => e,
                Err(err) => {
                    let mut guard = first_err.lock().unwrap();
                    if guard.is_none() {
                        *guard = Some(err);
                    }
                    return WalkState::Quit;
                }
            };

            // Fast skip directories using DirEntry file_type without extra stat syscall
            if let Some(ft) = entry.file_type() {
                if !ft.is_file() {
                    return WalkState::Continue;
                }
            }

            match read_file_entry(entry.path(), root, include, redact) {
                Ok(Some(file_entry)) => {
                    files.lock().unwrap().push(file_entry);
                    WalkState::Continue
                }
                Ok(None) => WalkState::Continue,
                Err(err) => {
                    let mut guard = first_err.lock().unwrap();
                    if guard.is_none() {
                        *guard = Some(err);
                    }
                    WalkState::Quit
                }
            }
        })
    });

    if let Some(err) = first_err.into_inner().unwrap() {
        return Err(err);
    }

    let mut collected = files.into_inner().unwrap();
    // Ensure deterministic ordering across threads and filesystems
    collected.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(collected)
}

pub fn parse_stdin_paths(input: &str, root: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for raw in input.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let candidate = Path::new(trimmed);
        if candidate.is_absolute() && !candidate.starts_with(root) {
            return Err(anyhow::anyhow!(
                "Security error: stdin path '{}' is outside scan root '{}'",
                trimmed,
                root.display()
            ));
        }

        let raw_path = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            root.join(candidate)
        };

        let root_depth = root
            .components()
            .filter(|c| matches!(c, Component::Normal(_)))
            .count();

        let mut normalized = PathBuf::new();
        let mut normal_depth = 0usize;

        for component in raw_path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    if normal_depth <= root_depth {
                        return Err(anyhow::anyhow!(
                            "Security error: path traversal '{}' escapes scan root '{}'",
                            trimmed,
                            root.display()
                        ));
                    }
                    normalized.pop();
                    normal_depth -= 1;
                }
                Component::Normal(part) => {
                    normalized.push(part);
                    normal_depth += 1;
                }
                other => normalized.push(other.as_os_str()),
            }
        }

        let key = normalized.to_string_lossy().replace('\\', "/");
        if seen.insert(key) {
            entries.push(normalized);
        }
    }

    Ok(entries)
}

pub fn collect_files_from_paths(
    paths: &[PathBuf],
    root: &Path,
    opts: &WalkOptions,
) -> Result<Vec<FileEntry>> {
    let include = IncludeFilter::new(root, &[])?;
    let mut files = Vec::with_capacity(paths.len());

    for path in paths {
        if !path.is_file() {
            return Err(anyhow::anyhow!(
                "File from stdin list was not found: {}",
                path.display()
            ));
        }

        match read_file_entry(path, root, &include, opts.redact_secrets) {
            Ok(Some(file_entry)) => files.push(file_entry),
            Ok(None) => continue,
            Err(err) => return Err(err),
        }
    }

    Ok(files)
}

pub fn collect_files_from_stdin(opts: &WalkOptions) -> Result<Vec<FileEntry>> {
    let stdin = std::io::read_to_string(std::io::stdin())?;
    let paths = parse_stdin_paths(&stdin, &opts.path)?;
    collect_files_from_paths(&paths, &opts.path, opts)
}

fn read_file_entry(
    path: &Path,
    root: &Path,
    include: &IncludeFilter,
    redact: bool,
) -> Result<Option<FileEntry>> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    // Filter by include glob BEFORE making filesystem metadata syscalls
    if !include.matches(relative) {
        return Ok(None);
    }

    let meta = match path.metadata() {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    if !meta.is_file() {
        return Ok(None);
    }

    let len = meta.len();
    if len > MAX_FILE_SIZE {
        eprintln!("Skipping large file: {}", path.display());
        return Ok(None);
    }

    let mut file =
        File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let cap = len as usize;
    let mut bytes = Vec::with_capacity(cap);

    // If the file is larger than 8KB, read only the first 8KB to sniff for binary NUL bytes
    // before reading the remaining up to 492KB from disk.
    if cap > BINARY_SNIFF_BYTES {
        let mut sniff_buf = [0u8; BINARY_SNIFF_BYTES];
        let n = file
            .read(&mut sniff_buf)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if sniff_buf[..n].contains(&0) {
            return Ok(None);
        }
        bytes.extend_from_slice(&sniff_buf[..n]);
    }

    file.read_to_end(&mut bytes)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    if cap <= BINARY_SNIFF_BYTES && is_binary(&bytes) {
        return Ok(None);
    }

    let mut content = String::from_utf8(bytes)
        .with_context(|| format!("{} is not valid UTF-8", path.display()))?;

    if redact {
        content = redact_secrets(&content);
    }

    Ok(Some(FileEntry {
        path: relative.to_string_lossy().replace('\\', "/"),
        content,
    }))
}

fn validate_glob_pattern(pattern: &str, label: &str) -> Result<()> {
    let open = pattern.bytes().filter(|&b| b == b'[').count();
    let close = pattern.bytes().filter(|&b| b == b']').count();
    if open != close {
        return Err(anyhow::anyhow!("Invalid {label} pattern: {pattern:?}"));
    }
    Ok(())
}

fn build_parallel_walker(opts: &WalkOptions) -> Result<WalkParallel> {
    let mut overrides = OverrideBuilder::new(&opts.path);

    if opts.default_excludes {
        for &default_pat in DEFAULT_EXCLUDES {
            overrides.add(&format!("!{default_pat}"))?;
        }
    }

    for raw_pattern in &opts.exclude {
        let pattern = normalize_glob_pattern(raw_pattern);
        if pattern.is_empty() {
            continue;
        }
        validate_glob_pattern(&pattern, "exclude")?;
        overrides
            .add(&format!("!{pattern}"))
            .with_context(|| format!("Invalid exclude pattern: {raw_pattern:?}"))?;
    }
    let overrides = overrides
        .build()
        .context("Failed to compile exclude patterns")?;

    let mut builder = WalkBuilder::new(&opts.path);
    builder
        .overrides(overrides)
        .add_custom_ignore_filename(".cratectxignore")
        .git_ignore(opts.respect_gitignore)
        .git_global(opts.respect_gitignore)
        .git_exclude(opts.respect_gitignore)
        .require_git(false);

    Ok(builder.build_parallel())
}

struct IncludeFilter(Option<Gitignore>);

impl IncludeFilter {
    fn new(root: &Path, patterns: &[String]) -> Result<Self> {
        if patterns.is_empty() {
            return Ok(Self(None));
        }
        let mut builder = GitignoreBuilder::new(root);
        for raw_pattern in patterns {
            let pattern = normalize_glob_pattern(raw_pattern);
            if pattern.is_empty() {
                continue;
            }
            validate_glob_pattern(&pattern, "include")?;
            builder
                .add_line(None, &pattern)
                .with_context(|| format!("Invalid include pattern: {raw_pattern:?}"))?;
        }
        let matcher = builder
            .build()
            .context("Failed to compile include patterns")?;
        Ok(Self(Some(matcher)))
    }

    fn matches(&self, relative: &Path) -> bool {
        match &self.0 {
            None => true,
            Some(gi) => gi.matched_path_or_any_parents(relative, false).is_ignore(),
        }
    }
}

/// Heuristic: a NUL byte in the first 8KB means binary.
#[inline]
fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_SNIFF_BYTES)].contains(&0)
}

/// Redacts PEM private keys and high-confidence API tokens (AWS, GitHub, OpenAI/Anthropic, Slack).
pub fn redact_secrets(input: &str) -> String {
    let mut text = input.to_string();

    // 1. Redact PEM private key blocks
    while let Some(start) = text.find("-----BEGIN ") {
        let after_begin = start + 11;
        let header_end = text[after_begin..]
            .find("-----")
            .map(|i| after_begin + i + 5);
        if let Some(h_end) = header_end {
            if text[start..h_end].contains("PRIVATE KEY") {
                if let Some(end_marker) = text[h_end..].find("-----END ") {
                    let tail_start = h_end + end_marker;
                    if let Some(tail_close) = text[tail_start + 9..].find("-----") {
                        let full_end = tail_start + 9 + tail_close + 5;
                        text.replace_range(start..full_end, "[REDACTED_PRIVATE_KEY]");
                        continue;
                    }
                }
            }
        }
        break;
    }

    // 2. Redact known token prefixes (AKIA/ASIA, ghp_/github_pat_, sk-, xoxb-/xoxp-)
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut last = 0;

    while i < bytes.len() {
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let rem = &text[i..];
        let prev_ok = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');

        if prev_ok {
            let token_match = if (rem.starts_with("AKIA") || rem.starts_with("ASIA"))
                && rem.len() >= 20
                && rem.as_bytes()[4..20]
                    .iter()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            {
                Some((20, "[REDACTED_AWS_KEY]"))
            } else if rem.starts_with("ghp_") || rem.starts_with("gho_") || rem.starts_with("ghs_")
            {
                let len = 4 + rem[4..]
                    .bytes()
                    .take_while(|b| b.is_ascii_alphanumeric())
                    .count();
                (len >= 36).then_some((len, "[REDACTED_GITHUB_TOKEN]"))
            } else if rem.starts_with("github_pat_") {
                let len = 11
                    + rem[11..]
                        .bytes()
                        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                        .count();
                (len >= 30).then_some((len, "[REDACTED_GITHUB_TOKEN]"))
            } else if rem.starts_with("sk-") {
                let len = 3 + rem[3..]
                    .bytes()
                    .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
                    .count();
                (len >= 24).then_some((len, "[REDACTED_API_KEY]"))
            } else {
                None
            };

            if let Some((tok_len, replacement)) = token_match {
                out.push_str(&text[last..i]);
                out.push_str(replacement);
                i += tok_len;
                last = i;
                continue;
            }
        }
        i += 1;
    }

    if last == 0 {
        text
    } else {
        out.push_str(&text[last..]);
        out
    }
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
            redact_secrets: true,
            default_excludes: true,
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

    #[test]
    fn skips_large_binary_file_via_early_sniff() {
        let dir = TempDir::new().unwrap();
        write(&dir, "keep.rs", "fn main() {}");
        let mut large_bin = vec![b'a'; 32 * 1024];
        large_bin[100] = 0; // NUL byte in first 8KB
        write(&dir, "asset.bin", large_bin);

        let files = collect_files(&opts(&dir)).unwrap();
        assert_eq!(paths(&files), ["keep.rs"]);
    }

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
            [
                "README.md",
                "docs/guide.md",
                "src/main.rs",
                "src/util/helper.rs"
            ]
        );
    }

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

    #[test]
    fn parse_stdin_paths_handles_relative_and_comment_entries() {
        let root = Path::new(".");
        let input = "\n# comment\nsrc/main.rs\n./README.md\n\n";
        let paths = parse_stdin_paths(input, root).unwrap();
        assert_eq!(
            paths,
            [PathBuf::from("src/main.rs"), PathBuf::from("README.md")]
        );
    }

    #[test]
    fn collect_files_from_stdin_reads_selected_files() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/main.rs", "fn main() {}\n");
        write(&dir, "src/lib.rs", "pub fn lib() {}\n");
        write(&dir, "README.md", "# doc\n");

        let root = dir.path();
        let selected = parse_stdin_paths("src/main.rs\nsrc/lib.rs\nREADME.md\n", root).unwrap();
        let files = collect_files_from_paths(&selected, root, &opts(&dir)).unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.iter().any(|f| f.path == "src/main.rs"));
        assert!(files.iter().any(|f| f.path == "src/lib.rs"));
        assert!(files.iter().any(|f| f.path == "README.md"));
    }

    #[test]
    fn stdin_paths_reject_parent_directory_traversal() {
        let root = Path::new("src");
        let err = parse_stdin_paths("../secrets.env\n", root)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Security error"), "{err}");
    }

    #[test]
    fn redacts_aws_github_openai_and_private_keys() {
        let sample = "aws = \"AKIAIOSFODNN7EXAMPLE\"\ngh = \"ghp_1234567890abcdefghijklmnopqrstuvwxyz\"\noai = \"sk-proj-1234567890abcdefghijklmnop\"\n-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA\n-----END RSA PRIVATE KEY-----";
        let clean = redact_secrets(sample);
        assert!(clean.contains("[REDACTED_AWS_KEY]"));
        assert!(clean.contains("[REDACTED_GITHUB_TOKEN]"));
        assert!(clean.contains("[REDACTED_API_KEY]"));
        assert!(clean.contains("[REDACTED_PRIVATE_KEY]"));
        assert!(!clean.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!clean.contains("MIIEpAIBAAKCAQEA"));
    }

    #[test]
    fn exclude_handles_dot_slash_and_windows_backslashes() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/main.rs", "fn main() {}");
        write(&dir, "target/debug/app.rs", "// build artifact");
        write(&dir, "docs/readme.md", "# docs");
        write(&dir, "config.yaml", "key: val");

        let mut o = opts(&dir);
        o.exclude = strings(&["./target", ".\\docs\\", "*.yaml"]);

        let files = collect_files(&o).unwrap();
        assert_eq!(paths(&files), ["src/main.rs"]);
    }

    #[test]
    fn default_excludes_skip_cargo_lock_and_respect_cratectxignore() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/lib.rs", "pub fn a() {}");
        write(&dir, "Cargo.lock", "# lockfile");
        write(&dir, "bundle.min.js", "function(){}");
        write(&dir, "scratch.txt", "ignore me");
        write(&dir, ".cratectxignore", "scratch.txt\n");

        let files = collect_files(&opts(&dir)).unwrap();
        assert_eq!(paths(&files), ["src/lib.rs"]);
    }

    #[test]
    fn redact_handles_multibyte_chars() {
        let input = "├── src/\n│   └── lib.rs\n你好 AKIAIOSFODNN7EXAMPLE é";
        let out = redact_secrets(input);
        assert!(out.contains("└── lib.rs"));
        assert!(out.contains("[REDACTED_AWS_KEY]"));
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
    }
}
