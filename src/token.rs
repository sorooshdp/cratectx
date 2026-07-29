/// Estimate token count using chars/4 heuristic
/// This is a rough approximation; real tokenizers will be added later
pub fn estimate_tokens(text: &str) -> usize {
    (text.chars().count() as f64 / 4.0).ceil() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_short() {
        assert_eq!(estimate_tokens("hi"), 1); // 2 chars / 4 = 0.5 -> ceil -> 1
    }

    #[test]
    fn test_estimate_tokens_typical() {
        let code = "fn main() {\n    println!(\"Hello\");\n}";
        let tokens = estimate_tokens(code);
        assert!(tokens > 0);
        assert!(tokens < code.len()); // tokens should be less than char count
    }

    #[test]
    fn test_estimate_tokens_multibyte() {
        // Unicode characters should count as single chars
        assert_eq!(estimate_tokens("café"), 1); // 4 chars / 4 = 1
        assert_eq!(estimate_tokens("你好世界"), 1); // 4 chars / 4 = 1
    }
}
