use std::cmp::Reverse;
use crate::walker::FileEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityStrategy {
    /// Include files in order they're discovered (current behavior)
    FirstCome,
    /// Prioritize smaller files first to maximize file count
    SmallestFirst,
    /// Prioritize by depth (shallower files first)
    ShallowFirst,
    /// Prioritize by depth (deeper files first, usually implementation details)
    DeepFirst,
}

impl PriorityStrategy {
    pub fn from_str(s: &str) -> Option<Self> {
        if s.eq_ignore_ascii_case("first-come") || s.eq_ignore_ascii_case("firstcome") {
            Some(Self::FirstCome)
        } else if s.eq_ignore_ascii_case("smallest-first")
            || s.eq_ignore_ascii_case("smallestfirst")
            || s.eq_ignore_ascii_case("smallest")
        {
            Some(Self::SmallestFirst)
        } else if s.eq_ignore_ascii_case("shallow-first")
            || s.eq_ignore_ascii_case("shallowfirst")
            || s.eq_ignore_ascii_case("shallow")
        {
            Some(Self::ShallowFirst)
        } else if s.eq_ignore_ascii_case("deep-first")
            || s.eq_ignore_ascii_case("deepfirst")
            || s.eq_ignore_ascii_case("deep")
        {
            Some(Self::DeepFirst)
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct BudgetResult {
    pub included: Vec<(FileEntry, usize)>,
    pub excluded: Vec<FileEntry>,
    pub total_tokens: usize,
}

#[inline]
fn path_depth(path: &str) -> usize {
    path.bytes().filter(|&b| b == b'/' || b == b'\\').count()
}

/// Apply token budget with prioritization strategy
pub fn apply_budget(
    files: Vec<FileEntry>,
    max_tokens: Option<usize>,
    strategy: PriorityStrategy,
    token_estimator: impl Fn(&str) -> usize,
) -> BudgetResult {
    let mut file_tokens: Vec<(FileEntry, usize)> = files
        .into_iter()
        .map(|f| {
            let tokens = token_estimator(&f.content);
            (f, tokens)
        })
        .collect();

    // Sort by priority strategy using O(N) cached keys
    match strategy {
        PriorityStrategy::FirstCome => {}
        PriorityStrategy::SmallestFirst => {
            file_tokens.sort_by_key(|&(_, tokens)| tokens);
        }
        PriorityStrategy::ShallowFirst => {
            file_tokens.sort_by_cached_key(|(f, _)| path_depth(&f.path));
        }
        PriorityStrategy::DeepFirst => {
            file_tokens.sort_by_cached_key(|(f, _)| Reverse(path_depth(&f.path)));
        }
    }

    let mut included = Vec::with_capacity(file_tokens.len());
    let mut excluded = Vec::new();
    let mut total_tokens: usize = 0;

    if let Some(max) = max_tokens {
        for (file, tokens) in file_tokens {
            if total_tokens.saturating_add(tokens) <= max {
                total_tokens += tokens;
                included.push((file, tokens));
            } else {
                excluded.push(file);
            }
        }
    } else {
        for (file, tokens) in file_tokens {
            total_tokens += tokens;
            included.push((file, tokens));
        }
    }

    BudgetResult {
        included,
        excluded,
        total_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_file(path: &str, content: &str) -> FileEntry {
        FileEntry {
            path: path.to_string(),
            content: content.to_string(),
        }
    }

    fn simple_estimator(content: &str) -> usize {
        content.len().div_ceil(4)
    }

    #[test]
    fn test_no_budget_includes_all() {
        let files = vec![
            make_file("a.rs", "short"),
            make_file("b.rs", "medium content"),
            make_file("c.rs", "very long content here"),
        ];

        let result = apply_budget(files, None, PriorityStrategy::FirstCome, simple_estimator);

        assert_eq!(result.included.len(), 3);
        assert_eq!(result.excluded.len(), 0);
    }

    #[test]
    fn test_first_come_order() {
        let files = vec![
            make_file("large.rs", &"x".repeat(100)),
            make_file("small.rs", "x"),
            make_file("medium.rs", &"x".repeat(50)),
        ];

        let result = apply_budget(
            files,
            Some(30),
            PriorityStrategy::FirstCome,
            simple_estimator,
        );

        assert_eq!(result.included.len(), 2);
        assert_eq!(result.included[0].0.path, "large.rs");
        assert_eq!(result.included[1].0.path, "small.rs");
        assert_eq!(result.excluded.len(), 1);
        assert_eq!(result.excluded[0].path, "medium.rs");
    }

    #[test]
    fn test_smallest_first_strategy() {
        let files = vec![
            make_file("large.rs", &"x".repeat(100)),
            make_file("small.rs", "x"),
            make_file("medium.rs", &"x".repeat(50)),
        ];

        let result = apply_budget(
            files,
            Some(30),
            PriorityStrategy::SmallestFirst,
            simple_estimator,
        );

        assert_eq!(result.included.len(), 2);
        assert_eq!(result.included[0].0.path, "small.rs");
        assert_eq!(result.included[1].0.path, "medium.rs");
        assert_eq!(result.excluded.len(), 1);
        assert_eq!(result.excluded[0].path, "large.rs");
    }

    #[test]
    fn test_shallow_first_strategy() {
        let files = vec![
            make_file("root.rs", &"x".repeat(20)),
            make_file("a/b/c/deep.rs", &"x".repeat(20)),
            make_file("a/mid.rs", &"x".repeat(20)),
        ];

        let result = apply_budget(
            files,
            Some(12),
            PriorityStrategy::ShallowFirst,
            simple_estimator,
        );

        assert_eq!(result.included.len(), 2);
        assert_eq!(result.included[0].0.path, "root.rs");
        assert_eq!(result.included[1].0.path, "a/mid.rs");
        assert_eq!(result.excluded[0].path, "a/b/c/deep.rs");
    }

    #[test]
    fn test_deep_first_strategy() {
        let files = vec![
            make_file("root.rs", &"x".repeat(20)),
            make_file("a/b/c/deep.rs", &"x".repeat(20)),
            make_file("a/mid.rs", &"x".repeat(20)),
        ];

        let result = apply_budget(
            files,
            Some(12),
            PriorityStrategy::DeepFirst,
            simple_estimator,
        );

        assert_eq!(result.included.len(), 2);
        assert_eq!(result.included[0].0.path, "a/b/c/deep.rs");
        assert_eq!(result.included[1].0.path, "a/mid.rs");
        assert_eq!(result.excluded[0].path, "root.rs");
    }

    #[test]
    fn test_exact_budget_fit() {
        let files = vec![make_file("a.rs", "xxxx"), make_file("b.rs", "xxxx")];

        let result = apply_budget(files, Some(2), PriorityStrategy::FirstCome, simple_estimator);

        assert_eq!(result.included.len(), 2);
        assert_eq!(result.total_tokens, 2);
    }

    #[test]
    fn test_single_file_exceeds_budget() {
        let files = vec![make_file("huge.rs", &"x".repeat(1000))];

        let result = apply_budget(
            files,
            Some(10),
            PriorityStrategy::FirstCome,
            simple_estimator,
        );

        assert_eq!(result.included.len(), 0);
        assert_eq!(result.excluded.len(), 1);
        assert_eq!(result.total_tokens, 0);
    }

    #[test]
    fn test_strategy_from_str() {
        assert_eq!(
            PriorityStrategy::from_str("first-come"),
            Some(PriorityStrategy::FirstCome)
        );
        assert_eq!(
            PriorityStrategy::from_str("SMALLEST-FIRST"),
            Some(PriorityStrategy::SmallestFirst)
        );
        assert_eq!(
            PriorityStrategy::from_str("shallow"),
            Some(PriorityStrategy::ShallowFirst)
        );
        assert_eq!(
            PriorityStrategy::from_str("deep-first"),
            Some(PriorityStrategy::DeepFirst)
        );
        assert_eq!(PriorityStrategy::from_str("invalid"), None);
    }

    #[test]
    fn test_empty_files_list() {
        let files: Vec<FileEntry> = vec![];

        let result = apply_budget(
            files,
            Some(1000),
            PriorityStrategy::FirstCome,
            simple_estimator,
        );

        assert_eq!(result.included.len(), 0);
        assert_eq!(result.excluded.len(), 0);
        assert_eq!(result.total_tokens, 0);
    }
}