# cratectx

cratectx is a Rust CLI tool that scans a codebase and packages selected files into compact, LLM-friendly context.

## What it does

- Walks a directory tree and collects source files
- Applies include/exclude filters
- Respects .gitignore by default
- Enforces a token budget and trims content using priority strategies
- Supports plain-text and XML output
- Can prepend a directory tree summary and optionally copy output to the clipboard

## Key features

- Token-aware packing for large codebases
- Priority strategies:
  - `smallest-first`
  - `shallow-first`
  - `deep-first`
  - `first-come`
- Flexible filtering with `--include` and `--exclude`
- Optional tree preview with `--tree`
- Dependency-aware entry-point traversal via `--entry-point`
- Stdin file list selection via `--stdin-list`
- Output to stdout, a file, or the clipboard

## Example usage

```bash
cargo run -- . --format xml --max-tokens 5000
```

```bash
cargo run -- src --include "*.rs" --exclude target --tree --format plain
```

```bash
cargo run -- src --entry-point src/lib.rs --dep-depth 2 --format plain
```

```bash
printf '%s\n' src/lib.rs src/budget.rs | cargo run -- src --stdin-list --format md
```

## CLI options

- `PATH`: directory to scan (defaults to `.`)
- `-i, --include <PATTERN>`: include files matching the given glob pattern; repeatable
- `-e, --exclude <PATTERN>`: exclude files matching the given glob pattern; repeatable
- `-m, --max-tokens <N>`: limit the output size by estimated token count
- `-p, --priority <STRATEGY>`: choose how files are dropped when the budget is exceeded
  - `first-come`
  - `smallest-first`
  - `shallow-first`
  - `deep-first`
- `-f, --format <FORMAT>`: output format
  - `xml`
  - `plain`
  - `md`
- `-o, --output <FILE>`: write the generated context to a file
- `-c, --clipboard`: copy the result to the clipboard
- `--no-gitignore`: disable automatic `.gitignore` respect
- `-t, --tree`: include a directory tree summary before the file contents
- `--stdin-list`: read newline-delimited file paths from stdin instead of walking the directory tree
- `--show-excluded`: print files that were excluded due to the token budget
- `--entry-point <FILE>`: keep the selected files anchored to a starting file and include its dependency chain
- `--dep-depth <N>`: maximum depth for dependency traversal (default: `3`)
- `--with-dependents`: include files that depend on the entry point as well as its dependencies

## Build

```bash
cargo build
```

## Run

```bash
cargo run -- --help
```
