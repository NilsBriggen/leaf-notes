//! Locked sections: the pages are encrypted with AES-256-GCM under a key derived from the
//! password (PBKDF2-HMAC-SHA256). Only the encrypted form is ever saved or synced; the password
//! is never stored, and a forgotten password can't be recovered.
use crate::model::{Lock, Page};
use base64::Engine;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use std::num::NonZeroU32;

const ROUNDS: u32 = 300_000;
pub type Key = [u8; 32];

fn b64() -> base64::engine::GeneralPurpose { base64::engine::general_purpose::STANDARD }

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

/// The key for a password with this lock's salt.
pub fn key(password: &str, salt_b64: &str) -> Result<Key, String> {
    let salt = b64().decode(salt_b64).map_err(|e| e.to_string())?;
    let mut k = [0u8; 32];
    ring::pbkdf2::derive(ring::pbkdf2::PBKDF2_HMAC_SHA256, NonZeroU32::new(ROUNDS).expect("non-zero"), &salt, password.as_bytes(), &mut k);
    Ok(k)
}

/// A new salt (once per locked section).
pub fn new_salt() -> Result<String, String> { Ok(b64().encode(random::<16>()?)) }

/// Encrypt pages under `key` (fresh nonce every time).
pub fn seal(key: &Key, salt: &str, pages: &[Page]) -> Result<Lock, String> {
    let mut data = serde_json::to_vec(pages).map_err(|e| e.to_string())?;
    let nonce = random::<12>()?;
    let k = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).map_err(|_| "bad key".to_string())?);
    k.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut data).map_err(|_| "encryption failed".to_string())?;
    Ok(Lock { salt: salt.to_string(), nonce: b64().encode(nonce), data: b64().encode(data) })
}

/// Decrypt; a wrong password (or damaged data) is an error, never garbage.
pub fn open(key: &Key, lock: &Lock) -> Result<Vec<Page>, String> {
    let nonce: [u8; 12] = b64().decode(&lock.nonce).ok().and_then(|n| n.try_into().ok()).ok_or("damaged lock")?;
    let mut data = b64().decode(&lock.data).map_err(|_| "damaged lock")?;
    let k = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).map_err(|_| "bad key")?);
    let plain = k.open_in_place(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut data).map_err(|_| "wrong password".to_string())?;
    serde_json::from_slice(plain).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seals_and_opens() {
        let salt = new_salt().unwrap();
        let k = key("correct horse", &salt).unwrap();
        let pages = vec![Page::new("Diary"), Page::new("Plans")];
        let lock = seal(&k, &salt, &pages).unwrap();
        assert!(!lock.data.contains("Diary"), "nothing readable in the saved form");
        let back = open(&k, &lock).unwrap();
        assert_eq!(back.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Diary", "Plans"]);
        assert!(open(&key("wrong", &salt).unwrap(), &lock).is_err());
        let mut broken = lock.clone();
        broken.data.replace_range(4..8, "AAAA");
        assert!(open(&k, &broken).is_err(), "tampering is detected");
        assert_ne!(seal(&k, &salt, &pages).unwrap().nonce, lock.nonce, "a fresh nonce each time");
    }
}
