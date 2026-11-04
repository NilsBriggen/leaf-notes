//! Passwords, keys and sign-ins live in the system keyring: Windows Credential Manager, the
//! Secret Service on Linux (GNOME Keyring, KWallet) or the macOS Keychain. The settings files
//! keep an empty field instead. Where no keyring answers (Android, whose app folder no other app
//! can read, or a Linux without a keyring running) the secret stays in the file, so it's never lost.
use std::collections::HashMap;
use std::sync::Mutex;

/// What this run knows is in the keyring. A name that's missing couldn't be read (keyring locked
/// or absent), so it's never deleted.
static KNOWN: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
/// Set after the keyring has failed once, so later saves don't keep waiting on it.
static BROKEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(cfg!(target_os = "android"));

fn known(name: &str) -> Option<String> { KNOWN.lock().ok()?.as_ref()?.get(name).cloned() }
#[cfg_attr(target_os = "android", allow(dead_code))]
fn remember(name: &str, v: &str) { if let Ok(mut k) = KNOWN.lock() { k.get_or_insert_with(HashMap::new).insert(name.into(), v.into()); } }
fn broken() -> bool { BROKEN.load(std::sync::atomic::Ordering::Relaxed) }
#[cfg_attr(target_os = "android", allow(dead_code))]
fn fail() { BROKEN.store(true, std::sync::atomic::Ordering::Relaxed); }

/// Where secrets are kept right now, for the settings window.
pub fn place() -> &'static str { crate::tr(if broken() { "this device's Leaf Notes folder (no system keyring available)" } else { "your system's keyring" }) }

/// Fill `value` from the keyring when the file had none (a value still in an older file wins, and moves over on the next save).
pub fn load(name: &str, value: &mut String) {
    if broken() { return; }
    #[cfg(not(target_os = "android"))]
    match keyring::Entry::new("Leaf Notes", name).and_then(|e| e.get_password()) {
        Ok(v) => { remember(name, &v); if value.is_empty() { *value = v; } }
        Err(keyring::Error::NoEntry) => remember(name, ""),
        Err(_) => fail(),
    }
    #[cfg(target_os = "android")]
    let _ = (name, value);
}

/// Keep `value` in the keyring. Returns what the file should hold: nothing once it's in the keyring, the value itself if there's no keyring.
pub fn store(name: &str, value: &str) -> String {
    if broken() { return value.to_string(); }
    let was = known(name);
    if was.as_deref() == Some(value) { return String::new(); }
    #[cfg(not(target_os = "android"))]
    {
        let Ok(entry) = keyring::Entry::new("Leaf Notes", name) else { fail(); return value.to_string() };
        if value.is_empty() {
            // only remove what's known to be there: a keyring that was locked at start keeps its sign-in
            if was.is_some() && matches!(entry.delete_credential(), Ok(()) | Err(keyring::Error::NoEntry)) { remember(name, ""); }
            return String::new();
        }
        match entry.set_password(value) {
            Ok(()) => { remember(name, value); String::new() }
            Err(_) => { fail(); value.to_string() }
        }
    }
    #[cfg(target_os = "android")]
    value.to_string()
}

#[cfg(test)]
mod tests {
    /// Writes, reads and removes a throwaway entry in the real keyring (run by hand: it touches the system keyring).
    #[test]
    #[ignore]
    fn keyring_round_trip() {
        let name = "selftest";
        assert_eq!(super::store(name, "s3cret"), "", "kept in {}", super::place());
        assert_eq!(super::store(name, "s3cret"), "", "an unchanged value isn't written again");
        let mut v = String::new();
        super::load(name, &mut v);
        assert_eq!(v, "s3cret");
        assert_eq!(super::store(name, ""), "");
        let mut gone = String::new();
        super::load(name, &mut gone);
        assert_eq!(gone, "");
    }
}
