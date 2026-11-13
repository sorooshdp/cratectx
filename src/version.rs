use std::process::Command;
use std::sync::OnceLock;

pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn git_version() -> Option<String> {
    static GIT_HASH: OnceLock<Option<String>> = OnceLock::new();
    GIT_HASH
        .get_or_init(|| {
            if let Some(compile_hash) = option_env!("GIT_HASH") {
                let trimmed = compile_hash.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }

            let output = Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()?;

            if !output.status.success() {
                return None;
            }

            let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if hash.is_empty() {
                None
            } else {
                Some(hash)
            }
        })
        .clone()
}

pub fn app_banner() -> String {
    let version = app_version();
    match git_version() {
        Some(git) => format!("cratectx v{version} (git: {git})"),
        None => format!("cratectx v{version}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_version_is_set() {
        assert!(!app_version().is_empty());
    }

    #[test]
    fn app_banner_is_descriptive() {
        let banner = app_banner();
        assert!(banner.starts_with("cratectx v"));
    }

    #[test]
    fn git_version_is_consistent_across_calls() {
        let first = git_version();
        let second = git_version();
        assert_eq!(first, second);
    }
}
