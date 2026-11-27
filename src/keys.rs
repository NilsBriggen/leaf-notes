//! Passwords, keys and sign-ins live in the system keyring: Windows Credential Manager, the
//! Secret Service on Linux (GNOME Keyring, KWallet) or the macOS Keychain. The settings files
//! keep an empty field instead. Where no keyring answers (Android, whose app folder no other app
//! can read, or a Linux without a keyring running) the secret stays in the file, so it's never lost.
use std::collections::HashMap;
use std::sync::Mutex;

/// What this run knows is in the keyring. A name that's missing couldn't be read (keyring locked
/// or absent), so it's never deleted.
static KNOWN: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
/// Android: original `aks:...` ciphertext if `unprotect` transiently failed on load, so we never wipe it on save.
#[cfg(target_os = "android")]
static PRESERVED_SEALED: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
/// True only when the platform genuinely has no usable keyring backend (not transient lock/dismiss).
static NO_KEYRING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(cfg!(target_os = "android"));

fn known(name: &str) -> Option<String> { KNOWN.lock().ok()?.as_ref()?.get(name).cloned() }
#[cfg_attr(target_os = "android", allow(dead_code))]
fn remember(name: &str, v: &str) { if let Ok(mut k) = KNOWN.lock() { k.get_or_insert_with(HashMap::new).insert(name.into(), v.into()); } }
fn no_keyring() -> bool { NO_KEYRING.load(std::sync::atomic::Ordering::Relaxed) }

/// Where secrets are kept right now, for the settings window.
pub fn place() -> &'static str {
    if cfg!(target_os = "android") { return crate::tr("Android's key store"); }
    crate::tr(if no_keyring() { "this device's Leaf Notes folder (no system keyring available)" } else { "your system's keyring" })
}

/// Fill `value` from the keyring when the file had none (a value still in an older file wins, and moves over on the next save).
pub fn load(name: &str, value: &mut String) {
    #[cfg(target_os = "android")]
    if let Some(sealed) = value.strip_prefix("aks:") {
        match crate::droid::unprotect(sealed) {
            Some(plain) => { *value = plain; }
            None => {
                // Transient keystore failure (e.g. device locked at background start): preserve original ciphertext!
                if let Ok(mut p) = PRESERVED_SEALED.lock() {
                    p.get_or_insert_with(HashMap::new).insert(name.into(), value.clone());
                }
                value.clear();
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    match keyring::Entry::new("Leaf Notes", name).and_then(|e| e.get_password()) {
        Ok(v) => {
            NO_KEYRING.store(false, std::sync::atomic::Ordering::Relaxed);
            remember(name, &v);
            if value.is_empty() { *value = v; }
        }
        Err(keyring::Error::NoEntry) => {
            NO_KEYRING.store(false, std::sync::atomic::Ordering::Relaxed);
            remember(name, "");
        }
        Err(keyring::Error::NoStorageAccess(_)) => {
            // Transiently locked or dismissed: do NOT mark keyring permanently broken.
        }
        Err(keyring::Error::PlatformFailure(_)) => {
            // Only treat platform failure as absent keyring when no entry could be reached.
            NO_KEYRING.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        Err(_) => {}
    }
    #[cfg(target_os = "android")]
    let _ = (name, value);
}

/// Keep `value` in the keyring. Returns what the file should hold: nothing once it's in the keyring,
/// or the value itself if there's genuinely no keyring backend.
pub fn store(name: &str, value: &str) -> String {
    #[cfg(target_os = "android")]
    {
        static SEALED: Mutex<Option<HashMap<String, (String, String)>>> = Mutex::new(None);
        if value.is_empty() {
            // If `unprotect` transiently failed at startup, keep the original `aks:` ciphertext intact rather than wiping it.
            if let Ok(p) = PRESERVED_SEALED.lock() {
                if let Some(orig) = p.as_ref().and_then(|m| m.get(name)) {
                    return orig.clone();
                }
            }
            return String::new();
        }
        if let Ok(mut p) = PRESERVED_SEALED.lock() {
            if let Some(m) = p.as_mut() { m.remove(name); }
        }
        let mut cache = SEALED.lock().unwrap_or_else(|e| e.into_inner());
        let cache = cache.get_or_insert_with(HashMap::new);
        if let Some((plain, sealed)) = cache.get(name) { if plain == value { return format!("aks:{sealed}"); } }
        return match crate::droid::protect(value) {
            Some(sealed) => { cache.insert(name.into(), (value.into(), sealed.clone())); format!("aks:{sealed}") }
            None => value.to_string(),
        };
    }
    #[allow(unreachable_code)]
    let was = known(name);
    if was.as_deref() == Some(value) { return String::new(); }
    #[cfg(not(target_os = "android"))]
    {
        let Ok(entry) = keyring::Entry::new("Leaf Notes", name) else {
            NO_KEYRING.store(true, std::sync::atomic::Ordering::Relaxed);
            return value.to_string();
        };
        if value.is_empty() {
            if was.is_some() && matches!(entry.delete_credential(), Ok(()) | Err(keyring::Error::NoEntry)) { remember(name, ""); }
            return String::new();
        }
        match entry.set_password(value) {
            Ok(()) => {
                NO_KEYRING.store(false, std::sync::atomic::Ordering::Relaxed);
                remember(name, value);
                String::new()
            }
            Err(keyring::Error::NoStorageAccess(_)) => {
                // Transiently locked keyring: do NOT silently downgrade to plaintext in JSON if it was already in the keyring!
                if was.is_some() { String::new() } else { value.to_string() }
            }
            Err(_) => {
                NO_KEYRING.store(true, std::sync::atomic::Ordering::Relaxed);
                value.to_string()
            }
        }
    }
    #[cfg(target_os = "android")]
    value.to_string()
}

#[cfg(test)]
mod tests {
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
