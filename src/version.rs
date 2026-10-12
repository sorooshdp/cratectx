use std::process::Command;

pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn git_version() -> Option<String> {
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
}
