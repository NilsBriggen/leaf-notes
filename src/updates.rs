//! "A new version is out": once per start, the list of releases is read from GitHub (only the
//! app's version is sent, as the User-Agent). Can be switched off in Settings ▸ Your data.
use std::cmp::Ordering;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

const RELEASES: &str = "https://api.github.com/repos/NilsBriggen/leaf-notes/releases?per_page=10";

pub type UpdateResult = Result<Option<(String, String, String)>, String>;

/// Pick the best release asset URL for the current OS and CPU architecture.
fn pick_asset(assets: &[serde_json::Value]) -> Option<String> {
    let ext = if cfg!(target_os = "android") {
        ".apk"
    } else if cfg!(windows) {
        ".msi"
    } else {
        ".AppImage"
    };
    let arch_tags: &[&str] = match std::env::consts::ARCH {
        "aarch64" => &["aarch64", "arm64"],
        "x86_64" => &["x86_64", "amd64", "x64"],
        _ => &[],
    };
    let other_arch: &[&str] = match std::env::consts::ARCH {
        "aarch64" => &["x86_64", "amd64", "x64"],
        "x86_64" => &["aarch64", "arm64"],
        _ => &[],
    };
    let matching: Vec<&serde_json::Value> = assets
        .iter()
        .filter(|x| x["name"].as_str().is_some_and(|n| n.ends_with(ext)))
        .collect();
    // 1. Prefer an asset whose filename explicitly matches our CPU architecture.
    if let Some(a) = matching.iter().find(|x| {
        let lower = x["name"].as_str().unwrap_or("").to_ascii_lowercase();
        arch_tags.iter().any(|t| lower.contains(t))
    }) {
        return a["browser_download_url"].as_str().map(String::from);
    }
    // 2. Otherwise fall back to a generic (non-other-arch) asset with the matching extension.
    matching
        .iter()
        .find(|x| {
            let lower = x["name"].as_str().unwrap_or("").to_ascii_lowercase();
            !other_arch.iter().any(|t| lower.contains(t))
        })
        .and_then(|x| x["browser_download_url"].as_str())
        .map(String::from)
}

/// `(tag, where to get it, what's new)` of the newest release, if it is newer than `current` and
/// not `skip`. Returns `Err` on network/HTTP failure so manual checks don't falsely claim "up to date".
pub fn check(current: &'static str, skip: String) -> Receiver<UpdateResult> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let res = (|| -> UpdateResult {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(15)))
                .build()
                .into();
            let mut resp = agent
                .get(RELEASES)
                .header("user-agent", format!("LeafNotes/{current}"))
                .header("accept", "application/vnd.github+json")
                .call()
                .map_err(|e| crate::tf!("Couldn't check for updates: {e}", e = e))?;
            let status = resp.status();
            if !status.is_success() {
                return Err(crate::tf!("Couldn't check for updates ({status}).", status = status));
            }
            let v: serde_json::Value = resp
                .body_mut()
                .read_json()
                .map_err(|e| crate::tf!("Couldn't read the update response: {e}", e = e))?;
            let list = v.as_array().ok_or_else(|| crate::tr("Unexpected update response.").to_string())?;
            let newest = list
                .iter()
                .filter(|r| r["draft"] != true && (r["prerelease"] != true || current.contains('-')))
                .filter_map(|r| {
                    let direct = r["assets"].as_array().and_then(|a| pick_asset(a));
                    let notes: String = r["body"]
                        .as_str()
                        .unwrap_or("")
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .take(8)
                        .collect::<Vec<_>>()
                        .join("\n");
                    Some((
                        r["tag_name"].as_str()?.to_string(),
                        direct.or_else(|| r["html_url"].as_str().map(String::from))?,
                        notes,
                    ))
                })
                .max_by(|a, b| cmp(&a.0, &b.0))
                .filter(|(tag, ..)| cmp(tag, current) == Ordering::Greater && *tag != skip);
            Ok(newest)
        })();
        let _ = tx.send(res);
    });
    rx
}

/// Semver-aware version comparison (`v0.2.0-beta.10 > v0.2.0-beta.2`, pre-release < release).
pub fn cmp(a: &str, b: &str) -> Ordering {
    fn split(v: &str) -> (Vec<u64>, &str) {
        let v = v.trim().trim_start_matches('v');
        let v = v.split_once('+').map_or(v, |(core, _)| core);
        let (nums, pre) = v.split_once('-').map_or((v, ""), |(n, p)| (n, p));
        let n: Vec<u64> = nums.split('.').map(|x| x.parse().unwrap_or(0)).chain(std::iter::repeat(0)).take(3).collect();
        (n, pre)
    }
    let (na, pa) = split(a);
    let (nb, pb) = split(b);
    let ord = na.cmp(&nb);
    if ord != Ordering::Equal {
        return ord;
    }
    match (pa.is_empty(), pb.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => {
            let mut ia = pa.split('.');
            let mut ib = pb.split('.');
            loop {
                match (ia.next(), ib.next()) {
                    (None, None) => return Ordering::Equal,
                    (None, Some(_)) => return Ordering::Less,
                    (Some(_), None) => return Ordering::Greater,
                    (Some(xa), Some(xb)) => {
                        let o = match (xa.parse::<u64>(), xb.parse::<u64>()) {
                            (Ok(a), Ok(b)) => a.cmp(&b),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => xa.cmp(xb),
                        };
                        if o != Ordering::Equal {
                            return o;
                        }
                    }
                }
            }
        }
    }
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
        assert_eq!(cmp("v0.2.0-beta.10", "v0.2.0-beta.2"), Greater, "numeric prerelease identifiers");
        assert_eq!(cmp("v0.2.0-alpha.1", "v0.2.0-alpha"), Greater);
    }
}
