# cratectx

`cratectx` is a fast Rust CLI tool that scans a codebase and packages source files into compact, LLM-ready context.

## Features

- **Token-Aware Budgeting**: Enforces `--max-tokens` using prioritization strategies (`smallest-first`, `shallow-first`, `deep-first`, `first-come`).
- **Rust Dependency Traversal**: Anchor context around an `--entry-point` and automatically pull in its module dependencies (and optionally reverse dependents) up to `--dep-depth`.
- **Rust Source Compression**: Strip unit tests (`--strip-tests`) or collapse function bodies to `{ /* ... */ }` (`--signatures-only`) before token budgeting.
- **Safe by Default**:
  - Automatically redacts PEM private keys and common API tokens (AWS, GitHub, OpenAI/Anthropic) unless `--no-redact` is passed.
  - Escapes closing `</file>` and `</context>` tags in XML mode to prevent tag-breakout prompt injection while keeping raw code syntax unescaped.
  - Blocks `..` path traversal and out-of-root paths when reading from `--stdin-list`.
- **Smart Filtering**:
  - Respects `.gitignore`, global gitignore, and `.cratectxignore` by default.
  - Automatically skips binary files (via 8 KB NUL-byte sniffing), files over 500 KB, lockfiles (`Cargo.lock`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`, `poetry.lock`, `uv.lock`, `Gemfile.lock`, `composer.lock`), and minified/map assets (`*.min.js`, `*.min.css`, `*.map`).
- **Multiple Output Formats & Targets**: Emit `xml`, `md` (Markdown), or `plain` text to `stdout`, a file (`-o`), or the system clipboard (`-c`).

## Installation & Build

```bash
cargo build --release
```

## Usage Examples

Pack the current repository into XML within a 5,000-token budget:

```bash
cratectx . --format xml --max-tokens 5000
```

Scan `src/`, include only `.rs` and `.toml` files, prepend a directory tree, strip tests, and copy to clipboard:

```bash
cratectx src -i *.rs,*.toml -e target --tree --strip-tests --clipboard
```

Pack `src/lib.rs` along with its dependencies (up to 2 hops) and reverse dependents:

```bash
cratectx . --entry-point src/lib.rs --dep-depth 2 --with-dependents --format md
```

Produce a high-level architectural overview by collapsing function bodies to signatures only:

```bash
cratectx src --signatures-only --tree --format md -o architecture.md
```

Pipe a custom file list from git or fd via stdin:

```bash
git diff --name-only main | cratectx . --stdin-list --format xml
```

## CLI Options

| Option | Description | Default |
| --- | --- | --- |
| `[PATH]` | Directory to scan. | — |
| `-i, --include <GLOB>...` | Include files/dirs matching glob pattern(s) (space- or comma-separated). | All non-ignored |
| `-e, --exclude <GLOB>...` | Exclude files/dirs matching glob pattern(s) (space- or comma-separated). | None |
| `-m, --max-tokens <TOKENS>` | Maximum estimated token budget. | Unlimited |
| `-p, --priority <STRATEGY>` | Budget strategy: `smallest-first`, `shallow-first`, `deep-first`, `first-come`. | `smallest-first` |
| `-f, --format <FORMAT>` | Output format: `xml`, `md`, `plain`. | `xml` |
| `-o, --output <FILE>` | Write output to a file instead of stdout. | stdout |
| `-c, --clipboard` | Copy generated context to the system clipboard. | `false` |
| `-t, --tree` | Prepend an ASCII directory tree of included files. | `false` |
| `--stdin-list` | Read newline-delimited file paths from stdin instead of walking `PATH`. | `false` |
| `--show-excluded` | Print files excluded by the token budget to stderr. | `false` |
| `--entry-point <FILE>` | Anchor included files to a starting Rust file and its dependency chain. | None |
| `--dep-depth <DEPTH>` | Maximum hop depth for dependency traversal. | `3` |
| `--with-dependents` | Include reverse dependencies (files that depend on `--entry-point`). | `false` |
| `--strip-tests` | Strip `#[cfg(test)]` modules and `#[test]` functions from Rust files. | `false` |
| `--signatures-only` | Collapse Rust function bodies to `{ /* ... */ }`. | `false` |
| `--no-redact` | Disable automatic redaction of API keys and PEM private keys. | `false` |
| `--no-gitignore` | Disable automatic `.gitignore` filtering. | `false` |
| `--no-default-excludes` | Include lockfiles and minified files excluded by default. | `false` |
| `-h, --help` | Print help information. | — |
| `-V, --version` | Print version information. | — |