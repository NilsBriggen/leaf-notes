//! "A new version is out": once per start, the list of releases is read from GitHub (only the
//! app's version is sent, as the User-Agent). Can be switched off in Settings ▸ Your data.
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

const RELEASES: &str = "https://api.github.com/repos/NilsBriggen/leaf-notes/releases?per_page=10";

/// (tag, page) of the newest release, if it is newer than this version.
pub fn check(current: &'static str) -> Receiver<Option<(String, String)>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).build().into();
        let newest = agent.get(RELEASES).header("user-agent", format!("LeafNotes/{current}")).header("accept", "application/vnd.github+json").call().ok()
            .and_then(|mut r| r.body_mut().read_json::<serde_json::Value>().ok())
            .and_then(|v| v.as_array().cloned())
            .and_then(|list| list.into_iter().filter(|r| r["draft"] != true && (r["prerelease"] != true || current.contains('-'))).filter_map(|r| Some((r["tag_name"].as_str()?.to_string(), r["html_url"].as_str()?.to_string()))).max_by(|a, b| cmp(&a.0, &b.0)))
            .filter(|(tag, _)| cmp(tag, current) == std::cmp::Ordering::Greater);
        let _ = tx.send(newest);
    });
    rx
}

/// Compare versions like "v0.2.0-beta" and "0.1.2": numbers first; a pre-release comes before its release.
pub fn cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |v: &str| {
        let v = v.trim_start_matches('v');
        let (nums, pre) = v.split_once('-').map_or((v, ""), |(n, p)| (n, p));
        let n: Vec<u64> = nums.split('.').map(|x| x.parse().unwrap_or(0)).chain(std::iter::repeat(0)).take(3).collect();
        (n, pre.is_empty(), pre.to_string())
    };
    parse(a).cmp(&parse(b))
}

#[cfg(test)]
mod tests {
    use super::cmp;
    use std::cmp::Ordering::*;
    #[test]
    fn compares_versions() {
        assert_eq!(cmp("v0.2.0-beta", "0.1.2"), Greater);
        assert_eq!(cmp("v0.1.2-beta", "0.1.2"), Less, "a beta comes before the release");
        assert_eq!(cmp("v0.1.10", "0.1.9"), Greater);
        assert_eq!(cmp("v0.2.0", "0.2.0"), Equal);
    }
}
