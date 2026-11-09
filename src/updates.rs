//! "A new version is out": once per start, the list of releases is read from GitHub (only the
//! app's version is sent, as the User-Agent). Can be switched off in Settings ▸ Your data.
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

const RELEASES: &str = "https://api.github.com/repos/NilsBriggen/leaf-notes/releases?per_page=10";

/// (tag, where to get it, what's new) of the newest release, if it is newer than this version and
/// not the one the person chose to skip. "Where" is this platform's download when the release has one.
pub fn check(current: &'static str, skip: String) -> Receiver<Option<(String, String, String)>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).build().into();
        let newest = agent.get(RELEASES).header("user-agent", format!("LeafNotes/{current}")).header("accept", "application/vnd.github+json").call().ok()
            .and_then(|mut r| r.body_mut().read_json::<serde_json::Value>().ok())
            .and_then(|v| v.as_array().cloned())
            .and_then(|list| list.into_iter().filter(|r| r["draft"] != true && (r["prerelease"] != true || current.contains('-'))).filter_map(|r| {
                let file = if cfg!(target_os = "android") { ".apk" } else if cfg!(windows) { ".msi" } else { ".AppImage" };
                let direct = r["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"].as_str().is_some_and(|n| n.ends_with(file)))).and_then(|x| x["browser_download_url"].as_str()).map(String::from);
                let notes: String = r["body"].as_str().unwrap_or("").lines().filter(|l| !l.trim().is_empty()).take(8).collect::<Vec<_>>().join("\n");
                Some((r["tag_name"].as_str()?.to_string(), direct.or_else(|| r["html_url"].as_str().map(String::from))?, notes))
            }).max_by(|a, b| cmp(&a.0, &b.0)))
            .filter(|(tag, ..)| cmp(tag, current) == std::cmp::Ordering::Greater && *tag != skip);
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
