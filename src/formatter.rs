use crate::walker::FileEntry;
use anyhow::Result;
use clap::ValueEnum;
use std::borrow::Cow;
use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Xml,
    Md,
    Plain,
}

pub fn format_files(files: &[(FileEntry, usize)], format: Format) -> Result<String> {
    let estimated_cap = estimate_output_capacity(files);
    let mut output = String::with_capacity(estimated_cap);
    format_files_into(files, format, &mut output)?;
    Ok(output)
}

/// Appends formatted file contents directly into an existing `String` buffer,
/// avoiding secondary full-context allocations when combined with `--tree`.
pub fn format_files_into(
    files: &[(FileEntry, usize)],
    format: Format,
    output: &mut String,
) -> Result<()> {
    output.reserve(estimate_output_capacity(files));
    match format {
        Format::Xml => format_xml_into(files, output),
        Format::Md => format_markdown_into(files, output),
        Format::Plain => format_plain_into(files, output),
    }
}

fn estimate_output_capacity(files: &[(FileEntry, usize)]) -> usize {
    files
        .iter()
        .map(|(f, _)| f.content.len() + f.path.len() + 64)
        .sum::<usize>()
        + 32
}

fn format_xml_into(files: &[(FileEntry, usize)], output: &mut String) -> Result<()> {
    output.push_str("<context>\n");

    for (file, tokens) in files {
        output.push_str("<file path=\"");
        escape_xml_into(&file.path, output);
        let _ = write!(output, "\" tokens=\"{tokens}\">\n");
        escape_xml_into(&file.content, output);
        output.push_str("\n</file>\n");
    }

    output.push_str("</context>");
    Ok(())
}

fn format_markdown_into(files: &[(FileEntry, usize)], output: &mut String) -> Result<()> {
    for (file, tokens) in files {
        let _ = writeln!(output, "## {}", file.path);
        let _ = writeln!(output, "*({tokens} tokens)*\n");
        output.push_str("```\n");
        output.push_str(&file.content);
        output.push_str("\n```\n\n");
    }

    Ok(())
}

fn format_plain_into(files: &[(FileEntry, usize)], output: &mut String) -> Result<()> {
    for (file, tokens) in files {
        let _ = writeln!(output, "=== {} ({tokens} tokens) ===", file.path);
        output.push_str(&file.content);
        output.push_str("\n\n");
    }

    Ok(())
}

/// Single-pass XML escaping directly into `out`.
/// Copies contiguous unescaped slices in bulk instead of character-by-character.
#[inline]
fn escape_xml_into(s: &str, out: &mut String) {
    let bytes = s.as_bytes();
    let mut last = 0;

    for (i, &b) in bytes.iter().enumerate() {
        let replacement = match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            b'\'' => "&apos;",
            _ => continue,
        };
        if last < i {
            out.push_str(&s[last..i]);
        }
        out.push_str(replacement);
        last = i + 1;
    }

    if last < s.len() {
        out.push_str(&s[last..]);
    }
}

/// Returns a `Cow<str>` that borrows `s` without allocating if no XML special characters exist.
pub fn escape_xml(s: &str) -> Cow<'_, str> {
    if !s
        .bytes()
        .any(|b| matches!(b, b'&' | b'<' | b'>' | b'"' | b'\''))
    {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + (s.len() >> 3));
    escape_xml_into(s, &mut out);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_files() -> Vec<(FileEntry, usize)> {
        vec![
            (
                FileEntry {
                    path: "test.rs".to_string(),
                    content: "fn main() {}".to_string(),
                },
                3,
            ),
            (
                FileEntry {
                    path: "lib.rs".to_string(),
                    content: "pub fn foo() {}".to_string(),
                },
                4,
            ),
        ]
    }

    #[test]
    fn test_format_xml() {
        let files = make_test_files();
        let output = format_files(&files, Format::Xml).unwrap();

        assert!(output.starts_with("<context>"));
        assert!(output.ends_with("</context>"));
        assert!(output.contains("<file path=\"test.rs\" tokens=\"3\">"));
        assert!(output.contains("fn main() {}"));
    }

    #[test]
    fn test_format_markdown() {
        let files = make_test_files();
        let output = format_files(&files, Format::Md).unwrap();

        assert!(output.contains("## test.rs"));
        assert!(output.contains("*(3 tokens)*"));
        assert!(output.contains("```"));
        assert!(output.contains("fn main() {}"));
    }

    #[test]
    fn test_format_plain() {
        let files = make_test_files();
        let output = format_files(&files, Format::Plain).unwrap();

        assert!(output.contains("=== test.rs (3 tokens) ==="));
        assert!(output.contains("fn main() {}"));
    }

    #[test]
    fn test_escape_xml_borrows_when_clean() {
        let clean = "src/main.rs";
        let escaped = escape_xml(clean);
        assert!(matches!(escaped, Cow::Borrowed(_)));
        assert_eq!(escaped, "src/main.rs");
    }

    #[test]
    fn test_escape_xml_special_chars() {
        assert_eq!(escape_xml("<hello>"), "&lt;hello&gt;");
        assert_eq!(escape_xml("a & b"), "a &amp; b");
        assert_eq!(escape_xml("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(escape_xml("'single'"), "&apos;single&apos;");
    }

    #[test]
    fn test_xml_with_special_chars() {
        let files = vec![(
            FileEntry {
                path: "test<>.rs".to_string(),
                content: "let x = \"<tag>\";".to_string(),
            },
            5,
        )];
        let output = format_files(&files, Format::Xml).unwrap();

        assert!(output.contains("&lt;tag&gt;"));
        assert!(!output.contains("<tag>"));
    }
}
