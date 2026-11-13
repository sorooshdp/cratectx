/// Estimate token count using chars/4 heuristic.
/// Uses an O(1) byte-length fast path for ASCII strings and integer ceiling division.
#[inline]
pub fn estimate_tokens(text: &str) -> usize {
    let char_count = if text.is_ascii() {
        text.len()
    } else {
        text.chars().count()
    };
    char_count.div_ceil(4)
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
    fn test_estimate_tokens_exact_boundaries() {
        assert_eq!(estimate_tokens("a"), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn test_estimate_tokens_typical() {
        let code = "fn main() {\n    println!(\"Hello\");\n}";
        let tokens = estimate_tokens(code);
        assert!(tokens > 0);
        assert!(tokens < code.len());
    }

    #[test]
    fn test_estimate_tokens_multibyte() {
        // Unicode characters should count as single chars
        assert_eq!(estimate_tokens("café"), 1); // 4 chars / 4 = 1
        assert_eq!(estimate_tokens("你好世界"), 1); // 4 chars / 4 = 1
        assert_eq!(estimate_tokens("你好世界!"), 2); // 5 chars / 4 -> 2
    }
}
