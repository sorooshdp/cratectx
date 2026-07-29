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
        match s.to_lowercase().as_str() {
            "first-come" | "firstcome" => Some(Self::FirstCome),
            "smallest-first" | "smallestfirst" | "smallest" => Some(Self::SmallestFirst),
            "shallow-first" | "shallowfirst" | "shallow" => Some(Self::ShallowFirst),
            "deep-first" | "deepfirst" | "deep" => Some(Self::DeepFirst),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct BudgetResult {
    pub included: Vec<(FileEntry, usize)>,
    pub excluded: Vec<FileEntry>,
    pub total_tokens: usize,
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

    // Sort by priority strategy
    match strategy {
        PriorityStrategy::FirstCome => {
            // Keep original order
        }
        PriorityStrategy::SmallestFirst => {
            file_tokens.sort_by(|a, b| a.1.cmp(&b.1));
        }
        PriorityStrategy::ShallowFirst => {
            file_tokens.sort_by(|a, b| {
                let depth_a = a.0.path.matches('/').count() + a.0.path.matches('\\').count();
                let depth_b = b.0.path.matches('/').count() + b.0.path.matches('\\').count();
                depth_a.cmp(&depth_b)
            });
        }
        PriorityStrategy::DeepFirst => {
            file_tokens.sort_by(|a, b| {
                let depth_a = a.0.path.matches('/').count() + a.0.path.matches('\\').count();
                let depth_b = b.0.path.matches('/').count() + b.0.path.matches('\\').count();
                depth_b.cmp(&depth_a)
            });
        }
    }

    let mut included = Vec::new();
    let mut excluded = Vec::new();
    let mut total_tokens = 0;

    if let Some(max) = max_tokens {
        for (file, tokens) in file_tokens {
            if total_tokens + tokens <= max {
                total_tokens += tokens;
                included.push((file, tokens));
            } else {
                excluded.push(file);
            }
        }
    } else {
        // No budget limit, include everything
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
        (content.len() as f64 / 4.0).ceil() as usize
    }

    #[test]
    fn test_no_budget_includes_all() {
        let files = vec![
            make_file("a.rs", "short"),
            make_file("b.rs", "medium content"),
            make_file("c.rs", "very long content here"),
        ];

        let result = apply_budget(
            files,
            None,
            PriorityStrategy::FirstCome,
            simple_estimator,
        );

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

        // large.rs (25 tokens) + small.rs (1 token) = 26 tokens
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

        // small.rs (1) + medium.rs (13) + large.rs (25) = 39, so only first two fit
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

        // root.rs (5) + a/mid.rs (5) = 10 tokens
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

        // a/b/c/deep.rs (5) + a/mid.rs (5) = 10 tokens
        assert_eq!(result.included.len(), 2);
        assert_eq!(result.included[0].0.path, "a/b/c/deep.rs");
        assert_eq!(result.included[1].0.path, "a/mid.rs");
        assert_eq!(result.excluded[0].path, "root.rs");
    }

    #[test]
    fn test_exact_budget_fit() {
        let files = vec![
            make_file("a.rs", "xxxx"), // 1 token
            make_file("b.rs", "xxxx"), // 1 token
        ];

        let result = apply_budget(
            files,
            Some(2),
            PriorityStrategy::FirstCome,
            simple_estimator,
        );

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
            PriorityStrategy::from_str("smallest-first"),
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
