use colored::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Debug)]
struct UpdateCache {
    latest_version: String,
    last_checked: u64,
}

fn get_cache_path() -> PathBuf {
    crate::paths::data_home().join("update_cache.json")
}

pub enum Status {
    Latest,
    UpdateAvailable(String),
    Ahead,
}

pub fn get_status() -> Status {
    let current_version = env!("CARGO_PKG_VERSION");
    let cache_path = get_cache_path();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // The cached latest version is trusted for 4 hours.
    let latest = if let Ok(content) = fs::read_to_string(&cache_path)
        && let Ok(cache) = serde_json::from_str::<UpdateCache>(&content)
        && now < cache.last_checked + 14400
    {
        Some(cache.latest_version)
    } else {
        // curl rather than an HTTP crate: one best-effort call is not worth 42 crates.
        // No curl, or no network, reads as offline.
        let output = std::process::Command::new("curl")
            .args([
                "-fsSL",
                "--connect-timeout",
                "2",
                "--max-time",
                "4",
                "-H",
                "User-Agent: contexere-cli",
                "https://api.github.com/repos/fajarhide/contexere/releases/latest",
            ])
            .output()
            .ok()
            .filter(|o| o.status.success());
        let latest = output.and_then(|o| latest_tag(&o.stdout));
        if let Some(v) = &latest {
            let cache = UpdateCache {
                latest_version: v.clone(),
                last_checked: now,
            };
            if let Ok(json) = serde_json::to_string(&cache) {
                let _ = fs::create_dir_all(cache_path.parent().unwrap());
                let _ = fs::write(&cache_path, json);
            }
        }
        latest
    };

    match latest {
        Some(v) => {
            if is_newer(&v, current_version) {
                Status::UpdateAvailable(v)
            } else if is_newer(current_version, &v) {
                Status::Ahead
            } else {
                Status::Latest
            }
        }
        None => Status::Latest, // offline
    }
}

fn latest_tag(body: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct GitHubRelease {
        tag_name: String,
    }
    let release: GitHubRelease = serde_json::from_slice(body).ok()?;
    Some(release.tag_name.trim_start_matches('v').to_string())
}

pub fn check() -> Option<String> {
    match get_status() {
        Status::UpdateAvailable(v) => Some(v),
        _ => None,
    }
}

fn is_newer(latest: &str, current: &str) -> bool {
    let parse_v = |v: &str| -> Vec<u32> {
        v.split('.')
            .map(|s| s.split('-').next().unwrap_or("0").parse().unwrap_or(0))
            .collect()
    };

    let v1 = parse_v(latest);
    let v2 = parse_v(current);

    for (a, b) in v1.iter().zip(v2.iter()) {
        if a > b {
            return true;
        }
        if a < b {
            return false;
        }
    }

    if v1 == v2 {
        let s1 = latest.split('-').nth(1).unwrap_or("");
        let s2 = current.split('-').nth(1).unwrap_or("");

        if s1.is_empty() && !s2.is_empty() {
            return true;
        } // "0.5.4" > "0.5.4-rc1"
        if !s1.is_empty() && s2.is_empty() {
            return false;
        } // "0.5.4-rc1" < "0.5.4"
        return s1 > s2; // "0.5.4-rc2" > "0.5.4-rc1"
    }

    v1.len() > v2.len()
}

pub fn print_notification(latest: &str) {
    println!(
        "\n  {} A new version of OMNI is available: {} → {}",
        "✨".yellow(),
        env!("CARGO_PKG_VERSION").bright_black(),
        latest.green().bold()
    );
    println!(
        "      Run: {} to upgrade.\n",
        "brew upgrade fajarhide/tap/contexere".cyan()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_version_from_a_release_and_nothing_from_anything_else() {
        let body = br#"{"tag_name":"v0.8.3","name":"x"}"#;
        assert_eq!(latest_tag(body).as_deref(), Some("0.8.3"));
        assert_eq!(latest_tag(b"<html>rate limited</html>"), None);
        assert_eq!(latest_tag(br#"{"message":"Not Found"}"#), None);
    }

    #[test]
    fn detects_newer_versions() {
        assert!(is_newer("0.5.3", "0.5.2"));
        assert!(is_newer("0.6.0", "0.5.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.5.2", "0.5.2"));
        assert!(!is_newer("0.5.1", "0.5.2"));

        assert!(is_newer("0.5.4", "0.5.4-rc1"));
        assert!(!is_newer("0.5.4-rc1", "0.5.4"));
        assert!(!is_newer("0.5.3", "0.5.4-rc1"));
        assert!(is_newer("0.5.4-rc2", "0.5.4-rc1"));
    }
}
