// src/formatter.rs
use anyhow::Result;
use clap::ValueEnum;
use crate::walker::FileEntry;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Format {
    Xml,
    Md,
    Plain,
}

pub fn format_files(files: &[(FileEntry, usize)], format: Format) -> Result<String> {
    match format {
        Format::Xml => format_xml(files),
        Format::Md => format_markdown(files),
        Format::Plain => format_plain(files),
    }
}

fn format_xml(files: &[(FileEntry, usize)]) -> Result<String> {
    let mut output = String::from("<context>\n");
    
    for (file, tokens) in files {
        output.push_str(&format!(
            "<file path=\"{}\" tokens=\"{}\">\n{}\n</file>\n",
            escape_xml(&file.path),
            tokens,
            escape_xml(&file.content)
        ));
    }
    
    output.push_str("</context>");
    Ok(output)
}

fn format_markdown(files: &[(FileEntry, usize)]) -> Result<String> {
    let mut output = String::new();
    
    for (file, tokens) in files {
        output.push_str(&format!("## {}\n", file.path));
        output.push_str(&format!("*({} tokens)*\n\n", tokens));
        output.push_str("```\n");
        output.push_str(&file.content);
        output.push_str("\n```\n\n");
    }
    
    Ok(output)
}

fn format_plain(files: &[(FileEntry, usize)]) -> Result<String> {
    let mut output = String::new();
    
    for (file, tokens) in files {
        output.push_str(&format!("=== {} ({} tokens) ===\n", file.path, tokens));
        output.push_str(&file.content);
        output.push_str("\n\n");
    }
    
    Ok(output)
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_files() -> Vec<(FileEntry, usize)> {
        vec![
            (FileEntry {
                path: "test.rs".to_string(),
                content: "fn main() {}".to_string(),
            }, 3),
            (FileEntry {
                path: "lib.rs".to_string(),
                content: "pub fn foo() {}".to_string(),
            }, 4),
        ]
    }

    #[test]
    fn test_format_xml() {
        let files = make_test_files();
        let output = format_xml(&files).unwrap();
        
        assert!(output.starts_with("<context>"));
        assert!(output.ends_with("</context>"));
        assert!(output.contains("<file path=\"test.rs\" tokens=\"3\">"));
        assert!(output.contains("fn main() {}"));
    }

    #[test]
    fn test_format_markdown() {
        let files = make_test_files();
        let output = format_markdown(&files).unwrap();
        
        assert!(output.contains("## test.rs"));
        assert!(output.contains("*(3 tokens)*"));
        assert!(output.contains("```"));
        assert!(output.contains("fn main() {}"));
    }

    #[test]
    fn test_format_plain() {
        let files = make_test_files();
        let output = format_plain(&files).unwrap();
        
        assert!(output.contains("=== test.rs (3 tokens) ==="));
        assert!(output.contains("fn main() {}"));
    }

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("<hello>"), "&lt;hello&gt;");
        assert_eq!(escape_xml("a & b"), "a &amp; b");
        assert_eq!(escape_xml("\"quoted\""), "&quot;quoted&quot;");
    }

    #[test]
    fn test_xml_with_special_chars() {
        let files = vec![
            (FileEntry {
                path: "test<>.rs".to_string(),
                content: "let x = \"<tag>\";".to_string(),
            }, 5),
        ];
        let output = format_xml(&files).unwrap();
        
        assert!(output.contains("&lt;tag&gt;"));
        assert!(!output.contains("<tag>")); // should be escaped
    }
}
